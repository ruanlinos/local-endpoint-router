use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};

use bytes::Bytes;
use hyper::body::Incoming;
use hyper::header::{HeaderMap, HeaderValue, HOST};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode, Uri};
use url::Url;

use hyper_util::rt::TokioIo;

use crate::error::AppError;
use crate::proxy::resolve::HostResolver;
use crate::proxy::upstream::{empty_body, full_body, ProxyBody, Upstream};
use crate::routing::engine::RuleEngine;
use crate::routing::guard::{is_always_blocked, is_metadata, is_passthrough_blocked};
use crate::routing::matcher::MatchInput;
use crate::tls::CertificateAuthority;
use crate::traffic::{PreviewBody, TrafficCollector};
use http_body_util::BodyExt;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct ProxyShared {
    pub engine: Arc<RuleEngine>,
    pub upstream: Upstream,
    pub request_timeout: Duration,
    pub resolver: HostResolver,
    pub block_restricted: bool,
    pub traffic: Arc<TrafficCollector>,
    pub ca: Option<Arc<CertificateAuthority>>,
}

pub async fn handle(req: Request<Incoming>, peer: IpAddr, shared: ProxyShared) -> Response<ProxyBody> {
    if req.method() == Method::CONNECT {
        return connect(req, peer, shared).await;
    }
    let method = req.method().clone();
    let url = req.uri().to_string();
    let host = req.uri().host().unwrap_or("-").to_string();
    match dispatch(req, peer, &shared).await {
        Ok(response) => response,
        Err(err) => {
            let message = err.to_string();
            let status = if message.contains("not allowed")
                || message.contains("blocked")
                || message.contains("metadata")
            {
                StatusCode::FORBIDDEN
            } else if message.contains("absolute-form")
                || message.contains("userinfo")
                || message.contains("only http")
                || message.contains("missing a host")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_GATEWAY
            };
            shared.traffic.fail(method.as_str(), &url, &host, status.as_u16(), &message, "http");
            text(status, message)
        }
    }
}

async fn dispatch(req: Request<Incoming>, peer: IpAddr, shared: &ProxyShared) -> Result<Response<ProxyBody>, AppError> {
    let target = absolute_target(req.uri())?;
    let (parts, body) = req.into_parts();
    let input = MatchInput {
        host: &target.host,
        port: target.port,
        method: parts.method.as_str(),
        path: &target.path,
        query: target.query.as_deref(),
    };
    if let Some(route) = shared.engine.route(input) {
        let destination = Url::parse(&route.destination_url)
            .map_err(|_| AppError::message("mapped destination is not a URL"))?;
        ensure_safe_destination(&destination, &shared.resolver).await?;
        let forwarded_host = display_host(&target.host, target.port, &target.scheme);
        return forward(
            parts.method,
            destination,
            &forwarded_host,
            &target.scheme,
            peer,
            &parts.headers,
            body,
            shared,
            true,
            Some(route.rule_id),
            Some(route.rule_name),
        )
        .await;
    }
    ensure_passthrough_allowed(&target, &shared.resolver, shared.block_restricted).await?;
    let original = Url::parse(&target.original)
        .map_err(|_| AppError::message("request target is not a URL"))?;
    let forwarded_host = display_host(&target.host, target.port, &target.scheme);
    forward(
        parts.method,
        original,
        &forwarded_host,
        &target.scheme,
        peer,
        &parts.headers,
        body,
        shared,
        false,
        None,
        None,
    )
    .await
}

struct AbsoluteTarget {
    original: String,
    scheme: String,
    host: String,
    port: u16,
    path: String,
    query: Option<String>,
}

