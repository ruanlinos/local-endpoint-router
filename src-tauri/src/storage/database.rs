use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::config::settings::{Settings, DEFAULT_PROFILE_ID};
use crate::error::AppError;
use crate::routing::rules::{NormalizedRule, Rule};
use crate::util::now_iso;

pub async fn open_database(path: &Path) -> Result<SqlitePool, AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub async fn ensure_defaults(pool: &SqlitePool) -> Result<Settings, AppError> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM profiles").fetch_one(pool).await?;
    if count == 0 {
        let now = now_iso();
        sqlx::query("INSERT INTO profiles (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(DEFAULT_PROFILE_ID)
            .bind("Default")
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await?;
    }
    let (settings_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM app_settings").fetch_one(pool).await?;
    if settings_count == 0 {
        let settings = Settings::default();
        save_settings(pool, &settings).await?;
        return Ok(settings);
    }
    let mut settings = load_settings(pool).await?;
    if !profile_exists(pool, &settings.active_profile_id).await? {
        let profiles = list_profiles(pool).await?;
        settings.active_profile_id = profiles
            .first()
            .map(|profile| profile.id.clone())
            .unwrap_or_else(|| DEFAULT_PROFILE_ID.to_string());
        save_settings(pool, &settings).await?;
    }
    Ok(settings)
}

pub async fn load_settings(pool: &SqlitePool) -> Result<Settings, AppError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT json FROM app_settings WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    match row {
        Some((json,)) => Ok(serde_json::from_str(&json)?),
        None => Ok(Settings::default()),
    }
}

pub async fn save_settings(pool: &SqlitePool, settings: &Settings) -> Result<(), AppError> {
    let json = serde_json::to_string(settings)?;
    sqlx::query(
        "INSERT INTO app_settings (id, json) VALUES (1, ?)
         ON CONFLICT(id) DO UPDATE SET json = excluded.json",
    )
    .bind(json)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(sqlx::FromRow)]
struct ProfileRow {
    id: String,
    name: String,
    created_at: String,
    updated_at: String,
}

