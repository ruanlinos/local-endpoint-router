use super::database::{self, open_database};
use crate::profiles::manager::{self, ProfileDocument};
use crate::routing::rules::RuleInput;

async fn pool() -> sqlx::SqlitePool {
    let dir = std::env::temp_dir().join(format!(
        "local-endpoint-router-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let pool = open_database(&dir.join("router.db")).await.unwrap();
    database::ensure_defaults(&pool).await.unwrap();
    pool
}

fn rule(destination: &str) -> RuleInput {
    RuleInput {
        name: "payments".into(),
        enabled: true,
        priority: 10,
        source_host: "*.company.com".into(),
        source_port: Some(443),
        methods: vec!["GET".into(), "POST".into()],
        path_prefix: Some("/api".into()),
        rewrite_from: Some("/api".into()),
        rewrite_to: Some("/v2".into()),
        destination: destination.into(),
    }
}

#[tokio::test]
async fn defaults_and_settings_roundtrip() {
    let pool = pool().await;
    let profiles = database::list_profiles(&pool).await.unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "Default");
    let mut settings = database::load_settings(&pool).await.unwrap();
    assert_eq!(settings.port, 8787);
    assert_eq!(settings.bind_host, "127.0.0.1");
    assert_eq!(settings.active_profile_id, profiles[0].id);
    settings.port = 8790;
    settings.bind_host = "127.0.0.1".into();
    let settings = crate::config::settings::validate_settings(&pool, settings).await.unwrap();
    database::save_settings(&pool, &settings).await.unwrap();
    let loaded = database::load_settings(&pool).await.unwrap();
    assert_eq!(loaded.port, 8790);
}

#[tokio::test]
async fn import_export_roundtrip_and_rollback() {
    let pool = pool().await;
    let before = database::list_profiles(&pool).await.unwrap().len();
    let bad = ProfileDocument {
        version: 1,
        name: "Broken".into(),
        rules: vec![rule("http://169.254.169.254/latest")],
    };
    assert!(manager::import_profile(&pool, bad).await.is_err());
    assert_eq!(database::list_profiles(&pool).await.unwrap().len(), before);

    let document = ProfileDocument {
        version: 1,
        name: "Work".into(),
        rules: vec![rule("http://localhost:5002")],
    };
    let created = manager::import_profile(&pool, document).await.unwrap();
    let exported = manager::export_profile(&pool, &created.id).await.unwrap();
    assert_eq!(exported.version, 1);
    assert_eq!(exported.name, "Work");
    assert_eq!(exported.rules.len(), 1);
    assert_eq!(exported.rules[0].source_host, "*.company.com");
    assert_eq!(exported.rules[0].destination, "http://localhost:5002");
    assert_eq!(exported.rules[0].methods, vec!["GET".to_string(), "POST".to_string()]);

    let settings = database::delete_profile(&pool, &created.id).await.unwrap();
    assert_ne!(settings.active_profile_id, created.id);
    assert!(database::list_rules(&pool, &created.id).await.unwrap().is_empty());
    let only = database::list_profiles(&pool).await.unwrap();
    assert_eq!(only.len(), 1);
    assert!(database::delete_profile(&pool, &only[0].id).await.is_err());
}
