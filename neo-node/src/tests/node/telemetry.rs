use std::sync::Arc;

use hyper::header::CONTENT_TYPE;
use hyper::{Body, Request, StatusCode};

use super::super::config::{TELEMETRY_HEALTH_PATH, TELEMETRY_READY_PATH};
use super::exporter::MetricsExporter;
use super::http::serve_metrics_request;

fn test_node() -> Arc<neo_system::Node> {
    Arc::new(
        neo_system::Node::new(
            Arc::new(neo_config::ProtocolSettings::testnet()),
            None,
            None,
        )
        .expect("node"),
    )
}

fn seed_ledger_height(node: &neo_system::Node, height: u32) {
    let pointer = neo_native_contracts::LedgerContract::new()
        .serialize_hash_index_state(&neo_primitives::UInt256::zero(), height)
        .expect("serialize current ledger pointer");
    let mut store = node.store_cache();
    store.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
        neo_storage::StorageItem::from_bytes(pointer),
    );
    store.commit();
}

fn indexed_service_at(height: u32) -> Arc<neo_indexer::IndexerService> {
    let indexer = Arc::new(neo_indexer::IndexerService::new());
    let mut header = neo_payloads::Header::new();
    header.set_index(height);
    indexer
        .index_block(&neo_payloads::Block::from_parts(header, Vec::new()))
        .expect("index block");
    indexer
}

#[test]
fn renders_node_metrics_payload() {
    let node = test_node();
    let exporter = MetricsExporter::new(node).expect("metrics exporter");

    let payload = exporter.render().expect("metrics payload");
    let text = String::from_utf8(payload).expect("utf8 metrics");

    assert!(text.contains("neo_node_up 1"));
    assert!(text.contains("neo_node_info"));
    assert!(text.contains("network=\"0x3554334E\""));
    assert!(text.contains("neo_node_mempool_transactions 0"));
    assert!(text.contains("neo_node_service_enabled{service=\"indexer\"} 0"));
    assert!(text.contains("neo_node_indexer_up 0"));
    assert!(text.contains("neo_node_indexer_indexed_height -1"));
    assert!(text.contains("neo_node_indexer_blocks_behind -1"));
    assert!(text.contains("neo_node_indexer_synced 0"));
}

#[test]
fn renders_indexer_service_metrics_when_registered() {
    let node = test_node();
    node.register_service(Arc::new(neo_indexer::IndexerService::new()));
    let exporter = MetricsExporter::new(node).expect("metrics exporter");

    let payload = exporter.render().expect("metrics payload");
    let text = String::from_utf8(payload).expect("utf8 metrics");

    assert!(text.contains("neo_node_service_enabled{service=\"indexer\"} 1"));
    assert!(text.contains("neo_node_indexer_up 1"));
    assert!(text.contains("neo_node_indexer_indexed_height -1"));
    assert!(text.contains("neo_node_indexer_indexed_blocks 0"));
    assert!(text.contains("neo_node_indexer_blocks_behind -1"));
    assert!(text.contains("neo_node_indexer_synced 0"));
}

#[test]
fn renders_indexer_lag_metrics_when_registered() {
    let node = test_node();
    seed_ledger_height(&node, 5);
    node.register_service(indexed_service_at(3));
    let exporter = MetricsExporter::new(node).expect("metrics exporter");

    let payload = exporter.render().expect("metrics payload");
    let text = String::from_utf8(payload).expect("utf8 metrics");

    assert!(text.contains("neo_node_ledger_height 5"));
    assert!(text.contains("neo_node_indexer_up 1"));
    assert!(text.contains("neo_node_indexer_indexed_height 3"));
    assert!(text.contains("neo_node_indexer_blocks_behind 2"));
    assert!(text.contains("neo_node_indexer_synced 0"));
}

#[test]
fn renders_indexer_ahead_of_ledger_as_unsynced() {
    let node = test_node();
    seed_ledger_height(&node, 3);
    node.register_service(indexed_service_at(5));
    let exporter = MetricsExporter::new(node).expect("metrics exporter");

    let payload = exporter.render().expect("metrics payload");
    let text = String::from_utf8(payload).expect("utf8 metrics");

    assert!(text.contains("neo_node_ledger_height 3"));
    assert!(text.contains("neo_node_indexer_indexed_height 5"));
    assert!(text.contains("neo_node_indexer_blocks_behind 0"));
    assert!(text.contains("neo_node_indexer_synced 0"));
}

