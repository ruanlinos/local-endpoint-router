use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::settings::Settings;
use crate::health::HealthMonitor;
use crate::proxy::ProxyHandle;
use crate::routing::engine::RuleEngine;
use crate::storage::database::open_database;
use crate::system::SystemProxyManager;
use crate::tls::CertificateAuthority;
use crate::traffic::{capture_limit, TrafficCollector};

pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub settings: RwLock<Settings>,
    pub engine: Arc<RuleEngine>,
    pub proxy: ProxyHandle,
    pub traffic: Arc<TrafficCollector>,
    pub system: SystemProxyManager,
    pub ca: Arc<CertificateAuthority>,
    pub health: Arc<HealthMonitor>,
}

impl AppState {
    pub async fn open(db_path: &std::path::Path) -> Result<Self, crate::error::AppError> {
        let db = open_database(db_path).await?;
        let settings = crate::storage::database::ensure_defaults(&db).await?;
        let engine = Arc::new(RuleEngine::new());
        crate::routing::engine::publish(&db, &engine, &settings).await?;
        let traffic = TrafficCollector::new(
            settings.redact_sensitive,
            capture_limit(settings.capture_body, settings.body_limit_bytes),
        );
        let data_dir = db_path.parent().unwrap_or(db_path).to_path_buf();
        Ok(Self {
            db,
            settings: RwLock::new(settings),
            engine,
            proxy: ProxyHandle::new(),
            traffic,
            system: SystemProxyManager::new(data_dir.clone()),
            ca: CertificateAuthority::open(&data_dir),
            health: HealthMonitor::new(),
        })
    }
}
