use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::settings::Settings;
use crate::error::AppError;
use crate::proxy::handler::{handle, ProxyShared};
use crate::proxy::resolve::HostResolver;
use crate::proxy::upstream::Upstream;
use crate::routing::engine::RuleEngine;
use crate::routing::guard::is_loopback_bind;
use crate::tls::CertificateAuthority;
use crate::traffic::{capture_limit, TrafficCollector};

pub struct ProxyHandle {
    inner: Mutex<Option<Running>>,
}

struct Running {
    cancel: CancellationToken,
    join: JoinHandle<()>,
    addr: SocketAddr,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub address: Option<String>,
    pub bind_host: String,
    pub port: u16,
    pub loopback_bind: bool,
    pub https_interception: bool,
}

impl ProxyHandle {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    pub async fn start(&self, bind: SocketAddr, shared: ProxyShared) -> Result<SocketAddr, AppError> {
        let mut guard = self.inner.lock().await;
        if let Some(running) = guard.as_ref() {
            return Ok(running.addr);
        }
        let listener = TcpListener::bind(bind)
            .await
            .map_err(|err| AppError::message(format!("could not bind {bind}: {err}")))?;
        let addr = listener.local_addr()?;
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let join = tokio::spawn(async move {
            accept_loop(listener, task_cancel, shared).await;
        });
        *guard = Some(Running { cancel, join, addr });
        tracing::info!(%addr, "proxy listening");
        Ok(addr)
    }

    pub async fn stop(&self) {
        let running = self.inner.lock().await.take();
        if let Some(running) = running {
            running.cancel.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(2), running.join).await;
            tracing::info!("proxy stopped");
        }
    }

    pub async fn address(&self) -> Option<SocketAddr> {
        self.inner.lock().await.as_ref().map(|running| running.addr)
    }
}

impl Default for ProxyHandle {
    fn default() -> Self {
        Self::new()
    }
}

async fn accept_loop(listener: TcpListener, cancel: CancellationToken, shared: ProxyShared) {
    loop {
        let accepted = tokio::select! {
            _ = cancel.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, peer) = match accepted {
            Ok(pair) => pair,
            Err(err) => {
                if cancel.is_cancelled() {
                    break;
                }
                tracing::warn!(error = %err, "accept failed");
                continue;
            }
        };
        let shared = shared.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let peer_ip = peer.ip();
            let service = service_fn(move |request| {
                let shared = shared.clone();
                async move { Ok::<_, std::convert::Infallible>(handle(request, peer_ip, shared).await) }
            });
            if let Err(err) = http1::Builder::new().serve_connection(io, service).with_upgrades().await {
                tracing::debug!(error = %err, "connection closed");
            }
        });
    }
}

pub async fn start_with_settings(
    handle: &ProxyHandle,
    settings: &Settings,
    engine: Arc<RuleEngine>,
    traffic: Arc<TrafficCollector>,
    ca: Arc<CertificateAuthority>,
) -> Result<ProxyStatus, AppError> {
    let bind: SocketAddr = format!("{}:{}", settings.bind_host, settings.port)
        .parse()
        .map_err(|_| AppError::message("bind address is invalid"))?;
    traffic.configure(
        settings.redact_sensitive,
        capture_limit(settings.capture_body, settings.body_limit_bytes),
    );
    let upstream = Upstream::new(Duration::from_millis(settings.connect_timeout_ms))?;
    let shared = ProxyShared {
        engine,
        upstream,
        request_timeout: Duration::from_millis(settings.request_timeout_ms),
        resolver: HostResolver::system(),
        block_restricted: true,
        traffic,
        ca: Some(ca),
    };
    let addr = handle.start(bind, shared).await?;
    Ok(status_for(Some(addr), settings))
}

pub fn status_for(addr: Option<SocketAddr>, settings: &Settings) -> ProxyStatus {
    ProxyStatus {
        running: addr.is_some(),
        address: addr.map(|addr| addr.to_string()),
        bind_host: settings.bind_host.clone(),
        port: settings.port,
        loopback_bind: is_loopback_bind(&settings.bind_host),
        https_interception: settings.https_interception,
    }
}
