use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};

use crate::proxy::handler::ProxyShared;
use crate::proxy::resolve::HostResolver;
use crate::proxy::server::ProxyHandle;
use crate::proxy::upstream::Upstream;
use crate::routing::engine::{RuleEngine, RuleSnapshot};
use crate::routing::matcher::CompiledRule;
use crate::routing::rules::{parse_destination, parse_host_pattern};
use crate::traffic::TrafficCollector;

#[derive(Default)]
struct Seen {
    hits: AtomicUsize,
    method: Mutex<String>,
    path: Mutex<String>,
    host: Mutex<String>,
    forwarded_host: Mutex<String>,
    forwarded_for: Mutex<String>,
    forwarded_proto: Mutex<String>,
    keep: Mutex<String>,
    removed: Mutex<String>,
    authorization: Mutex<String>,
    body: Mutex<Vec<u8>>,
}

fn rule(host: &str, destination: &str) -> CompiledRule {
    CompiledRule {
        id: host.into(),
        name: host.into(),
        priority: 10,
        host: parse_host_pattern(host).unwrap(),
        source_port: None,
        methods: vec![],
        path_prefix: None,
        rewrite_from: None,
        rewrite_to: None,
        destination: parse_destination(destination).unwrap(),
    }
}

async fn spawn_server<F, Fut>(handler: F) -> std::net::SocketAddr
where
    F: Fn(Request<Incoming>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Response<Full<Bytes>>> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handler = Arc::new(handler);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let service = service_fn(move |request| {
                    let handler = handler.clone();
                    async move { Ok::<_, Infallible>(handler(request).await) }
                });
                let _ = http1::Builder::new().serve_connection(io, service).await;
            });
        }
    });
    addr
}

async fn start_proxy(
    engine: Arc<RuleEngine>,
    timeout: Duration,
) -> (ProxyHandle, std::net::SocketAddr, Arc<TrafficCollector>) {
    start_proxy_with(engine, timeout, HostResolver::system(), true, None).await
}

async fn start_proxy_with(
    engine: Arc<RuleEngine>,
    timeout: Duration,
    resolver: HostResolver,
    block_restricted: bool,
    ca: Option<Arc<crate::tls::CertificateAuthority>>,
) -> (ProxyHandle, std::net::SocketAddr, Arc<TrafficCollector>) {
    let handle = ProxyHandle::new();
    let traffic = TrafficCollector::new(true, 64 * 1024);
    let shared = ProxyShared {
        engine,
        upstream: Upstream::new(Duration::from_secs(2)).unwrap(),
        request_timeout: timeout,
        resolver,
        block_restricted,
        traffic: traffic.clone(),
        ca,
    };
    let addr = handle.start("127.0.0.1:0".parse().unwrap(), shared).await.unwrap();
    (handle, addr, traffic)
}

async fn through_proxy(proxy: std::net::SocketAddr, request: Request<Full<Bytes>>) -> (StatusCode, hyper::HeaderMap, Bytes) {
    let stream = TcpStream::connect(proxy).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let response = sender.send_request(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, body)
}

fn remember(seen: &Seen, method: &str, path: &str, headers: &hyper::HeaderMap, body: &[u8]) {
    seen.hits.fetch_add(1, Ordering::SeqCst);
    *seen.method.lock().unwrap() = method.to_string();
    *seen.path.lock().unwrap() = path.to_string();
    *seen.host.lock().unwrap() = header_value(headers, "host");
    *seen.forwarded_host.lock().unwrap() = header_value(headers, "x-forwarded-host");
    *seen.forwarded_for.lock().unwrap() = header_value(headers, "x-forwarded-for");
    *seen.forwarded_proto.lock().unwrap() = header_value(headers, "x-forwarded-proto");
    *seen.keep.lock().unwrap() = header_value(headers, "x-keep");
    *seen.removed.lock().unwrap() = header_value(headers, "x-remove-me");
    *seen.authorization.lock().unwrap() = header_value(headers, "authorization");
    *seen.body.lock().unwrap() = body.to_vec();
}