fn absolute_target(uri: &Uri) -> Result<AbsoluteTarget, AppError> {
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| AppError::message("the proxy requires an absolute-form URL"))?;
    if scheme != "http" && scheme != "https" {
        return Err(AppError::message("only http and https requests are accepted"));
    }
    let authority = uri
        .authority()
        .ok_or_else(|| AppError::message("the request is missing a host"))?;
    if authority.as_str().contains('@') {
        return Err(AppError::message("request userinfo is not allowed"));
    }
    let host = uri
        .host()
        .ok_or_else(|| AppError::message("the request is missing a host"))?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err(AppError::message("the request is missing a host"));
    }
    if is_metadata_name(&host) {
        return Err(AppError::message("metadata hosts are blocked"));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_metadata(ip) || ip.is_unspecified() {
            return Err(AppError::message("that address is blocked"));
        }
    }
    let port = uri.port_u16().unwrap_or(if scheme == "https" { 443 } else { 80 });
    let path = if uri.path().is_empty() { "/" } else { uri.path() };
    Ok(AbsoluteTarget {
        original: uri.to_string(),
        scheme: scheme.to_string(),
        host,
        port,
        path: path.to_string(),
        query: uri.query().map(str::to_string),
    })
}

async fn ensure_safe_destination(url: &Url, resolver: &HostResolver) -> Result<(), AppError> {
    let host = url
        .host_str()
        .ok_or_else(|| AppError::message("destination host is required"))?
        .to_ascii_lowercase();
    reject_blocked_host(&host, url_port(url), resolver, false).await
}

async fn ensure_passthrough_allowed(
    target: &AbsoluteTarget,
    resolver: &HostResolver,
    block_restricted: bool,
) -> Result<(), AppError> {
    reject_blocked_host(&target.host, target.port, resolver, block_restricted).await
}

async fn reject_blocked_host(
    host: &str,
    port: u16,
    resolver: &HostResolver,
    block_restricted: bool,
) -> Result<(), AppError> {
    if is_metadata_name(host) {
        return Err(AppError::message("metadata hosts are blocked"));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return check_ip(ip, block_restricted);
    }
    let addrs = resolver.lookup(host, port).await?;
    if addrs.is_empty() {
        return Err(AppError::message(format!("no addresses for {host}")));
    }
    for addr in addrs {
        check_ip(addr.ip(), block_restricted)?;
    }
    Ok(())
}

fn check_ip(ip: IpAddr, block_restricted: bool) -> Result<(), AppError> {
    if is_always_blocked(ip) {
        return Err(AppError::message(format!("{ip} is blocked")));
    }
    if block_restricted && is_passthrough_blocked(ip) {
        return Err(AppError::message(format!("{ip} is not allowed for an unmapped request")));
    }
    Ok(())
}

