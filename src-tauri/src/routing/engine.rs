use std::sync::Arc;

use arc_swap::ArcSwap;
use sqlx::SqlitePool;

use crate::config::settings::Settings;
use crate::error::AppError;
use crate::routing::matcher::{self, CompiledRule, MatchInput, Route};
use crate::storage::database;

#[derive(Debug, Clone)]
pub struct RuleSnapshot {
    pub profile_id: String,
    pub https_interception: bool,
    pub rules: Vec<CompiledRule>,
}

impl Default for RuleSnapshot {
    fn default() -> Self {
        Self {
            profile_id: String::new(),
            https_interception: false,
            rules: Vec::new(),
        }
    }
}

pub struct RuleEngine {
    current: ArcSwap<RuleSnapshot>,
}

impl RuleEngine {
    pub fn new() -> Self {
        Self {
            current: ArcSwap::from_pointee(RuleSnapshot::default()),
        }
    }

    pub fn snapshot(&self) -> arc_swap::Guard<Arc<RuleSnapshot>> {
        self.current.load()
    }

    pub fn store(&self, snapshot: RuleSnapshot) {
        self.current.store(Arc::new(snapshot));
    }

    pub fn route(&self, input: MatchInput<'_>) -> Option<Route> {
        let snapshot = self.snapshot();
        let rule = matcher::select(&snapshot.rules, input)?;
        Some(matcher::route_of(rule, input))
    }

    pub fn host_mapped(&self, host: &str, port: u16) -> bool {
        matcher::host_mapped(&self.snapshot().rules, host, port)
    }

    pub fn https_interception(&self) -> bool {
        self.snapshot().https_interception
    }

    pub fn destinations(&self) -> Vec<String> {
        let snapshot = self.snapshot();
        let mut origins = Vec::new();
        for rule in &snapshot.rules {
            let origin = rule.destination.origin();
            if !origins.iter().any(|item| item == &origin) {
                origins.push(origin);
            }
        }
        origins
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn publish(pool: &SqlitePool, engine: &RuleEngine, settings: &Settings) -> Result<(), AppError> {
    let stored = database::list_rules(pool, &settings.active_profile_id).await?;
    let mut rules = Vec::new();
    for rule in stored {
        if rule.enabled {
            rules.push(matcher::compile_rule(&rule)?);
        }
    }
    engine.store(RuleSnapshot {
        profile_id: settings.active_profile_id.clone(),
        https_interception: settings.https_interception,
        rules,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::matcher::CompiledRule;
    use crate::routing::rules::{parse_destination, parse_host_pattern, RuleInput};
    use crate::storage::database::{self, open_database};

    #[test]
    fn swapping_the_snapshot_changes_the_next_match() {
        let engine = RuleEngine::new();
        let input = MatchInput {
            host: "payments.api.company.com",
            port: 80,
            method: "GET",
            path: "/api/payments/123",
            query: Some("x=1"),
        };
        engine.store(RuleSnapshot {
            profile_id: "default".into(),
            https_interception: false,
            rules: vec![CompiledRule {
                id: "payments".into(),
                name: "payments".into(),
                priority: 10,
                host: parse_host_pattern("payments.api.company.com").unwrap(),
                source_port: None,
                methods: vec![],
                path_prefix: None,
                rewrite_from: None,
                rewrite_to: None,
                destination: parse_destination("http://127.0.0.1:5002").unwrap(),
            }],
        });
        let route = engine.route(input).unwrap();
        assert_eq!(route.destination_url, "http://127.0.0.1:5002/api/payments/123?x=1");
        engine.store(RuleSnapshot {
            profile_id: "default".into(),
            https_interception: true,
            rules: Vec::new(),
        });
        assert!(engine.route(input).is_none());
        assert!(engine.snapshot().https_interception);
    }

    #[tokio::test]
    async fn disable_reloads_the_snapshot_from_sqlite() {
        let dir = std::env::temp_dir().join(format!("ler-rules-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = open_database(&dir.join("router.db")).await.unwrap();
        let settings = database::ensure_defaults(&pool).await.unwrap();
        let engine = RuleEngine::new();
        let created = database::insert_rule(
            &pool,
            &settings.active_profile_id,
            &crate::routing::rules::validate_rule_input(&RuleInput {
                name: "payments".into(),
                enabled: true,
                priority: 10,
                source_host: "payments.api.company.com".into(),
                source_port: None,
                methods: vec![],
                path_prefix: None,
                rewrite_from: None,
                rewrite_to: None,
                destination: "http://localhost:5002".into(),
            })
            .unwrap(),
        )
        .await
        .unwrap();
        publish(&pool, &engine, &settings).await.unwrap();
        let input = MatchInput {
            host: "payments.api.company.com",
            port: 80,
            method: "GET",
            path: "/api/payments/123",
            query: None,
        };
        assert!(engine.route(input).is_some());
        database::set_rule_enabled(&pool, &created.id, false).await.unwrap();
        publish(&pool, &engine, &settings).await.unwrap();
        assert!(engine.route(input).is_none());
    }
}
