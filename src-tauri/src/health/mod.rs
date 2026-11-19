use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hyper::{Request, StatusCode};
use serde::Serialize;

use crate::proxy::upstream::{empty_body, Upstream};
use crate::routing::engine::RuleEngine;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub destination: String,
    pub up: bool,
    pub status: Option<u16>,
    pub latency_ms: u64,
    pub error: Option<String>,
    pub checked_at: String,
}

pub struct HealthMonitor {
    reports: Mutex<Vec<HealthReport>>,
    app: Mutex<Option<tauri::AppHandle>>,
}

impl HealthMonitor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            reports: Mutex::new(Vec::new()),
            app: Mutex::new(None),
        })
    }

    pub fn attach(&self, app: tauri::AppHandle) {
        *self.app.lock().expect("health app") = Some(app);
    }

    pub fn list(&self) -> Vec<HealthReport> {
        self.reports.lock().expect("health").clone()
    }

    pub async fn run(self: Arc<Self>, engine: Arc<RuleEngine>) {
        let upstream = match Upstream::new(Duration::from_secs(2)) {
            Ok(upstream) => upstream,
            Err(err) => {
                tracing::error!(error = %err, "health checks disabled");
                return;
            }
        };
        loop {
            self.check(&engine, &upstream).await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    pub async fn check(&self, engine: &RuleEngine, upstream: &Upstream) {
        let mut reports = Vec::new();
        for destination in engine.destinations() {
            reports.push(probe(upstream, &destination).await);
        }
        *self.reports.lock().expect("health") = reports.clone();
        let Ok(app) = self.app.lock() else { return };
        if let Some(app) = app.as_ref() {
            use tauri::Emitter;
            let _ = app.emit("health://status", &reports);
        }
    }
}

async fn probe(upstream: &Upstream, destination: &str) -> HealthReport {
    let url = format!("{destination}/");
    let started = Instant::now();
    let request = Request::builder().method("GET").uri(&url).body(empty_body());
    let request = match request {
        Ok(request) => request,
        Err(err) => return failed(destination, started, err.to_string()),
    };
    match tokio::time::timeout(Duration::from_secs(2), upstream.send(request)).await {
        Ok(Ok(response)) => {
            let status = response.status();
            HealthReport {
                destination: destination.to_string(),
                up: status < StatusCode::INTERNAL_SERVER_ERROR,
                status: Some(status.as_u16()),
                latency_ms: started.elapsed().as_millis() as u64,
                error: None,
                checked_at: crate::util::now_iso(),
            }
        }
        Ok(Err(err)) => failed(destination, started, err.to_string()),
        Err(_) => failed(destination, started, "health check timed out".into()),
    }
}

fn failed(destination: &str, started: Instant, error: String) -> HealthReport {
    HealthReport {
        destination: destination.to_string(),
        up: false,
        status: None,
        latency_ms: started.elapsed().as_millis() as u64,
        error: Some(error),
        checked_at: crate::util::now_iso(),
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::time::Duration;

    use bytes::Bytes;
    use http_body_util::Full;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use tokio::net::TcpListener;

    use super::*;
    use crate::routing::engine::{RuleEngine, RuleSnapshot};
    use crate::routing::matcher::compile_rule;
    use crate::routing::rules::Rule;

    #[tokio::test]
    async fn checks_unique_destinations_without_using_env_proxy() {
        unsafe {
            std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");
            std::env::set_var("http_proxy", "http://127.0.0.1:1");
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let service = service_fn(|_request| async {
                        Ok::<_, Infallible>(
                            Response::builder()
                                .status(StatusCode::NO_CONTENT)
                                .body(Full::new(Bytes::new()))
                                .unwrap(),
                        )
                    });
                    let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
                });
            }
        });
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
        let engine = Arc::new(RuleEngine::new());
        engine.store(RuleSnapshot {
            profile_id: "default".into(),
            https_interception: false,
            rules: vec![
                compiled("one", &format!("http://127.0.0.1:{}/", addr.port())),
                compiled("two", &format!("http://127.0.0.1:{}/", addr.port())),
                compiled("down", &format!("http://127.0.0.1:{}/", closed.port())),
            ],
        });
        let monitor = HealthMonitor::new();
        let upstream = Upstream::new(Duration::from_secs(2)).unwrap();
        monitor.check(&engine, &upstream).await;
        let reports = monitor.list();
        assert_eq!(reports.len(), 2);
        let up_origin = format!("http://127.0.0.1:{}", addr.port());
        let down_origin = format!("http://127.0.0.1:{}", closed.port());
        let up = reports.iter().find(|report| report.destination == up_origin).unwrap();
        assert!(up.up);
        assert_eq!(up.status, Some(204));
        let down = reports.iter().find(|report| report.destination == down_origin).unwrap();
        assert!(!down.up);
        assert!(down.error.is_some());
        unsafe {
            std::env::remove_var("HTTP_PROXY");
            std::env::remove_var("http_proxy");
        }
    }

    fn compiled(name: &str, destination: &str) -> crate::routing::matcher::CompiledRule {
        compile_rule(&Rule {
            id: name.into(),
            profile_id: "default".into(),
            name: name.into(),
            enabled: true,
            priority: 10,
            source_host: format!("{name}.example"),
            source_port: None,
            methods: Vec::new(),
            path_prefix: None,
            rewrite_from: None,
            rewrite_to: None,
            destination: destination.into(),
            created_at: String::new(),
            updated_at: String::new(),
        })
        .unwrap()
    }
}
