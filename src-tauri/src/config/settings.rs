use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::routing::is_metadata;
use crate::storage::database;

pub const DEFAULT_PROFILE_ID: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub bind_host: String,
    pub port: u16,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub redact_sensitive: bool,
    pub capture_body: bool,
    pub body_limit_bytes: u32,
    pub https_interception: bool,
    pub autostart_proxy: bool,
    pub active_profile_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bind_host: "127.0.0.1".into(),
            port: 8787,
            connect_timeout_ms: 10_000,
            request_timeout_ms: 60_000,
            redact_sensitive: true,
            capture_body: false,
            body_limit_bytes: 65_536,
            https_interception: false,
            autostart_proxy: false,
            active_profile_id: DEFAULT_PROFILE_ID.into(),
        }
    }
}

pub fn normalize_settings(settings: Settings) -> Result<Settings, AppError> {
    let bind_host = settings.bind_host.trim().to_string();
    let ip: std::net::IpAddr = bind_host
        .parse()
        .map_err(|_| AppError::message("bind host must be an IP address"))?;
    if is_metadata(ip) || ip.is_multicast() {
        return Err(AppError::message("bind address is not allowed"));
    }
    if settings.port == 0 {
        return Err(AppError::message("port must be between 1 and 65535"));
    }
    if !(100..=120_000).contains(&settings.connect_timeout_ms) {
        return Err(AppError::message("connect timeout must be between 100 and 120000 ms"));
    }
    if !(500..=600_000).contains(&settings.request_timeout_ms) {
        return Err(AppError::message("request timeout must be between 500 and 600000 ms"));
    }
    if !(1024..=1_048_576).contains(&settings.body_limit_bytes) {
        return Err(AppError::message("body limit must be between 1 KiB and 1 MiB"));
    }
    let active_profile_id = settings.active_profile_id.trim().to_string();
    if active_profile_id.is_empty() {
        return Err(AppError::message("active profile is required"));
    }
    Ok(Settings {
        bind_host,
        port: settings.port,
        connect_timeout_ms: settings.connect_timeout_ms,
        request_timeout_ms: settings.request_timeout_ms,
        redact_sensitive: settings.redact_sensitive,
        capture_body: settings.capture_body,
        body_limit_bytes: settings.body_limit_bytes,
        https_interception: settings.https_interception,
        autostart_proxy: settings.autostart_proxy,
        active_profile_id,
    })
}

pub async fn validate_settings(pool: &sqlx::SqlitePool, settings: Settings) -> Result<Settings, AppError> {
    let settings = normalize_settings(settings)?;
    if !database::profile_exists(pool, &settings.active_profile_id).await? {
        return Err(AppError::message("active profile does not exist"));
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bind_is_loopback_and_metadata_is_rejected() {
        let settings = normalize_settings(Settings::default()).unwrap();
        assert_eq!(settings.bind_host, "127.0.0.1");
        assert_eq!(settings.port, 8787);
        let mut off = Settings::default();
        off.bind_host = "0.0.0.0".into();
        assert!(normalize_settings(off).is_ok());
        let mut metadata = Settings::default();
        metadata.bind_host = "169.254.169.254".into();
        assert!(normalize_settings(metadata).is_err());
        let mut port = Settings::default();
        port.port = 0;
        assert!(normalize_settings(port).is_err());
    }
}
