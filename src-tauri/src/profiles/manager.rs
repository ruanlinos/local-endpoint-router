use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::error::AppError;
use crate::routing::rules::{validate_rule_input, RuleInput};
use crate::storage::database::{self, Profile};
use crate::util::now_iso;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDocument {
    pub version: u32,
    pub name: String,
    pub rules: Vec<RuleInput>,
}

pub async fn export_profile(pool: &SqlitePool, id: &str) -> Result<ProfileDocument, AppError> {
    let profile = database::get_profile(pool, id)
        .await?
        .ok_or_else(|| AppError::message("profile not found"))?;
    let rules = database::list_rules(pool, id).await?;
    let rules = rules
        .into_iter()
        .map(|rule| RuleInput {
            name: rule.name,
            enabled: rule.enabled,
            priority: rule.priority,
            source_host: rule.source_host,
            source_port: rule.source_port,
            methods: rule.methods,
            path_prefix: rule.path_prefix,
            rewrite_from: rule.rewrite_from,
            rewrite_to: rule.rewrite_to,
            destination: rule.destination,
        })
        .collect();
    Ok(ProfileDocument {
        version: 1,
        name: profile.name,
        rules,
    })
}

pub async fn import_profile(pool: &SqlitePool, document: ProfileDocument) -> Result<Profile, AppError> {
    if document.version != 1 {
        return Err(AppError::message("unsupported profile document version"));
    }
    let name = document.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(AppError::message("profile name must be 1-120 characters"));
    }
    let mut normalized = Vec::with_capacity(document.rules.len());
    for rule in &document.rules {
        normalized.push(validate_rule_input(rule)?);
    }
    let now = now_iso();
    let profile = Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO profiles (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&profile.id)
        .bind(&profile.name)
        .bind(&profile.created_at)
        .bind(&profile.updated_at)
        .execute(&mut *tx)
        .await?;
    for rule in &normalized {
        database::insert_rule_tx(&mut tx, &profile.id, rule).await?;
    }
    tx.commit().await?;
    Ok(profile)
}