fn header_value(headers: &hyper::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn mapped_exchange_does_not_follow_redirects_or_use_env_proxy() {
    unsafe {
        std::env::set_var("HTTP_PROXY", "http://127.0.0.1:1");
        std::env::set_var("http_proxy", "http://127.0.0.1:1");
        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:1");
        std::env::set_var("ALL_PROXY", "http://127.0.0.1:1");
    }

    let primary_seen = Arc::new(Seen::default());
    let secondary_hits = Arc::new(AtomicUsize::new(0));
    let primary_for_server = primary_seen.clone();
    let primary = spawn_server(move |request| {
        let seen = primary_for_server.clone();
        async move {
            let (parts, body) = request.into_parts();
            let bytes = body.collect().await.unwrap().to_bytes();
            let path = parts
                .uri
                .path_and_query()
                .map(|value| value.as_str().to_string())
                .unwrap_or_else(|| parts.uri.path().to_string());
            remember(&seen, parts.method.as_str(), &path, &parts.headers, &bytes);
            if parts.uri.path() == "/redirect" {
                return Response::builder()
                    .status(StatusCode::FOUND)
                    .header("location", "http://billing.company.com/landed")
                    .body(Full::new(Bytes::from("go")))
                    .unwrap();
            }
            Response::builder()
                .status(StatusCode::CREATED)
                .header("x-upstream", "primary")
                .body(Full::new(bytes))
                .unwrap()
        }
    })
    .await;
    let secondary_for_server = secondary_hits.clone();
    let secondary = spawn_server(move |_request| {
        let hits = secondary_for_server.clone();
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from_static(b"from-b")))
                .unwrap()
        }
    })
    .await;

    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: false,
        rules: vec![
            rule("payments.api.company.com", &format!("http://127.0.0.1:{}", primary.port())),
            rule("billing.company.com", &format!("http://127.0.0.1:{}", secondary.port())),
        ],
    });
    let (proxy, proxy_addr, traffic) = start_proxy(engine.clone(), Duration::from_secs(3)).await;

    let request = Request::builder()
        .method("POST")
        .uri("http://payments.api.company.com/api/payments/123?x=1")
        .header("host", "payments.api.company.com")
        .header("content-type", "text/plain")
        .header("connection", "close, x-remove-me")
        .header("x-remove-me", "secret")
        .header("x-keep", "yes")
        .header("authorization", "Bearer secret")
        .body(Full::new(Bytes::from_static(b"hello-body")))
        .unwrap();
    let (status, _headers, body) = through_proxy(proxy_addr, request).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.as_ref(), b"hello-body");
    assert_eq!(primary_seen.method.lock().unwrap().as_str(), "POST");
    assert_eq!(primary_seen.path.lock().unwrap().as_str(), "/api/payments/123?x=1");
    assert_eq!(
        primary_seen.host.lock().unwrap().as_str(),
        format!("127.0.0.1:{}", primary.port())
    );
    assert_eq!(
        primary_seen.forwarded_host.lock().unwrap().as_str(),
        "payments.api.company.com"
    );
    assert_eq!(primary_seen.forwarded_proto.lock().unwrap().as_str(), "http");
    assert!(!primary_seen.forwarded_for.lock().unwrap().is_empty());
    assert_eq!(primary_seen.keep.lock().unwrap().as_str(), "yes");
    assert!(primary_seen.removed.lock().unwrap().is_empty());
    assert_eq!(primary_seen.body.lock().unwrap().as_slice(), b"hello-body");
    assert_eq!(primary_seen.authorization.lock().unwrap().as_str(), "Bearer secret");
    let events = traffic.list();
    let posted = events.iter().find(|event| event.method == "POST").unwrap();
    assert_eq!(posted.request_body_preview, "hello-body");
    assert_eq!(
        posted.request_headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("authorization")).unwrap().1,
        "[redacted]"
    );
    assert!(posted.mapped);

    let redirect = Request::builder()
        .method("GET")
        .uri("http://payments.api.company.com/redirect")
        .header("host", "payments.api.company.com")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, headers, _) = through_proxy(proxy_addr, redirect).await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers.get("location").unwrap(), "http://billing.company.com/landed");
    assert_eq!(secondary_hits.load(Ordering::SeqCst), 0);

    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: false,
        rules: vec![rule("payments.api.company.com", &format!("http://127.0.0.1:{}", secondary.port()))],
    });
    let again = Request::builder()
        .method("GET")
        .uri("http://payments.api.company.com/landed")
        .header("host", "payments.api.company.com")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _, body) = through_proxy(proxy_addr, again).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_ref(), b"from-b");
    assert_eq!(secondary_hits.load(Ordering::SeqCst), 1);
    assert_eq!(primary_seen.hits.load(Ordering::SeqCst), 2);

    let blocked = Request::builder()
        .method("GET")
        .uri(format!("http://127.0.0.1:{}/secret", primary.port()))
        .header("host", format!("127.0.0.1:{}", primary.port()))
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _, body) = through_proxy(proxy_addr, blocked).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let blocked_body = String::from_utf8_lossy(&body);
    assert!(blocked_body.contains("not allowed") || blocked_body.contains("blocked"), "{blocked_body}");
    assert_eq!(primary_seen.hits.load(Ordering::SeqCst), 2);

    proxy.stop().await;
    unsafe {
        std::env::remove_var("HTTP_PROXY");
        std::env::remove_var("http_proxy");
        std::env::remove_var("HTTPS_PROXY");
        std::env::remove_var("ALL_PROXY");
    }
}