async fn connect(req: Request<Incoming>, peer: IpAddr, shared: ProxyShared) -> Response<ProxyBody> {
    let uri = req.uri().clone();
    let host = uri.host().unwrap_or("").trim_end_matches('.').to_ascii_lowercase();
    let port = uri.port_u16().unwrap_or(443);
    let target = if host.is_empty() { uri.to_string() } else { format!("{host}:{port}") };
    if host.is_empty() {
        shared.traffic.fail("CONNECT", &target, "-", StatusCode::BAD_REQUEST.as_u16(), "CONNECT is missing a host", "connect-blocked");
        return text(StatusCode::BAD_REQUEST, "CONNECT is missing a host");
    }
    if shared.engine.host_mapped(&host, port) {
        if !shared.engine.https_interception() {
            let message = "HTTPS interception is off for this mapped host. The connection was not tunneled to the remote host.";
            shared.traffic.fail("CONNECT", &target, &host, StatusCode::NOT_IMPLEMENTED.as_u16(), message, "connect-blocked");
            return text(StatusCode::NOT_IMPLEMENTED, message);
        }
        let Some(ca) = shared.ca.clone() else {
            let message = "HTTPS interception is on, but the local CA is not loaded. The connection was not tunneled to the remote host.";
            shared.traffic.fail("CONNECT", &target, &host, StatusCode::NOT_IMPLEMENTED.as_u16(), message, "connect-blocked");
            return text(StatusCode::NOT_IMPLEMENTED, message);
        };
        if let Some(message) = ca.unavailable() {
            let message = format!("HTTPS interception is on, but the local CA is unavailable: {message}. The connection was not tunneled to the remote host.");
            shared.traffic.fail("CONNECT", &target, &host, StatusCode::NOT_IMPLEMENTED.as_u16(), &message, "connect-blocked");
            return text(StatusCode::NOT_IMPLEMENTED, message);
        }
        let on_upgrade = hyper::upgrade::on(req);
        let exchange = shared.traffic.open("CONNECT", &target, &host, "connect-intercept");
        shared.traffic.finish(exchange.id, Some(StatusCode::OK.as_u16()), &HeaderMap::new(), None);
        let host_for_task = host.clone();
        tokio::spawn(async move {
            match on_upgrade.await {
                Ok(upgraded) => intercept(upgraded, host_for_task, port, peer, shared, ca).await,
                Err(err) => tracing::debug!(error = %err, "CONNECT upgrade failed"),
            }
        });
        return empty_ok();
    }
    let addrs = match allowed_addrs(&host, port, &shared.resolver, shared.block_restricted).await {
        Ok(addrs) => addrs,
        Err(err) => {
            let message = err.to_string();
            let status = if message.contains("not allowed") || message.contains("blocked") || message.contains("metadata") {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::BAD_GATEWAY
            };
            shared.traffic.fail("CONNECT", &target, &host, status.as_u16(), &message, "connect-blocked");
            return text(status, message);
        }
    };
    let on_upgrade = hyper::upgrade::on(req);
    let exchange = shared.traffic.open("CONNECT", &target, &host, "connect-tunnel");
    shared.traffic.finish(exchange.id, Some(StatusCode::OK.as_u16()), &HeaderMap::new(), None);
    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => {
                if let Err(err) = tunnel(upgraded, addrs).await {
                    tracing::debug!(error = %err, "CONNECT tunnel ended");
                }
            }
            Err(err) => tracing::debug!(error = %err, "CONNECT upgrade failed"),
        }
    });
    empty_ok()
}

fn empty_ok() -> Response<ProxyBody> {
    Response::builder()
        .status(StatusCode::OK)
        .body(empty_body())
        .unwrap_or_else(|_| Response::new(empty_body()))
}

async fn intercept(
    upgraded: hyper::upgrade::Upgraded,
    host: String,
    port: u16,
    peer: IpAddr,
    shared: ProxyShared,
    ca: Arc<CertificateAuthority>,
) {
    let acceptor = tokio_rustls::TlsAcceptor::from(ca.server_config(shared.engine.clone(), port));
    let tls = match acceptor.accept(TokioIo::new(upgraded)).await {
        Ok(tls) => tls,
        Err(err) => {
            tracing::debug!(error = %err, "TLS interception handshake failed");
            return;
        }
    };
    let service = service_fn(move |request| {
        let shared = shared.clone();
        let host = host.clone();
        async move {
            let request = with_absolute_https(request, &host, port);
            let response = match dispatch(request, peer, &shared).await {
                Ok(response) => response,
                Err(err) => text(StatusCode::BAD_GATEWAY, err.to_string()),
            };
            Ok::<_, std::convert::Infallible>(response)
        }
    });
    if let Err(err) = http1::Builder::new().serve_connection(TokioIo::new(tls), service).await {
        tracing::debug!(error = %err, "intercepted connection closed");
    }
}

fn with_absolute_https(req: Request<Incoming>, host: &str, port: u16) -> Request<Incoming> {
    let (mut parts, body) = req.into_parts();
    if parts.uri.scheme().is_none() {
        let path = parts.uri.path_and_query().map(|value| value.as_str()).unwrap_or("/");
        let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
        let authority = if port == 443 { host.to_string() } else { format!("{host}:{port}") };
        if let Ok(uri) = format!("https://{authority}{path}").parse() {
            parts.uri = uri;
        }
    }
    Request::from_parts(parts, body)
}

