//! Tests for the checks applied at the HTTP boundary, ahead of the routes.

use super::gate::BodyReadError;
use super::*;
use crate::client_identity;
use crate::rpc_method_attribute::RpcMethodDescriptor;
use crate::rpc_server::RpcHandler;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use bytes::Bytes;
use hyper::header::{AUTHORIZATION, CONTENT_LENGTH, HeaderMap};
use hyper::service::{Service, service_fn};
use hyper::{Body, Request, Response};
use neo_protocol::ProtocolSettings;
use neo_system::NeoSystem;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const USER: &str = "testuser";
const PASS: &str = "testpass";

fn peer() -> SocketAddr {
    "203.0.113.7:4242".parse().expect("peer address")
}

fn secured(settings: RpcServerConfig) -> RpcServerConfig {
    RpcServerConfig {
        rpc_user: USER.to_string(),
        rpc_pass: PASS.to_string(),
        ..settings
    }
}

fn basic(user: &str, pass: &str) -> String {
    format!("Basic {}", BASE64_STANDARD.encode(format!("{user}:{pass}")))
}

/// Missing credentials and wrong credentials.
fn rejected_credentials() -> [Option<String>; 2] {
    [None, Some(basic(USER, "wrong"))]
}

/// A request body that records when anything reads from it.
fn counted_body(content: &'static [u8], reads: &Arc<AtomicUsize>) -> Body {
    let reads = Arc::clone(reads);
    Body::wrap_stream(futures::stream::once(async move {
        reads.fetch_add(1, Ordering::SeqCst);
        Ok::<_, Infallible>(content)
    }))
}

fn post(body: Body, content_length: usize, authorization: Option<&str>) -> Request<Body> {
    let mut builder = Request::post("/").header(CONTENT_LENGTH, content_length);
    if let Some(value) = authorization {
        builder = builder.header(AUTHORIZATION, value);
    }
    builder.body(body).expect("request")
}

fn get(path_and_query: &str, authorization: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path_and_query);
    if let Some(value) = authorization {
        builder = builder.header(AUTHORIZATION, value);
    }
    builder.body(Body::empty()).expect("request")
}

/// Stands in for the routes: counts calls and reads the whole body.
fn draining_service(
    calls: &Arc<AtomicUsize>,
) -> impl Service<Request<Body>, Response = Response<Body>, Error = Infallible> + use<> {
    let calls = Arc::clone(calls);
    service_fn(move |request: Request<Body>| {
        let calls = Arc::clone(&calls);
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            let _ = hyper::body::to_bytes(request.into_body()).await;
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }
    })
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: bytes::Bytes,
}

impl Answer {
    async fn from_response(response: Response<Body>) -> Self {
        let (parts, body) = response.into_parts();
        let body = hyper::body::to_bytes(body).await.expect("response body");
        Self {
            status: parts.status,
            headers: parts.headers,
            body,
        }
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("JSON response body")
    }

    fn error_code(&self) -> Option<i64> {
        self.json()
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64)
    }

    /// The transport-level refusal of a request that failed authentication:
    /// HTTP 401 with a challenge and, the body being unread, a null id.
    fn assert_unauthorized(&self) {
        assert_eq!(self.status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            self.headers
                .get(WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok()),
            Some("Basic realm=\"Restricted\"")
        );
        assert_eq!(
            self.error_code(),
            Some(RpcError::access_denied().code() as i64)
        );
        assert_eq!(self.json().get("id"), Some(&Value::Null));
    }
}

/// The gate in front of the real routes, wired as `start_rpc_server` wires
/// them, with one public method (`echo`) that counts its invocations.
struct Harness {
    gate: Arc<HttpGate>,
    routes: warp::filters::BoxedFilter<(HttpResponse,)>,
    handler_calls: Arc<AtomicUsize>,
    _server: Arc<RwLock<RpcServer>>,
}

