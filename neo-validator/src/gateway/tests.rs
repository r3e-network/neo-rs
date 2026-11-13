use super::*;
use crate::state::{ValidationPhase, VerifiedPoint};
use crate::upstream::NodeKind;
use std::sync::atomic::{AtomicU32, Ordering};

#[test]
fn wrong_or_missing_bearer_is_rejected() {
    let expected = TokenDigest::from_token("secret");
    let mut headers = hyper::HeaderMap::new();
    assert!(!bearer_matches(&headers, &expected));
    headers.insert(
        hyper::header::AUTHORIZATION,
        "Bearer wrong".parse().unwrap(),
    );
    assert!(!bearer_matches(&headers, &expected));
    headers.insert(
        hyper::header::AUTHORIZATION,
        "Bearer secret".parse().unwrap(),
    );
    assert!(bearer_matches(&headers, &expected));
}

#[tokio::test]
async fn chunked_body_cannot_bypass_limit() {
    let body = Body::from(vec![b'x'; 65]);
    assert!(read_bounded(body, 64).await.is_err());
}

#[tokio::test]
async fn healthy_proxy_forwards_only_at_verified_height() {
    let gates = healthy_gates();
    let (url, server) = spawn_upstream(gates.clone(), false, false).await;
    let gateway = gateway(gates, &url);
    let response = gateway.handle(request()).await;
    assert_eq!(response.status(), StatusCode::OK);
    server.abort();
}

#[tokio::test]
async fn incident_during_forward_discards_upstream_response() {
    let gates = healthy_gates();
    let (url, server) = spawn_upstream(gates.clone(), true, false).await;
    let gateway = gateway(gates.clone(), &url);
    let response = gateway.handle(request()).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(gates.status().phase, ValidationPhase::Mismatch);
    server.abort();
}

#[tokio::test]
async fn block_advanced_during_forward_discards_upstream_response() {
    let gates = healthy_gates();
    let (url, server) = spawn_upstream(gates.clone(), false, true).await;
    let gateway = gateway(gates, &url);
    let response = gateway.handle(request()).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    server.abort();
}

fn healthy_gates() -> GateRegistry {
    let gates = GateRegistry::default();
    gates.mark_verified(VerifiedPoint {
        height: 7,
        block_hash: UInt256::zero(),
        state_root: UInt256::zero(),
    });
    gates
}

fn gateway(gates: GateRegistry, url: &str) -> ProxyGateway {
    let config = crate::config::UpstreamConfig {
        rpc_url: url.to_owned(),
        ..Default::default()
    };
    let node = HttpNode::new(
        NodeKind::NeoCli,
        &config,
        std::time::Duration::from_secs(2),
        1024 * 1024,
        1024 * 1024,
    )
    .unwrap();
    ProxyGateway::new(
        node,
        gates,
        Arc::new(BTreeSet::from(["getblockcount".to_owned()])),
        None,
        4096,
    )
}

fn request() -> Request<Body> {
    Request::builder()
        .method(hyper::Method::POST)
        .uri("/")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"getblockcount","params":[]}"#,
        ))
        .unwrap()
}

async fn spawn_upstream(
    gates: GateRegistry,
    incident: bool,
    advance: bool,
) -> (String, tokio::task::JoinHandle<()>) {
    let height = Arc::new(AtomicU32::new(8));
    let calls = Arc::new(AtomicU32::new(0));
    let make = make_service_fn(move |_| {
        let gates = gates.clone();
        let height = height.clone();
        let calls = calls.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |request: Request<Body>| {
                let gates = gates.clone();
                let height = height.clone();
                let calls = calls.clone();
                async move {
                    let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    let bytes = hyper::body::to_bytes(request.into_body()).await.unwrap();
                    let request: Value = serde_json::from_slice(&bytes).unwrap();
                    let id = request["id"].clone();
                    if call == 2 && incident {
                        gates.quarantine(
                            ValidationPhase::Mismatch,
                            8,
                            "in_flight_test",
                            "incident while forwarding",
                        );
                    }
                    if call == 2 && advance {
                        height.fetch_add(1, Ordering::SeqCst);
                    }
                    let body = json!({
                        "jsonrpc":"2.0",
                        "id":id,
                        "result":height.load(Ordering::SeqCst),
                    });
                    Ok::<_, Infallible>(Response::new(Body::from(body.to_string())))
                }
            }))
        }
    });
    let server = Server::try_bind(&"127.0.0.1:0".parse().unwrap())
        .unwrap()
        .serve(make);
    let address = server.local_addr();
    let handle = tokio::spawn(async move {
        let _ = server.await;
    });
    (format!("http://{address}"), handle)
}