#[tokio::test]
async fn serves_health_and_readiness_endpoints() {
    let node = test_node();
    let exporter = Arc::new(MetricsExporter::new(node).expect("metrics exporter"));

    let health = serve_metrics_request(
        Request::builder()
            .method(hyper::Method::GET)
            .uri(TELEMETRY_HEALTH_PATH)
            .body(Body::empty())
            .expect("health request"),
        "/metrics".to_string(),
        Arc::clone(&exporter),
    )
    .await
    .expect("health response");
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(
        health
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
    let health_body = hyper::body::to_bytes(health.into_body())
        .await
        .expect("health body");
    let health_json: serde_json::Value = serde_json::from_slice(&health_body).expect("health json");
    assert_eq!(health_json["status"], "ok");
    assert_eq!(health_json["service"], "neo-node");

    let ready = serve_metrics_request(
        Request::builder()
            .method(hyper::Method::GET)
            .uri(TELEMETRY_READY_PATH)
            .body(Body::empty())
            .expect("ready request"),
        "/metrics".to_string(),
        exporter,
    )
    .await
    .expect("ready response");
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    let ready_body = hyper::body::to_bytes(ready.into_body())
        .await
        .expect("ready body");
    let ready_json: serde_json::Value = serde_json::from_slice(&ready_body).expect("ready json");
    assert_eq!(ready_json["status"], "starting");
    assert_eq!(ready_json["ready"], false);
    assert_eq!(ready_json["ledger_height"], serde_json::Value::Null);
    assert_eq!(ready_json["services"]["indexer"]["enabled"], false);
    assert_eq!(ready_json["services"]["indexer"]["ready"], true);
}

#[tokio::test]
async fn readiness_reports_registered_indexer_status() {
    let node = test_node();
    node.register_service(Arc::new(neo_indexer::IndexerService::new()));
    let exporter = Arc::new(MetricsExporter::new(node).expect("metrics exporter"));

    let ready = serve_metrics_request(
        Request::builder()
            .method(hyper::Method::GET)
            .uri(TELEMETRY_READY_PATH)
            .body(Body::empty())
            .expect("ready request"),
        "/metrics".to_string(),
        exporter,
    )
    .await
    .expect("ready response");
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    let ready_body = hyper::body::to_bytes(ready.into_body())
        .await
        .expect("ready body");
    let ready_json: serde_json::Value = serde_json::from_slice(&ready_body).expect("ready json");
    assert_eq!(ready_json["services"]["indexer"]["enabled"], true);
    assert_eq!(ready_json["services"]["indexer"]["ready"], true);
    assert_eq!(ready_json["services"]["indexer"]["indexed_blocks"], 0);
}

#[tokio::test]
async fn readiness_reports_indexer_lag_and_sync_state() {
    let node = test_node();
    seed_ledger_height(&node, 5);
    node.register_service(indexed_service_at(3));
    let exporter = Arc::new(MetricsExporter::new(node).expect("metrics exporter"));

    let ready = serve_metrics_request(
        Request::builder()
            .method(hyper::Method::GET)
            .uri(TELEMETRY_READY_PATH)
            .body(Body::empty())
            .expect("ready request"),
        "/metrics".to_string(),
        exporter,
    )
    .await
    .expect("ready response");
    assert_eq!(ready.status(), StatusCode::OK);
    let ready_body = hyper::body::to_bytes(ready.into_body())
        .await
        .expect("ready body");
    let ready_json: serde_json::Value = serde_json::from_slice(&ready_body).expect("ready json");
    assert_eq!(ready_json["status"], "ready");
    assert_eq!(ready_json["ledger_height"], 5);
    assert_eq!(ready_json["services"]["indexer"]["indexed_height"], 3);
    assert_eq!(ready_json["services"]["indexer"]["blocks_behind"], 2);
    assert_eq!(ready_json["services"]["indexer"]["synced"], false);
}

#[tokio::test]
async fn readiness_reports_indexer_ahead_of_ledger_as_unsynced() {
    let node = test_node();
    seed_ledger_height(&node, 3);
    node.register_service(indexed_service_at(5));
    let exporter = Arc::new(MetricsExporter::new(node).expect("metrics exporter"));

    let ready = serve_metrics_request(
        Request::builder()
            .method(hyper::Method::GET)
            .uri(TELEMETRY_READY_PATH)
            .body(Body::empty())
            .expect("ready request"),
        "/metrics".to_string(),
        exporter,
    )
    .await
    .expect("ready response");
    assert_eq!(ready.status(), StatusCode::OK);
    let ready_body = hyper::body::to_bytes(ready.into_body())
        .await
        .expect("ready body");
    let ready_json: serde_json::Value = serde_json::from_slice(&ready_body).expect("ready json");
    assert_eq!(ready_json["ledger_height"], 3);
    assert_eq!(ready_json["services"]["indexer"]["indexed_height"], 5);
    assert_eq!(ready_json["services"]["indexer"]["blocks_behind"], 0);
    assert_eq!(ready_json["services"]["indexer"]["synced"], false);
}

#[tokio::test]
async fn telemetry_routes_reject_unknown_paths_and_non_get_methods() {
    let node = test_node();
    let exporter = Arc::new(MetricsExporter::new(node).expect("metrics exporter"));

    let missing = serve_metrics_request(
        Request::builder()
            .method(hyper::Method::GET)
            .uri("/missing")
            .body(Body::empty())
            .expect("missing request"),
        "/custom-metrics".to_string(),
        Arc::clone(&exporter),
    )
    .await
    .expect("missing response");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let wrong_method = serve_metrics_request(
        Request::builder()
            .method(hyper::Method::POST)
            .uri(TELEMETRY_HEALTH_PATH)
            .body(Body::empty())
            .expect("post request"),
        "/custom-metrics".to_string(),
        exporter,
    )
    .await
    .expect("post response");
    assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
}