impl Harness {
    fn new(settings: RpcServerConfig) -> Self {
        let handler_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&handler_calls);
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let mut server = RpcServer::new(system, settings.clone());
        server.register_handlers(vec![RpcHandler::new(
            RpcMethodDescriptor::new("echo"),
            Arc::new(move |_, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Value::String("pong".to_string()))
            }),
        )]);
        let server = Arc::new(RwLock::new(server));

        let auth = Arc::new(BasicAuth::from_settings(&settings));
        let routes = build_rpc_routes(
            Arc::downgrade(&server),
            Arc::new(HashSet::new()),
            Arc::clone(&auth),
            settings.clone(),
        )
        .boxed();
        Self {
            gate: Arc::new(HttpGate::new(auth, &settings)),
            routes,
            handler_calls,
            _server: server,
        }
    }

    async fn send(&self, request: Request<Body>) -> Answer {
        self.send_from(peer(), request).await
    }

    async fn send_from(&self, remote: SocketAddr, request: Request<Body>) -> Answer {
        let response = Arc::clone(&self.gate)
            .serve_scoped(remote, request, warp::service(self.routes.clone()))
            .await
            .expect("gate");
        Answer::from_response(response).await
    }

    fn handler_calls(&self) -> usize {
        self.handler_calls.load(Ordering::SeqCst)
    }
}

const ECHO: &[u8] = br#"{"jsonrpc":"2.0","method":"echo","params":[],"id":7}"#;