impl From<ProfileRow> for Profile {
    fn from(row: ProfileRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

pub async fn list_profiles(pool: &SqlitePool) -> Result<Vec<Profile>, AppError> {
    let rows = sqlx::query_as::<_, ProfileRow>(
        "SELECT id, name, created_at, updated_at FROM profiles ORDER BY name COLLATE NOCASE, id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Profile::from).collect())
}

pub async fn profile_exists(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM profiles WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

pub async fn insert_profile(pool: &SqlitePool, name: &str) -> Result<Profile, AppError> {
    let name = clean_name(name)?;
    let now = now_iso();
    let profile = Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        created_at: now.clone(),
        updated_at: now,
    };
    sqlx::query("INSERT INTO profiles (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&profile.id)
        .bind(&profile.name)
        .bind(&profile.created_at)
        .bind(&profile.updated_at)
        .execute(pool)
        .await?;
    Ok(profile)
}

pub async fn rename_profile(pool: &SqlitePool, id: &str, name: &str) -> Result<Profile, AppError> {
    let name = clean_name(name)?;
    let now = now_iso();
    let result = sqlx::query("UPDATE profiles SET name = ?, updated_at = ? WHERE id = ?")
        .bind(&name)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::message("profile not found"));
    }
    get_profile(pool, id).await?.ok_or_else(|| AppError::message("profile not found"))
}

pub async fn get_profile(pool: &SqlitePool, id: &str) -> Result<Option<Profile>, AppError> {
    let row = sqlx::query_as::<_, ProfileRow>(
        "SELECT id, name, created_at, updated_at FROM profiles WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Profile::from))
}

pub async fn delete_profile(pool: &SqlitePool, id: &str) -> Result<Settings, AppError> {
    let profiles = list_profiles(pool).await?;
    if profiles.len() <= 1 {
        return Err(AppError::message("the last profile cannot be deleted"));
    }
    if !profiles.iter().any(|profile| profile.id == id) {
        return Err(AppError::message("profile not found"));
    }
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM profiles WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let mut settings = load_settings(pool).await?;
    if settings.active_profile_id == id {
        settings.active_profile_id = profiles
            .into_iter()
            .find(|profile| profile.id != id)
            .map(|profile| profile.id)
            .ok_or_else(|| AppError::message("the last profile cannot be deleted"))?;
        let json = serde_json::to_string(&settings)?;
        sqlx::query("UPDATE app_settings SET json = ? WHERE id = 1")
            .bind(json)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(settings)
}

#[derive(sqlx::FromRow)]
struct RuleRow {
    id: String,
    profile_id: String,
    name: String,
    enabled: i64,
    priority: i64,
    source_host: String,
    source_port: Option<i64>,
    methods: String,
    path_prefix: Option<String>,
    rewrite_from: Option<String>,
    rewrite_to: Option<String>,
    destination: String,
    created_at: String,
    updated_at: String,
}

impl RuleRow {
    fn into_rule(self) -> Result<Rule, AppError> {
        let methods = serde_json::from_str(&self.methods).unwrap_or_default();
        let source_port = match self.source_port {
            Some(port) => Some(
                u16::try_from(port).map_err(|_| AppError::message("stored source port is invalid"))?,
            ),
            None => None,
        };
        Ok(Rule {
            id: self.id,
            profile_id: self.profile_id,
            name: self.name,
            enabled: self.enabled != 0,
            priority: i32::try_from(self.priority).unwrap_or(100),
            source_host: self.source_host,
            source_port,
            methods,
            path_prefix: self.path_prefix,
            rewrite_from: self.rewrite_from,
            rewrite_to: self.rewrite_to,
            destination: self.destination,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

pub async fn list_rules(pool: &SqlitePool, profile_id: &str) -> Result<Vec<Rule>, AppError> {
    let rows = sqlx::query_as::<_, RuleRow>(
        "SELECT id, profile_id, name, enabled, priority, source_host, source_port, methods,
                path_prefix, rewrite_from, rewrite_to, destination, created_at, updated_at
         FROM rules WHERE profile_id = ?
         ORDER BY priority ASC, id ASC",
    )
    .bind(profile_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(RuleRow::into_rule).collect()
}

pub async fn insert_rule(
    pool: &SqlitePool,
    profile_id: &str,
    rule: &NormalizedRule,
) -> Result<Rule, AppError> {
    if !profile_exists(pool, profile_id).await? {
        return Err(AppError::message("profile not found"));
    }
    let now = now_iso();
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO rules (
            id, profile_id, name, enabled, priority, source_host, source_port, methods,
            path_prefix, rewrite_from, rewrite_to, destination, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(profile_id)
    .bind(&rule.name)
    .bind(if rule.enabled { 1 } else { 0 })
    .bind(rule.priority)
    .bind(&rule.source_host)
    .bind(rule.source_port.map(i64::from))
    .bind(serde_json::to_string(&rule.methods)?)
    .bind(&rule.path_prefix)
    .bind(&rule.rewrite_from)
    .bind(&rule.rewrite_to)
    .bind(&rule.destination)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    list_rules(pool, profile_id)
        .await?
        .into_iter()
        .find(|stored| stored.id == id)
        .ok_or_else(|| AppError::message("rule was not stored"))
}

pub async fn insert_rule_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    profile_id: &str,
    rule: &NormalizedRule,
) -> Result<(), AppError> {
    let now = now_iso();
    sqlx::query(
        "INSERT INTO rules (
            id, profile_id, name, enabled, priority, source_host, source_port, methods,
            path_prefix, rewrite_from, rewrite_to, destination, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(profile_id)
    .bind(&rule.name)
    .bind(if rule.enabled { 1 } else { 0 })
    .bind(rule.priority)
    .bind(&rule.source_host)
    .bind(rule.source_port.map(i64::from))
    .bind(serde_json::to_string(&rule.methods)?)
    .bind(&rule.path_prefix)
    .bind(&rule.rewrite_from)
    .bind(&rule.rewrite_to)
    .bind(&rule.destination)
    .bind(&now)
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn set_rule_enabled(pool: &SqlitePool, id: &str, enabled: bool) -> Result<Rule, AppError> {
    let now = now_iso();
    let result = sqlx::query("UPDATE rules SET enabled = ?, updated_at = ? WHERE id = ?")
        .bind(if enabled { 1 } else { 0 })
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::message("rule not found"));
    }
    get_rule(pool, id).await
}

pub async fn update_rule(pool: &SqlitePool, id: &str, rule: &NormalizedRule) -> Result<Rule, AppError> {
    let now = now_iso();
    let result = sqlx::query(
        "UPDATE rules SET name = ?, enabled = ?, priority = ?, source_host = ?, source_port = ?,
                methods = ?, path_prefix = ?, rewrite_from = ?, rewrite_to = ?, destination = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(&rule.name)
    .bind(if rule.enabled { 1 } else { 0 })
    .bind(rule.priority)
    .bind(&rule.source_host)
    .bind(rule.source_port.map(i64::from))
    .bind(serde_json::to_string(&rule.methods)?)
    .bind(&rule.path_prefix)
    .bind(&rule.rewrite_from)
    .bind(&rule.rewrite_to)
    .bind(&rule.destination)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::message("rule not found"));
    }
    get_rule(pool, id).await
}

pub async fn delete_rule(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM rules WHERE id = ?").bind(id).execute(pool).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::message("rule not found"));
    }
    Ok(())
}

pub async fn get_rule(pool: &SqlitePool, id: &str) -> Result<Rule, AppError> {
    let row = sqlx::query_as::<_, RuleRow>(
        "SELECT id, profile_id, name, enabled, priority, source_host, source_port, methods,
                path_prefix, rewrite_from, rewrite_to, destination, created_at, updated_at
         FROM rules WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.ok_or_else(|| AppError::message("rule not found"))?.into_rule()
}

fn clean_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(AppError::message("profile name must be 1-120 characters"));
    }
    Ok(name.to_string())
}