async fn allowed_addrs(
    host: &str,
    port: u16,
    resolver: &HostResolver,
    block_restricted: bool,
) -> Result<Vec<SocketAddr>, AppError> {
    if is_metadata_name(host) {
        return Err(AppError::message("metadata hosts are blocked"));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        check_ip(ip, block_restricted)?;
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addrs = resolver.lookup(host, port).await?;
    if addrs.is_empty() {
        return Err(AppError::message(format!("no addresses for {host}")));
    }
    for addr in &addrs {
        check_ip(addr.ip(), block_restricted)?;
    }
    Ok(addrs)
}

async fn tunnel(upgraded: hyper::upgrade::Upgraded, addrs: Vec<SocketAddr>) -> Result<(), AppError> {
    let mut remote = connect_any(&addrs).await?;
    let mut client = TokioIo::new(upgraded);
    let _ = tokio::io::copy_bidirectional(&mut client, &mut remote).await;
    Ok(())
}

async fn connect_any(addrs: &[SocketAddr]) -> Result<tokio::net::TcpStream, AppError> {
    let mut last = String::from("no address");
    for addr in addrs {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(stream) => return Ok(stream),
            Err(err) => last = err.to_string(),
        }
    }
    Err(AppError::message(format!("could not open the CONNECT tunnel: {last}")))
}

async fn forward(
    method: Method,
    url: Url,
    forwarded_host: &str,
    forwarded_proto: &str,
    peer: IpAddr,
    headers: &HeaderMap,
    body: Incoming,
    shared: &ProxyShared,
    mapped: bool,
    rule_id: Option<String>,
    rule_name: Option<String>,
) -> Result<Response<ProxyBody>, AppError> {
    let host = host_header(&url)?;
    let upstream_headers = prepare_headers(headers, &host, forwarded_host, forwarded_proto, peer);
    let exchange = shared.traffic.open(method.as_str(), url.as_str(), forwarded_host, "http");
    shared.traffic.set_headers(exchange.id, headers, mapped, rule_id, rule_name);
    let traffic = shared.traffic.clone();
    let exchange_id = exchange.id;
    let request_body = PreviewBody::new(body, exchange.request_preview.clone(), {
        let traffic = traffic.clone();
        move || traffic.emit(exchange_id)
    })
    .boxed();
    let mut builder = Request::builder().method(method).uri(
        url.as_str()
            .parse::<Uri>()
            .map_err(|_| AppError::message("could not build the upstream URI"))?,
    );
    for (name, value) in upstream_headers.iter() {
        builder = builder.header(name, value);
    }
    let request = builder
        .body(request_body)
        .map_err(|err| AppError::message(format!("could not build the upstream request: {err}")))?;
    let response = match tokio::time::timeout(shared.request_timeout, shared.upstream.send(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            let message = format!("upstream request failed: {err}");
            shared.traffic.finish(exchange.id, None, &HeaderMap::new(), Some(message.clone()));
            return Err(AppError::message(message));
        }
        Err(_) => {
            shared.traffic.finish(
                exchange.id,
                Some(StatusCode::GATEWAY_TIMEOUT.as_u16()),
                &HeaderMap::new(),
                Some("upstream timed out".into()),
            );
            return Ok(text(StatusCode::GATEWAY_TIMEOUT, "upstream timed out"));
        }
    };
    let (parts, body) = response.into_parts();
    shared.traffic.finish(exchange.id, Some(parts.status.as_u16()), &parts.headers, None);
    let response_body = PreviewBody::new(body, exchange.response_preview, {
        let traffic = shared.traffic.clone();
        let id = exchange.id;
        move || traffic.emit(id)
    })
    .boxed();
    let headers = strip_hop_by_hop(&parts.headers);
    let mut builder = Response::builder().status(parts.status);
    for (name, value) in headers.iter() {
        builder = builder.header(name, value);
    }
    builder
        .body(response_body)
        .map_err(|err| AppError::message(format!("could not build the proxy response: {err}")))
}