#[tokio::test]
async fn unauthenticated_request_is_refused_before_its_body_is_read() {
    let settings = secured(RpcServerConfig::default());
    let gate = Arc::new(HttpGate::new(
        Arc::new(BasicAuth::from_settings(&settings)),
        &settings,
    ));
    let reads = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));

    for authorization in rejected_credentials() {
        let request = post(
            counted_body(ECHO, &reads),
            ECHO.len(),
            authorization.as_deref(),
        );
        let response = Arc::clone(&gate)
            .serve_scoped(peer(), request, draining_service(&calls))
            .await
            .expect("gate");

        Answer::from_response(response).await.assert_unauthorized();
        assert_eq!(reads.load(Ordering::SeqCst), 0, "body must stay unread");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "routes must not run");
    }

    let request = post(
        counted_body(ECHO, &reads),
        ECHO.len(),
        Some(&basic(USER, PASS)),
    );
    let response = Arc::clone(&gate)
        .serve_scoped(peer(), request, draining_service(&calls))
        .await
        .expect("gate");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_body_is_parsed_only_for_authenticated_requests() {
    const MALFORMED: &[u8] = b"{ invalid json";
    let harness = Harness::new(secured(RpcServerConfig::default()));
    let reads = Arc::new(AtomicUsize::new(0));

    for authorization in rejected_credentials() {
        let request = post(
            counted_body(MALFORMED, &reads),
            MALFORMED.len(),
            authorization.as_deref(),
        );
        harness.send(request).await.assert_unauthorized();
        assert_eq!(reads.load(Ordering::SeqCst), 0, "body must stay unread");
    }

    let request = post(
        counted_body(MALFORMED, &reads),
        MALFORMED.len(),
        Some(&basic(USER, PASS)),
    );
    let answer = harness.send(request).await;

    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(
        answer.error_code(),
        Some(RpcError::bad_request().code() as i64)
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(harness.handler_calls(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_body_is_measured_only_for_authenticated_requests() {
    const OVERSIZED: &[u8] = &[b' '; 64];
    let harness = Harness::new(secured(RpcServerConfig {
        max_request_body_size: 16,
        ..Default::default()
    }));
    let reads = Arc::new(AtomicUsize::new(0));

    for authorization in rejected_credentials() {
        let request = post(
            counted_body(OVERSIZED, &reads),
            OVERSIZED.len(),
            authorization.as_deref(),
        );
        harness.send(request).await.assert_unauthorized();
    }

    let request = post(
        counted_body(OVERSIZED, &reads),
        OVERSIZED.len(),
        Some(&basic(USER, PASS)),
    );
    let answer = harness.send(request).await;

    // Same answer as before the gate existed: the declared length alone is
    // enough to reject it.
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(reads.load(Ordering::SeqCst), 0, "body must stay unread");
    assert_eq!(harness.handler_calls(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_get_is_parsed_only_for_authenticated_requests() {
    const MALFORMED: &str = "/?jsonrpc=2.0&method=echo&id=1&params=not-json";
    let harness = Harness::new(secured(RpcServerConfig::default()));

    for authorization in rejected_credentials() {
        let request = get(MALFORMED, authorization.as_deref());
        harness.send(request).await.assert_unauthorized();
    }

    let answer = harness.send(get(MALFORMED, Some(&basic(USER, PASS)))).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(
        answer.error_code(),
        Some(RpcError::invalid_request().code() as i64)
    );
    assert_eq!(harness.handler_calls(), 0);

    let answer = harness
        .send(get(
            "/?jsonrpc=2.0&method=echo&id=1&params=%5B%5D",
            Some(&basic(USER, PASS)),
        ))
        .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json().get("result"), Some(&json!("pong")));
    assert_eq!(harness.handler_calls(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn server_without_credentials_stays_open_for_public_methods() {
    let harness = Harness::new(RpcServerConfig::default());

    let answer = harness.send(post(Body::from(ECHO), ECHO.len(), None)).await;

    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json().get("result"), Some(&json!("pong")));
    assert_eq!(answer.json().get("id"), Some(&json!(7)));
    assert_eq!(harness.handler_calls(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn admission_limit_is_applied_per_peer_before_authentication() {
    let harness = Harness::new(secured(RpcServerConfig {
        admission_requests_per_second: Some(1),
        admission_burst: Some(2),
        ..Default::default()
    }));
    let reads = Arc::new(AtomicUsize::new(0));
    let unauthenticated = || post(counted_body(ECHO, &reads), ECHO.len(), None);

    harness.send(unauthenticated()).await.assert_unauthorized();
    harness.send(unauthenticated()).await.assert_unauthorized();

    // The burst is spent: the peer is now refused before its credentials are
    // even looked at, valid or not.
    for authorization in [None, Some(basic(USER, PASS))] {
        let request = post(
            counted_body(ECHO, &reads),
            ECHO.len(),
            authorization.as_deref(),
        );
        let answer = harness.send(request).await;
        assert_eq!(answer.status, StatusCode::TOO_MANY_REQUESTS);
        assert!(answer.headers.get(WWW_AUTHENTICATE).is_none());
        assert_eq!(
            answer.error_code(),
            Some(RpcError::too_many_requests().code() as i64)
        );
    }

    // Another peer has a budget of its own.
    let other: SocketAddr = "203.0.113.8:4242".parse().expect("peer address");
    harness
        .send_from(other, unauthenticated())
        .await
        .assert_unauthorized();

    assert_eq!(reads.load(Ordering::SeqCst), 0, "body must stay unread");
    assert_eq!(harness.handler_calls(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn admission_draws_nothing_from_the_method_tier_budget() {
    // `echo` is a Standard-tier method, so its budget is exactly one call.
    // Were admission charged against the per-method limiter, the admitted
    // call would find that budget already spent.
    let harness = Harness::new(RpcServerConfig {
        max_requests_per_second: 1,
        rate_limit_burst: 1,
        ..Default::default()
    });

    let first = harness.send(post(Body::from(ECHO), ECHO.len(), None)).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.json().get("result"), Some(&json!("pong")));

    // The second call is admitted (the derived admission budget is larger)
    // and then limited by its method tier, as a JSON-RPC error with its id.
    let second = harness.send(post(Body::from(ECHO), ECHO.len(), None)).await;
    assert_eq!(second.status, StatusCode::OK);
    assert_eq!(
        second.error_code(),
        Some(RpcError::too_many_requests().code() as i64)
    );
    assert_eq!(second.json().get("id"), Some(&json!(7)));
    assert_eq!(harness.handler_calls(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn preflight_is_answered_without_credentials_or_admission_budget() {
    let harness = Harness::new(secured(RpcServerConfig {
        enable_cors: true,
        allow_origins: vec!["https://a.example".to_string()],
        admission_requests_per_second: Some(1),
        admission_burst: Some(1),
        ..Default::default()
    }));

    for _ in 0..3 {
        let request = Request::options("/")
            .header("origin", "https://a.example")
            .body(Body::empty())
            .expect("request");
        let answer = harness.send(request).await;
        assert_eq!(answer.status, StatusCode::NO_CONTENT);
        assert_eq!(
            answer
                .headers
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some("https://a.example")
        );
    }

    // A refusal carries the CORS headers too, so a browser client can read it.
    let request = Request::post("/")
        .header(CONTENT_LENGTH, ECHO.len())
        .header("origin", "https://a.example")
        .body(Body::from(ECHO))
        .expect("request");
    let answer = harness.send(request).await;
    answer.assert_unauthorized();
    assert_eq!(
        answer
            .headers
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("https://a.example")
    );
}

#[tokio::test]
async fn websocket_upgrade_reaches_its_route_untouched() {
    let upgrade = |authorization: Option<&str>| {
        let mut builder = Request::get("/ws")
            .header("connection", "upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==");
        if let Some(value) = authorization {
            builder = builder.header(AUTHORIZATION, value);
        }
        builder.body(Body::empty()).expect("request")
    };
    let calls = Arc::new(AtomicUsize::new(0));

    let open = RpcServerConfig::default();
    let gate = Arc::new(HttpGate::new(Arc::new(None), &open));
    let response = gate
        .serve_scoped(peer(), upgrade(None), draining_service(&calls))
        .await
        .expect("gate");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let settings = secured(RpcServerConfig::default());
    let gate = Arc::new(HttpGate::new(
        Arc::new(BasicAuth::from_settings(&settings)),
        &settings,
    ));
    let response = Arc::clone(&gate)
        .serve_scoped(peer(), upgrade(None), draining_service(&calls))
        .await
        .expect("gate");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let response = gate
        .serve_scoped(
            peer(),
            upgrade(Some(&basic(USER, PASS))),
            draining_service(&calls),
        )
        .await
        .expect("gate");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn client_identity_is_the_tcp_peer_not_a_forwarding_header() {
    const SPOOFED: &str = "198.51.100.9";
    let settings = RpcServerConfig {
        admission_requests_per_second: Some(1),
        admission_burst: Some(1),
        ..Default::default()
    };
    let gate = Arc::new(HttpGate::new(Arc::new(None), &settings));
    let forwarded = |claimed: &str| {
        Request::post("/")
            .header(CONTENT_LENGTH, 0)
            .header("x-forwarded-for", claimed)
            .header("x-real-ip", claimed)
            .header("forwarded", format!("for={claimed}"))
            .body(Body::empty())
            .expect("request")
    };

    let seen = Arc::new(parking_lot::Mutex::new(None));
    let recorder = {
        let seen = Arc::clone(&seen);
        service_fn(move |request: Request<Body>| {
            let seen = Arc::clone(&seen);
            async move {
                *seen.lock() = Some((
                    client_identity::current_client_key(),
                    request.extensions().get::<SocketAddr>().copied(),
                ));
                Ok::<_, Infallible>(Response::new(Body::empty()))
            }
        })
    };

    let response = Arc::clone(&gate)
        .serve_scoped(peer(), forwarded(SPOOFED), recorder)
        .await
        .expect("gate");
    assert_eq!(response.status(), StatusCode::OK);

    // Session quotas (task-local key) and the method-tier limiter (request
    // extension) see the same address.
    let (client_key, remote) = seen.lock().take().expect("routes ran");
    assert_eq!(client_key, Some(peer().ip().to_string()));
    assert_eq!(remote, Some(peer()));

    // And so does admission: claiming to be someone else buys no new budget.
    let calls = Arc::new(AtomicUsize::new(0));
    let response = gate
        .serve_scoped(peer(), forwarded("198.51.100.10"), draining_service(&calls))
        .await
        .expect("gate");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

/// How long the `nap` method of a server from `start_server` takes.
const NAP: Duration = Duration::from_millis(1500);

/// Starts a real server on an ephemeral loopback port.
fn start_server(settings: RpcServerConfig) -> (Arc<RwLock<RpcServer>>, SocketAddr) {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
    let mut server = RpcServer::new(
        system,
        RpcServerConfig {
            port: 0,
            ..settings
        },
    );
    server.register_handlers(vec![
        RpcHandler::new(
            RpcMethodDescriptor::new("echo"),
            Arc::new(|_, _| Ok(Value::String("pong".to_string()))),
        ),
        RpcHandler::new(
            RpcMethodDescriptor::new("nap"),
            Arc::new(|_, _| {
                std::thread::sleep(NAP);
                Ok(Value::String("rested".to_string()))
            }),
        ),
    ]);
    let server = Arc::new(RwLock::new(server));
    server
        .write()
        .start_rpc_server(Arc::downgrade(&server), None);
    let address = server.read().local_addr().expect("bound address");
    (server, address)
}

/// Reads until the server closes the connection. A reset counts as closed.
async fn read_until_closed<R: AsyncRead + Unpin>(stream: &mut R) -> Vec<u8> {
    let mut received = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return received,
            Ok(read) => received.extend_from_slice(&chunk[..read]),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn server_answers_401_without_waiting_for_the_body() {
    let (server, address) = start_server(secured(RpcServerConfig::default()));

    let mut stream = TcpStream::connect(address).await.expect("connect");
    stream
        .write_all(
            b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
              Content-Length: 4096\r\n\r\n",
        )
        .await
        .expect("send request head");

    // None of the 4096 promised bytes is ever sent. The server answers from
    // the head alone and closes the connection instead of waiting for them.
    let received = tokio::time::timeout(Duration::from_secs(10), read_until_closed(&mut stream))
        .await
        .expect("server must answer and close without the body");
    let received = String::from_utf8_lossy(&received);
    assert!(received.starts_with("HTTP/1.1 401 "), "got: {received}");
    assert!(received.contains(r#""id":null"#), "got: {received}");

    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn server_serves_authenticated_request_over_a_socket() {
    let (server, address) = start_server(secured(RpcServerConfig::default()));

    let mut stream = TcpStream::connect(address).await.expect("connect");
    let head = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Authorization: {}\r
Connection: close\r
Content-Length: {}\r
\r
",
        basic(USER, PASS),
        ECHO.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .expect("send request head");
    stream.write_all(ECHO).await.expect("send request body");

    let received = tokio::time::timeout(Duration::from_secs(10), read_until_closed(&mut stream))
        .await
        .expect("server must answer");
    let received = String::from_utf8_lossy(&received);
    assert!(received.starts_with("HTTP/1.1 200 "), "got: {received}");

    server.write().stop_rpc_server();
}

/// A gate with no credentials and the given body limits, in seconds and
/// bytes.
fn body_gate(total: u64, idle: u64, max_len: usize) -> Arc<HttpGate> {
    let settings = RpcServerConfig {
        request_body_timeout: total,
        request_body_idle_timeout: idle,
        max_request_body_size: max_len,
        ..Default::default()
    };
    Arc::new(HttpGate::new(Arc::new(None), &settings))
}

/// Runs `request` through `gate` and reports how long the gate took.
async fn timed<S>(
    gate: &Arc<HttpGate>,
    request: Request<Body>,
    service: S,
) -> (Result<Response<Body>, BodyReadError>, Duration)
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>,
{
    let started = tokio::time::Instant::now();
    let result = Arc::clone(gate)
        .serve_scoped(peer(), request, service)
        .await;
    (result, started.elapsed())
}

/// Paused time moves from one timer to the next, each rounded up to a
/// millisecond.
fn assert_took(elapsed: Duration, expected: Duration) {
    assert!(
        elapsed >= expected && elapsed < expected + Duration::from_millis(50),
        "took {elapsed:?}, expected {expected:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn idle_deadline_restarts_only_when_data_arrives() {
    let gate = body_gate(0, 2, 1024);
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut sender, body) = Body::channel();
    let feed = async move {
        for piece in ["ab", "cd"] {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            sender
                .send_data(Bytes::from(piece))
                .await
                .expect("body is being read");
        }
        // Handed back still open: the body is never completed.
        sender
    };

    // Without a deadline this body would be awaited for ever; fail instead.
    let ((result, elapsed), _sender) = tokio::time::timeout(Duration::from_secs(600), async {
        tokio::join!(
            timed(&gate, post(body, 64, None), draining_service(&calls)),
            feed
        )
    })
    .await
    .expect("gate must give up on a stalled body");

    // Pieces 1.5 s apart keep a 2 s limit from passing. The stall after the
    // second piece, which arrived at 3 s, does not.
    assert!(
        matches!(result, Err(BodyReadError::Idle)),
        "got: {result:?}"
    );
    assert_took(elapsed, Duration::from_secs(5));
    assert_eq!(calls.load(Ordering::SeqCst), 0, "routes must not run");
}

#[tokio::test(start_paused = true)]
async fn empty_chunks_do_not_count_as_progress() {
    let gate = body_gate(0, 2, 1024);
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut sender, body) = Body::channel();
    let feed = async move {
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if sender.send_data(Bytes::new()).await.is_err() {
                break;
            }
        }
    };

    let ((result, elapsed), ()) = tokio::join!(
        timed(&gate, post(body, 64, None), draining_service(&calls)),
        feed
    );

    assert!(
        matches!(result, Err(BodyReadError::Idle)),
        "got: {result:?}"
    );
    assert_took(elapsed, Duration::from_secs(2));
    assert_eq!(calls.load(Ordering::SeqCst), 0, "routes must not run");
}

#[tokio::test(start_paused = true)]
async fn total_deadline_is_not_extended_by_progress() {
    let gate = body_gate(5, 2, 1024);
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut sender, body) = Body::channel();
    let feed = async move {
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if sender.send_data(Bytes::from(" ")).await.is_err() {
                break;
            }
        }
    };

    let ((result, elapsed), ()) = tokio::join!(
        timed(&gate, post(body, 64, None), draining_service(&calls)),
        feed
    );

    // A byte a second never trips the 2 s idle limit.
    assert!(
        matches!(result, Err(BodyReadError::Deadline)),
        "got: {result:?}"
    );
    assert_took(elapsed, Duration::from_secs(5));
    assert_eq!(calls.load(Ordering::SeqCst), 0, "routes must not run");
}

#[tokio::test(start_paused = true)]
async fn body_deadlines_end_once_the_body_is_complete() {
    const HANDLING: Duration = Duration::from_secs(3600);
    let gate = body_gate(1, 1, 1024);
    let slow = service_fn(|request: Request<Body>| async move {
        let body = hyper::body::to_bytes(request.into_body())
            .await
            .expect("buffered body");
        tokio::time::sleep(HANDLING).await;
        Ok::<_, Infallible>(Response::new(Body::from(body)))
    });

    let (result, elapsed) = timed(&gate, post(Body::from(ECHO), ECHO.len(), None), slow).await;

    let response = result.expect("handling a received request is not timed");
    assert_took(elapsed, HANDLING);
    let echoed = hyper::body::to_bytes(response.into_body())
        .await
        .expect("response body");
    assert_eq!(&echoed[..], ECHO);
}

#[tokio::test(start_paused = true)]
async fn body_deadlines_can_be_disabled() {
    const DELAY: Duration = Duration::from_secs(24 * 3600);
    let gate = body_gate(0, 0, 1024);
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut sender, body) = Body::channel();
    let feed = async move {
        tokio::time::sleep(DELAY).await;
        sender
            .send_data(Bytes::from(ECHO))
            .await
            .expect("body is being read");
    };

    let ((result, elapsed), ()) = tokio::join!(
        timed(
            &gate,
            post(body, ECHO.len(), None),
            draining_service(&calls)
        ),
        feed
    );

    assert_eq!(result.expect("gate").status(), StatusCode::OK);
    assert_took(elapsed, DELAY);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn body_received_in_pieces_reaches_the_routes_whole() {
    let harness = Harness::new(RpcServerConfig::default());
    let (mut sender, body) = Body::channel();
    let feed = async move {
        for piece in ECHO.chunks(7) {
            sender
                .send_data(Bytes::from(piece))
                .await
                .expect("body is being read");
        }
    };

    let (answer, ()) = tokio::join!(harness.send(post(body, ECHO.len(), None)), feed);

    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json().get("result"), Some(&json!("pong")));
    assert_eq!(answer.json().get("id"), Some(&json!(7)));
    assert_eq!(harness.handler_calls(), 1);
}

#[tokio::test]
async fn body_is_measured_as_it_arrives_not_as_declared() {
    let gate = body_gate(0, 0, 16);
    let calls = Arc::new(AtomicUsize::new(0));
    let pieces = |pieces: [&'static str; 2]| {
        Body::wrap_stream(futures::stream::iter(pieces.map(Ok::<_, Infallible>)))
    };

    // Sixteen bytes are allowed, however they arrive.
    let (result, _) = timed(
        &gate,
        post(pieces(["12345678", "12345678"]), 8, None),
        draining_service(&calls),
    )
    .await;
    assert_eq!(result.expect("gate").status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Seventeen are not, whatever length was declared.
    let (result, _) = timed(
        &gate,
        post(pieces(["12345678", "123456789"]), 8, None),
        draining_service(&calls),
    )
    .await;
    assert!(
        matches!(result, Err(BodyReadError::TooLarge)),
        "got: {result:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "routes must not run");
}

#[tokio::test(flavor = "multi_thread")]
async fn post_without_content_length_is_rejected_unread() {
    let harness = Harness::new(RpcServerConfig::default());
    let reads = Arc::new(AtomicUsize::new(0));
    let request = Request::post("/")
        .body(counted_body(ECHO, &reads))
        .expect("request");

    let answer = harness.send(request).await;

    // Same answer as before the gate received bodies.
    assert_eq!(answer.status, StatusCode::LENGTH_REQUIRED);
    assert_eq!(reads.load(Ordering::SeqCst), 0, "body must stay unread");
    assert_eq!(harness.handler_calls(), 0);
}

const POST_HEAD: &str = "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
                         Connection: close\r\n";

/// The head of a POST that declares `content_length` body bytes.
fn post_head(content_length: usize) -> String {
    format!("{POST_HEAD}Content-Length: {content_length}\r\n\r\n")
}

/// A complete POST of `body`.
fn post_request(body: &[u8]) -> Vec<u8> {
    let mut request = post_head(body.len()).into_bytes();
    request.extend_from_slice(body);
    request
}

/// Everything the server sends before it closes the connection. The wait is
/// far shorter than any deadline a test wants to show was not the cause.
async fn response_of<R: AsyncRead + Unpin>(stream: &mut R) -> String {
    let received = tokio::time::timeout(Duration::from_secs(20), read_until_closed(stream))
        .await
        .expect("server must close the connection");
    String::from_utf8_lossy(&received).into_owned()
}

/// Sends `request` on a new connection and returns the server's response.
async fn exchange(address: SocketAddr, request: &[u8]) -> String {
    let mut stream = TcpStream::connect(address).await.expect("connect");
    // A connection the server does not want may already be closed.
    let _ = stream.write_all(request).await;
    response_of(&mut stream).await
}

#[tokio::test(flavor = "multi_thread")]
async fn stalled_body_is_cut_off_at_the_idle_deadline() {
    let (server, address) = start_server(RpcServerConfig {
        request_body_timeout: 60,
        request_body_idle_timeout: 1,
        ..Default::default()
    });

    let started = std::time::Instant::now();
    let mut stream = TcpStream::connect(address).await.expect("connect");
    let mut sent = post_head(ECHO.len()).into_bytes();
    sent.extend_from_slice(&ECHO[..ECHO.len() / 2]);
    stream.write_all(&sent).await.expect("send partial request");

    // The rest never comes. The connection is closed without a response.
    let received = response_of(&mut stream).await;
    assert_eq!(received, "");
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "closed after {:?}",
        started.elapsed()
    );

    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn dribbled_body_is_cut_off_at_the_total_deadline() {
    let (server, address) = start_server(RpcServerConfig {
        request_body_timeout: 2,
        request_body_idle_timeout: 1,
        ..Default::default()
    });

    let started = std::time::Instant::now();
    let mut stream = TcpStream::connect(address).await.expect("connect");
    stream
        .write_all(post_head(4096).as_bytes())
        .await
        .expect("send request head");
    let (mut reader, mut writer) = stream.into_split();
    let dribble = tokio::spawn(async move {
        // Whitespace is valid ahead of a JSON value, so nothing but the
        // deadline can end this body early.
        while writer.write_all(b" ").await.is_ok() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    // A byte every 100 ms keeps the 1 s idle limit from passing, which the
    // connection outliving it shows. The total limit closes it regardless.
    let received = response_of(&mut reader).await;
    assert_eq!(received, "");
    assert!(
        started.elapsed() >= Duration::from_secs(2),
        "closed after {:?}",
        started.elapsed()
    );

    dribble.abort();
    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn timely_body_sent_in_pieces_is_served() {
    let (server, address) = start_server(RpcServerConfig {
        request_body_timeout: 2,
        request_body_idle_timeout: 1,
        ..Default::default()
    });

    let mut stream = TcpStream::connect(address).await.expect("connect");
    stream
        .write_all(post_head(ECHO.len()).as_bytes())
        .await
        .expect("send request head");
    for piece in ECHO.chunks(20) {
        tokio::time::sleep(Duration::from_millis(50)).await;
        stream.write_all(piece).await.expect("send request body");
    }

    let received = response_of(&mut stream).await;
    assert!(received.starts_with("HTTP/1.1 200 "), "got: {received}");

    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn body_deadline_does_not_cut_off_a_slow_handler() {
    const NAP_CALL: &[u8] = br#"{"jsonrpc":"2.0","method":"nap","params":[],"id":1}"#;
    let (server, address) = start_server(RpcServerConfig {
        request_body_timeout: 1,
        request_body_idle_timeout: 1,
        ..Default::default()
    });

    let started = std::time::Instant::now();
    let received = exchange(address, &post_request(NAP_CALL)).await;

    // The handler runs for longer than either body deadline.
    assert!(received.starts_with("HTTP/1.1 200 "), "got: {received}");
    assert!(started.elapsed() >= NAP, "took {:?}", started.elapsed());

    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn timed_out_connection_gives_its_slot_back() {
    let (server, address) = start_server(RpcServerConfig {
        max_concurrent_connections: 1,
        request_body_timeout: 60,
        request_body_idle_timeout: 2,
        ..Default::default()
    });

    let mut stalled = TcpStream::connect(address).await.expect("connect");
    stalled
        .write_all(post_head(ECHO.len()).as_bytes())
        .await
        .expect("send request head");

    // While the stalled connection holds the only slot, nobody else is
    // served.
    let turned_away = exchange(address, &post_request(ECHO)).await;
    assert_eq!(turned_away, "");

    // The idle deadline closes it...
    assert_eq!(response_of(&mut stalled).await, "");

    // ...and that frees the slot. The server lets go of it just after the
    // client sees the close, hence the retries.
    let mut served = String::new();
    for _ in 0..100 {
        served = exchange(address, &post_request(ECHO)).await;
        if !served.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(served.starts_with("HTTP/1.1 200 "), "got: {served}");

    server.write().stop_rpc_server();
}

#[tokio::test(flavor = "multi_thread")]
async fn body_size_and_length_rules_hold_over_a_socket() {
    let (server, address) = start_server(RpcServerConfig {
        max_request_body_size: 64,
        ..Default::default()
    });
    assert!(ECHO.len() <= 64);
    let chunked = |content: &[u8]| {
        let mut body = format!("{:x}\r\n", content.len()).into_bytes();
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n0\r\n\r\n");
        body
    };

    // Declared length over the limit: rejected from the head, as before.
    let received = exchange(address, post_head(4096).as_bytes()).await;
    assert!(received.starts_with("HTTP/1.1 413 "), "got: {received}");

    // No declared length, with or without a chunked body: rejected as before.
    let received = exchange(address, format!("{POST_HEAD}\r\n").as_bytes()).await;
    assert!(received.starts_with("HTTP/1.1 411 "), "got: {received}");

    let mut request = format!("{POST_HEAD}Transfer-Encoding: chunked\r\n\r\n").into_bytes();
    request.extend_from_slice(&chunked(ECHO));
    let received = exchange(address, &request).await;
    assert!(received.starts_with("HTTP/1.1 411 "), "got: {received}");

    // A declared length followed by chunked encoding is framed by the
    // chunks, so the declared length says nothing about the size. Within the
    // limit it is served, as before...
    let both = format!("{POST_HEAD}Content-Length: 8\r\nTransfer-Encoding: chunked\r\n\r\n");
    let mut request = both.clone().into_bytes();
    request.extend_from_slice(&chunked(ECHO));
    let received = exchange(address, &request).await;
    assert!(received.starts_with("HTTP/1.1 200 "), "got: {received}");

    // ...and over the limit the connection is closed.
    let mut request = both.into_bytes();
    request.extend_from_slice(&chunked(&[b' '; 200]));
    let received = exchange(address, &request).await;
    assert_eq!(received, "");

    server.write().stop_rpc_server();
}