#[tokio::test]
async fn browser_follow_up_to_a_mapped_location_uses_that_rules_destination() {
    let primary_hits = Arc::new(AtomicUsize::new(0));
    let secondary_hits = Arc::new(AtomicUsize::new(0));
    let primary_for_server = primary_hits.clone();
    let primary = spawn_server(move |_request| {
        let hits = primary_for_server.clone();
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            Response::builder()
                .status(StatusCode::FOUND)
                .header("location", "http://billing.company.com/landed")
                .body(Full::new(Bytes::from_static(b"go")))
                .unwrap()
        }
    })
    .await;
    let secondary_for_server = secondary_hits.clone();
    let secondary = spawn_server(move |_request| {
        let hits = secondary_for_server.clone();
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from_static(b"from-billing")))
                .unwrap()
        }
    })
    .await;
    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: false,
        rules: vec![
            rule("payments.api.company.com", &format!("http://127.0.0.1:{}/", primary.port())),
            rule("billing.company.com", &format!("http://127.0.0.1:{}/", secondary.port())),
        ],
    });
    let (proxy, proxy_addr, _) = start_proxy(engine, Duration::from_secs(3)).await;
    let first = Request::builder()
        .method("GET")
        .uri("http://payments.api.company.com/redirect")
        .header("host", "payments.api.company.com")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, headers, _) = through_proxy(proxy_addr, first).await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers.get("location").unwrap(), "http://billing.company.com/landed");
    assert_eq!(primary_hits.load(Ordering::SeqCst), 1);
    assert_eq!(secondary_hits.load(Ordering::SeqCst), 0);

    let follow = Request::builder()
        .method("GET")
        .uri("http://billing.company.com/landed")
        .header("host", "billing.company.com")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _, body) = through_proxy(proxy_addr, follow).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_ref(), b"from-billing");
    assert_eq!(secondary_hits.load(Ordering::SeqCst), 1);
    assert_eq!(primary_hits.load(Ordering::SeqCst), 1);
    proxy.stop().await;
}

#[tokio::test]
async fn upstream_timeout_becomes_gateway_timeout() {
    let slow = spawn_server(|_request| async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        Response::new(Full::new(Bytes::from_static(b"late")))
    })
    .await;
    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: false,
        rules: vec![rule("slow.example", &format!("http://127.0.0.1:{}", slow.port()))],
    });
    let (proxy, proxy_addr, _) = start_proxy(engine, Duration::from_millis(200)).await;
    let request = Request::builder()
        .method("GET")
        .uri("http://slow.example/")
        .header("host", "slow.example")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let (status, _, _) = through_proxy(proxy_addr, request).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    proxy.stop().await;
}

async fn echo_server() -> (std::net::SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let (mut read, mut write) = socket.split();
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
        }
    });
    (addr, hits)
}

async fn raw_exchange(proxy: std::net::SocketAddr, target: &str, payload: Option<&[u8]>) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = TcpStream::connect(proxy).await.unwrap();
    let request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut received = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf).await.unwrap();
        assert!(n > 0, "proxy closed before a response");
        received.extend_from_slice(&buf[..n]);
        if received.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let header_end = received.windows(4).position(|window| window == b"\r\n\r\n").unwrap() + 4;
    let head = String::from_utf8_lossy(&received[..header_end]).into_owned();
    let status = head
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse::<u16>()
        .unwrap_or(0);
    if let Some(payload) = payload {
        stream.write_all(payload).await.unwrap();
        let mut echoed = vec![0u8; payload.len()];
        stream.read_exact(&mut echoed).await.unwrap();
        assert_eq!(echoed, payload);
    }
    (status, head)
}