pub(crate) fn prepare_headers(
    headers: &HeaderMap,
    host: &str,
    forwarded_host: &str,
    forwarded_proto: &str,
    peer: IpAddr,
) -> HeaderMap {
    let mut out = strip_hop_by_hop(headers);
    out.remove(HOST);
    if let Ok(value) = HeaderValue::from_str(host) {
        out.insert(HOST, value);
    }
    let forwarded_for = match out.get("x-forwarded-for").and_then(|value| value.to_str().ok()) {
        Some(existing) if !existing.is_empty() => format!("{existing}, {peer}"),
        _ => peer.to_string(),
    };
    out.insert("x-forwarded-for", HeaderValue::from_str(&forwarded_for).unwrap_or(HeaderValue::from_static("0.0.0.0")));
    if let Ok(value) = HeaderValue::from_str(forwarded_host) {
        out.insert("x-forwarded-host", value);
    }
    if let Ok(value) = HeaderValue::from_str(forwarded_proto) {
        out.insert("x-forwarded-proto", value);
    }
    out
}

fn strip_hop_by_hop(headers: &HeaderMap) -> HeaderMap {
    let mut extra = connection_tokens(headers);
    extra.insert("connection".into());
    extra.insert("keep-alive".into());
    extra.insert("proxy-authenticate".into());
    extra.insert("proxy-authorization".into());
    extra.insert("proxy-connection".into());
    extra.insert("te".into());
    extra.insert("trailer".into());
    extra.insert("trailers".into());
    extra.insert("transfer-encoding".into());
    extra.insert("upgrade".into());
    let mut out = HeaderMap::new();
    for (name, value) in headers.iter() {
        if extra.contains(&name.as_str().to_ascii_lowercase()) {
            continue;
        }
        out.append(name, value.clone());
    }
    out
}

fn connection_tokens(headers: &HeaderMap) -> HashSet<String> {
    let mut tokens = HashSet::new();
    for value in headers.get_all("connection") {
        let Ok(text) = value.to_str() else {
            continue;
        };
        for token in text.split(',') {
            let token = token.trim().to_ascii_lowercase();
            if !token.is_empty() {
                tokens.insert(token);
            }
        }
    }
    tokens
}

fn host_header(url: &Url) -> Result<String, AppError> {
    let host = url
        .host_str()
        .ok_or_else(|| AppError::message("upstream host is required"))?;
    let port = url_port(url);
    let default_port = if url.scheme() == "https" { 443 } else { 80 };
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    if port == default_port {
        Ok(host)
    } else {
        Ok(format!("{host}:{port}"))
    }
}

fn url_port(url: &Url) -> u16 {
    url.port().unwrap_or(if url.scheme() == "https" { 443 } else { 80 })
}

fn display_host(host: &str, port: u16, scheme: &str) -> String {
    let default_port = if scheme == "https" { 443 } else { 80 };
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    if port == default_port {
        host
    } else {
        format!("{host}:{port}")
    }
}

fn is_metadata_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("metadata.google.internal") || host.eq_ignore_ascii_case("metadata.google.com")
}

fn text(status: StatusCode, message: impl Into<String>) -> Response<ProxyBody> {
    let bytes = Bytes::from(message.into());
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .header("content-length", bytes.len().to_string())
        .body(full_body(bytes))
        .unwrap_or_else(|_| Response::new(empty_body()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_hop_by_hop_and_rewrites_forwarding_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("close, x-remove-me"));
        headers.insert("x-remove-me", HeaderValue::from_static("secret"));
        headers.insert("x-keep", HeaderValue::from_static("yes"));
        headers.insert("proxy-authorization", HeaderValue::from_static("Basic eA=="));
        headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.8"));
        let prepared = prepare_headers(
            &headers,
            "127.0.0.1:5002",
            "payments.api.company.com",
            "http",
            "192.0.2.10".parse().unwrap(),
        );
        assert!(prepared.get("x-remove-me").is_none());
        assert!(prepared.get("proxy-authorization").is_none());
        assert!(prepared.get("connection").is_none());
        assert_eq!(prepared.get("x-keep").unwrap(), "yes");
        assert_eq!(prepared.get(HOST).unwrap(), "127.0.0.1:5002");
        assert_eq!(prepared.get("x-forwarded-host").unwrap(), "payments.api.company.com");
        assert_eq!(prepared.get("x-forwarded-proto").unwrap(), "http");
        assert_eq!(prepared.get("x-forwarded-for").unwrap(), "10.0.0.8, 192.0.2.10");
    }
}
