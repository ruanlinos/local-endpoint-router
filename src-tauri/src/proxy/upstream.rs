use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use crate::error::AppError;

pub type ProxyBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

pub fn empty_body() -> ProxyBody {
    Empty::<Bytes>::new().map_err(|never| match never {}).boxed()
}

pub fn full_body(bytes: Bytes) -> ProxyBody {
    Full::new(bytes).map_err(|never| match never {}).boxed()
}

pub fn incoming_body(body: Incoming) -> ProxyBody {
    body.boxed()
}

#[derive(Clone)]
pub struct Upstream {
    client: Client<hyper_rustls::HttpsConnector<HttpConnector>, ProxyBody>,
}

impl Upstream {
    /// Direct connector. `HttpConnector` does not read `HTTP_PROXY`, so the
    /// process cannot loop back into this listener through the environment.
    pub fn new(connect_timeout: Duration) -> Result<Self, AppError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        http.set_connect_timeout(Some(connect_timeout));
        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|err| AppError::message(format!("could not load the OS trust store: {err}")))?
            .https_or_http()
            .enable_http1()
            .wrap_connector(http);
        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(Duration::from_secs(30))
            .build(connector);
        Ok(Self { client })
    }

    pub async fn send(
        &self,
        request: hyper::Request<ProxyBody>,
    ) -> Result<hyper::Response<Incoming>, hyper_util::client::legacy::Error> {
        self.client.request(request).await
    }
}