#[tokio::test]
async fn unmatched_connect_is_a_byte_tunnel_and_mapped_connect_is_refused() {
    let (echo, hits) = echo_server().await;
    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: false,
        rules: vec![rule("mapped.test", "http://127.0.0.1:9/")],
    });
    let (proxy, proxy_addr, _) = start_proxy_with(
        engine,
        Duration::from_secs(2),
        HostResolver::fixed(vec![echo]),
        false,
        None,
    )
    .await;

    let (status, _) = raw_exchange(proxy_addr, "unmatched.test:443", Some(b"ping")).await;
    assert_eq!(status, 200);
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let (status, _) = raw_exchange(proxy_addr, "mapped.test:443", None).await;
    assert_eq!(status, 501);
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    proxy.stop().await;
}

#[tokio::test]
async fn connect_blocks_loopback_and_metadata() {
    let (echo, hits) = echo_server().await;
    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: false,
        rules: Vec::new(),
    });
    let (proxy, proxy_addr, _) = start_proxy(engine.clone(), Duration::from_secs(2)).await;
    let (status, _) = raw_exchange(proxy_addr, &format!("127.0.0.1:{}", echo.port()), None).await;
    assert_eq!(status, 403);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    proxy.stop().await;

    let (proxy, proxy_addr, _) = start_proxy_with(engine, Duration::from_secs(2), HostResolver::system(), false, None).await;
    let (status, _) = raw_exchange(proxy_addr, "169.254.169.254:80", None).await;
    assert_eq!(status, 403);
    proxy.stop().await;
}

fn identity(name: &str) -> (rustls::pki_types::CertificateDer<'static>, rustls::pki_types::PrivateKeyDer<'static>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = rcgen::CertificateParams::new(vec![name.to_string()]).unwrap().self_signed(&key).unwrap();
    let der = rustls::pki_types::CertificateDer::from(cert.der().to_vec());
    let private = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()));
    (der, private)
}

async fn read_connect_preface(stream: &mut TcpStream) -> u16 {
    use tokio::io::AsyncReadExt;
    let mut received = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf).await.unwrap();
        assert!(n > 0);
        received.extend_from_slice(&buf[..n]);
        if received.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&received);
    head.split_whitespace().nth(1).unwrap().parse().unwrap()
}

async fn tls_over_connect(
    proxy: std::net::SocketAddr,
    target: &str,
    sni: &'static str,
    trust: rustls::pki_types::CertificateDer<'static>,
    request: &[u8],
) -> (String, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = TcpStream::connect(proxy).await.unwrap();
    let preface = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n");
    stream.write_all(preface.as_bytes()).await.unwrap();
    let status = read_connect_preface(&mut stream).await;
    assert_eq!(status, 200, "CONNECT {target}");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(trust).unwrap();
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let name = rustls::pki_types::ServerName::try_from(sni).unwrap();
    let mut tls = connector.connect(name, stream).await.expect("tls handshake");
    let peer = tls.get_ref().1.peer_certificates().unwrap()[0].as_ref().to_vec();
    tls.write_all(request).await.unwrap();
    let mut buf = vec![0u8; 8192];
    let n = tokio::time::timeout(Duration::from_secs(3), tls.read(&mut buf)).await.unwrap().unwrap();
    (String::from_utf8_lossy(&buf[..n]).into_owned(), peer)
}

async fn tls_origin(body: &'static [u8]) -> (std::net::SocketAddr, rustls::pki_types::CertificateDer<'static>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (cert, key) = identity("unmatched.example");
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key)
        .unwrap();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { break };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else { return };
                let mut buf = [0u8; 2048];
                let _ = tls.read(&mut buf).await;
                let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = tls.write_all(header.as_bytes()).await;
                let _ = tls.write_all(body).await;
            });
        }
    });
    (addr, cert)
}

#[tokio::test]
async fn mapped_connect_is_terminated_and_unmatched_connect_keeps_the_upstream_certificate() {
    let seen = Arc::new(Seen::default());
    let seen_for_server = seen.clone();
    let local = spawn_server(move |request| {
        let seen = seen_for_server.clone();
        async move {
            let (parts, _) = request.into_parts();
            remember(&seen, parts.method.as_str(), parts.uri.path(), &parts.headers, b"");
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from_static(b"local-body")))
                .unwrap()
        }
    })
    .await;
    let dir = std::env::temp_dir().join(format!("ler-ca-{}", uuid::Uuid::new_v4()));
    let ca = crate::tls::CertificateAuthority::open(&dir);
    assert!(ca.unavailable().is_none(), "{}", ca.unavailable().unwrap_or_default());
    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: true,
        rules: vec![rule("payments.example", &format!("http://127.0.0.1:{}/", local.port()))],
    });
    let (proxy, proxy_addr, _) = start_proxy_with(engine, Duration::from_secs(3), HostResolver::system(), true, Some(ca.clone())).await;
    let (response, _) = tls_over_connect(
        proxy_addr,
        "payments.example:443",
        "payments.example",
        ca.cert_der(),
        b"GET /hello HTTP/1.1\r\nHost: payments.example\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.contains("local-body"), "{response}");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert_eq!(seen.host.lock().unwrap().as_str(), &format!("127.0.0.1:{}", local.port()));
    assert_eq!(seen.forwarded_host.lock().unwrap().as_str(), "payments.example");
    assert_eq!(seen.forwarded_proto.lock().unwrap().as_str(), "https");
    assert_eq!(seen.path.lock().unwrap().as_str(), "/hello");
    proxy.stop().await;

    let (origin, origin_cert) = tls_origin(b"upstream-tls").await;
    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: true,
        rules: Vec::new(),
    });
    let (proxy, proxy_addr, _) = start_proxy_with(
        engine,
        Duration::from_secs(3),
        HostResolver::fixed(vec![origin]),
        false,
        Some(ca.clone()),
    )
    .await;
    let (response, peer) = tls_over_connect(
        proxy_addr,
        "unmatched.example:443",
        "unmatched.example",
        origin_cert.clone(),
        b"GET / HTTP/1.1\r\nHost: unmatched.example\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.contains("upstream-tls"), "{response}");
    assert_eq!(peer, origin_cert.as_ref());
    proxy.stop().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn intercepted_https_destination_keeps_certificate_verification() {
    let (origin, _) = tls_origin(b"should-not-see").await;
    let dir = std::env::temp_dir().join(format!("ler-ca-{}", uuid::Uuid::new_v4()));
    let ca = crate::tls::CertificateAuthority::open(&dir);
    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: true,
        rules: vec![rule("secure.example", &format!("https://127.0.0.1:{}/", origin.port()))],
    });
    let (proxy, proxy_addr, _) = start_proxy_with(engine, Duration::from_secs(3), HostResolver::system(), true, Some(ca.clone())).await;
    let (response, _) = tls_over_connect(
        proxy_addr,
        "secure.example:443",
        "secure.example",
        ca.cert_der(),
        b"GET /secret HTTP/1.1\r\nHost: secure.example\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
    assert!(!response.contains("should-not-see"), "{response}");
    proxy.stop().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn unmapped_sni_does_not_receive_a_leaf() {
    let seen = Arc::new(Seen::default());
    let seen_for_server = seen.clone();
    let local = spawn_server(move |request| {
        let seen = seen_for_server.clone();
        async move {
            let (parts, _) = request.into_parts();
            remember(&seen, parts.method.as_str(), parts.uri.path(), &parts.headers, b"");
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from_static(b"local-body")))
                .unwrap()
        }
    })
    .await;
    let dir = std::env::temp_dir().join(format!("ler-ca-{}", uuid::Uuid::new_v4()));
    let ca = crate::tls::CertificateAuthority::open(&dir);
    let engine = Arc::new(RuleEngine::new());
    engine.store(RuleSnapshot {
        profile_id: "default".into(),
        https_interception: true,
        rules: vec![rule("payments.example", &format!("http://127.0.0.1:{}/", local.port()))],
    });
    let (proxy, proxy_addr, _) = start_proxy_with(engine, Duration::from_secs(3), HostResolver::system(), true, Some(ca.clone())).await;
    use tokio::io::AsyncWriteExt;
    let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
    stream
        .write_all(b"CONNECT payments.example:443 HTTP/1.1\r\nHost: payments.example:443\r\n\r\n")
        .await
        .unwrap();
    assert_eq!(read_connect_preface(&mut stream).await, 200);
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca.cert_der()).unwrap();
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let name = rustls::pki_types::ServerName::try_from("other.example").unwrap();
    let handshake = connector.connect(name, stream).await;
    assert!(handshake.is_err(), "an unmapped SNI must not complete TLS with a local leaf");
    assert_eq!(seen.hits.load(Ordering::SeqCst), 0);
    proxy.stop().await;
    let _ = std::fs::remove_dir_all(dir);
}
