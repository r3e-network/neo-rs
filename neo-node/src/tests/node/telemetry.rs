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

fn rocksdb_test_node() -> (Arc<neo_system::Node>, tempfile::TempDir) {
    use neo_blockchain::HeaderCache;
    use neo_network::NetworkHandle;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::store::Store;
    use neo_storage::rocksdb::{RocksDBStoreProvider, WriteBatchConfig};

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let cfg = StorageConfig {
        path: tmp.path().join("telemetry-rocksdb"),
        ..Default::default()
    };
    let storage: Arc<dyn Store> = Arc::new(
        RocksDBStoreProvider::new(cfg)
            .with_batch_config(WriteBatchConfig::balanced())
            .get_rocksdb_store("")
            .expect("rocksdb store"),
    );
    let settings = Arc::new(neo_config::ProtocolSettings::testnet());
    let (blockchain, _rx) = neo_blockchain::BlockchainHandle::with_capacity();
    let (network, _nrx, _etx) = NetworkHandle::channel(8, 8);
    let node = neo_system::Node::builder()
        .with_settings(Arc::clone(&settings))
        .with_storage(storage)
        .with_blockchain(blockchain)
        .with_network(network)
        .with_mempool(Arc::new(neo_mempool::MemoryPool::new(&settings)))
        .with_header_cache(Arc::new(HeaderCache::default()))
        .build()
        .expect("node");
    (Arc::new(node), tmp)
}

fn mdbx_test_node(map_size: isize) -> (Arc<neo_system::Node>, tempfile::TempDir) {
    use neo_blockchain::HeaderCache;
    use neo_network::NetworkHandle;
    use neo_storage::persistence::StoreFactory;
    use neo_storage::persistence::storage::StorageConfig;

    let tmp = tempfile::TempDir::new().expect("tempdir");
    let settings = Arc::new(neo_config::ProtocolSettings::testnet());
    let storage = StoreFactory::get_store_with_config(
        "mdbx",
        StorageConfig {
            path: tmp.path().join("telemetry-mdbx"),
            mdbx_geometry_upper_bytes: Some(map_size),
            ..Default::default()
        },
    )
    .expect("mdbx store");
    let (blockchain, _rx) = neo_blockchain::BlockchainHandle::with_capacity();
    let (network, _nrx, _etx) = NetworkHandle::channel(8, 8);
    let node = neo_system::Node::builder()
        .with_settings(Arc::clone(&settings))
        .with_storage(storage)
        .with_blockchain(blockchain)
        .with_network(network)
        .with_mempool(Arc::new(neo_mempool::MemoryPool::new(&settings)))
        .with_header_cache(Arc::new(HeaderCache::default()))
        .build()
        .expect("node");
    (Arc::new(node), tmp)
}

fn remote_ledger_node(height: u32) -> Arc<neo_system::Node> {
    remote_ledger_node_with_height(Some(height))
}

fn remote_ledger_node_with_height(height: Option<u32>) -> Arc<neo_system::Node> {
    let node = test_node();
    node.register_service(Arc::new(
        super::super::remote_ledger::RemoteLedgerStatus::new("https://rpc.example.invalid", height),
    ));
    node
}

fn remote_ledger_node_with_error(error: &str) -> Arc<neo_system::Node> {
    let node = test_node();
    node.register_service(Arc::new(
        super::super::remote_ledger::RemoteLedgerStatus::unavailable(
            "https://rpc.example.invalid",
            error,
        ),
    ));
    node
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
fn metrics_exporter_uses_observability_ledger_provider() {
    let source = include_str!("../../node/telemetry/exporter.rs");

    assert!(
        source.contains("observability_ledger_height"),
        "metrics exporter should share observability ledger-height resolution"
    );
    assert!(
        !source.contains("StorageLedgerProviderFactory"),
        "metrics exporter should not construct storage ledger providers directly"
    );
}

#[test]
fn renders_mdbx_environment_metrics() {
    const MAP_SIZE: isize = 128 * 1024 * 1024;
    let (node, _tmp) = mdbx_test_node(MAP_SIZE);
    let exporter = MetricsExporter::new(node).expect("metrics exporter");

    let payload = exporter.render().expect("metrics payload");
    let text = String::from_utf8(payload).expect("utf8 metrics");

    assert!(text.contains(&format!("neo_storage_mdbx_map_size_bytes {MAP_SIZE}")));
    assert!(text.contains("neo_storage_mdbx_last_page_number"));
    assert!(text.contains("neo_storage_mdbx_last_transaction_id"));
    assert!(text.contains("neo_storage_mdbx_max_readers"));
    assert!(text.contains("neo_storage_mdbx_reader_slots_used"));
    assert!(
        !text.contains("neo_storage_rocksdb_batch_pending_operations"),
        "MDBX nodes should not emit RocksDB-only batch metrics"
    );
}

#[test]
fn renders_rocksdb_fast_sync_batch_metrics() {
    let (node, _tmp) = rocksdb_test_node();
    let storage = node.storage();
    storage
        .as_fast_sync_store()
        .expect("RocksDB store supports fast-sync")
        .enable_fast_sync_mode();

    for index in 0..3 {
        let mut writer = node.store_cache();
        writer.add(
            neo_storage::StorageKey::new(42, vec![index]),
            neo_storage::StorageItem::from_bytes(vec![index]),
        );
        writer.try_commit().expect("buffer fast-sync write");
    }

    let exporter = MetricsExporter::new(node).expect("metrics exporter");
    let payload = exporter.render().expect("metrics payload");
    let text = String::from_utf8(payload).expect("utf8 metrics");

    assert!(text.contains("neo_storage_rocksdb_batch_pending_operations 3"));
    assert!(text.contains("neo_storage_rocksdb_batch_batches_flushed_total 0"));
    assert!(text.contains("neo_storage_rocksdb_batch_operations_written_total 0"));
    assert!(text.contains("neo_storage_rocksdb_batch_disable_wal 1"));
    assert!(text.contains("neo_storage_rocksdb_batch_max_batch_size 5000"));

    storage.flush().expect("flush pending fast-sync writes");

    let payload = exporter.render().expect("metrics payload after flush");
    let text = String::from_utf8(payload).expect("utf8 metrics after flush");
    assert!(text.contains("neo_storage_rocksdb_batch_pending_operations 0"));
    assert!(text.contains("neo_storage_rocksdb_batch_batches_flushed_total 1"));
    assert!(text.contains("neo_storage_rocksdb_batch_operations_written_total 3"));
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
    assert!(text.contains("neo_sync_native_persist_blocks_total"));
    assert!(text.contains("neo_sync_native_persist_avg_onpersist_us"));
    assert!(text.contains("neo_sync_native_persist_avg_tx_us"));
    assert!(text.contains("neo_sync_native_persist_avg_cache_commit_us"));
    assert!(text.contains("neo_sync_native_contract_hook_calls_total"));
    assert!(text.contains(
        "neo_sync_native_contract_hook_avg_us{trigger=\"onpersist\",contract=\"GasToken\",id=\"-6\"}"
    ));
    assert!(text.contains("neo_sync_native_persist_tx_stage_calls_total"));
    assert!(text.contains("neo_sync_native_persist_tx_stage_avg_us{stage=\"load_execute\"}"));
    assert!(text.contains("neo_sync_neotoken_onpersist_stage_calls_total"));
    assert!(text.contains("neo_sync_neotoken_onpersist_stage_avg_us{stage=\"compute_committee\"}"));
    assert!(text.contains("neo_sync_neotoken_committee_compute_stage_calls_total"));
    assert!(text.contains(
        "neo_sync_neotoken_committee_compute_stage_avg_us{stage=\"candidate_state_decode\"}"
    ));
    assert!(text.contains(
        "neo_sync_neotoken_committee_compute_stage_avg_us{stage=\"candidate_blocked_prefetch\"}"
    ));
    assert!(text.contains("neo_sync_neotoken_committee_candidate_scan_items_total"));
    assert!(text.contains(
        "neo_sync_neotoken_committee_candidate_scan_avg_items{kind=\"eligible_candidates\"}"
    ));
    assert!(text.contains("neo_state_service_mpt_apply_blocks_total"));
    assert!(text.contains("neo_state_service_mpt_apply_avg_total_us"));
    assert!(text.contains("neo_state_service_mpt_apply_avg_changes"));
    assert!(text.contains("neo_state_service_mpt_apply_stage_calls_total"));
    assert!(text.contains("neo_state_service_mpt_apply_stage_avg_us{stage=\"queue_wait\"}"));
    assert!(text.contains("neo_state_service_mpt_apply_stage_avg_us{stage=\"enqueue_blocking\"}"));
    assert!(text.contains("neo_state_service_mpt_apply_stage_avg_us{stage=\"trie_commit\"}"));
    assert!(text.contains("neo_state_service_mpt_apply_items_total"));
    assert!(text.contains("neo_state_service_mpt_apply_avg_items{kind=\"overlay_entries\"}"));
    assert!(text.contains("neo_state_service_mpt_apply_avg_items{kind=\"batch_blocks\"}"));
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
async fn remote_ledger_readiness_uses_upstream_height_without_local_ledger() {
    let node = remote_ledger_node(42);
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
    assert_eq!(ready_json["ready"], true);
    assert_eq!(ready_json["ledger_height"], 42);
    assert_eq!(ready_json["ledger_source"], "remote_rpc");
    assert_eq!(
        ready_json["remote_ledger_rpc"],
        "https://rpc.example.invalid"
    );
}

#[tokio::test]
async fn remote_ledger_readiness_waits_when_upstream_height_is_unknown() {
    let node = remote_ledger_node_with_height(None);
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
    assert_eq!(ready_json["status"], "starting");
    assert_eq!(ready_json["ready"], false);
    assert!(ready_json["ledger_height"].is_null());
    assert_eq!(ready_json["ledger_source"], "remote_rpc");
    assert_eq!(
        ready_json["remote_ledger_rpc"],
        "https://rpc.example.invalid"
    );
}

#[tokio::test]
async fn remote_ledger_readiness_reports_upstream_tip_error() {
    let node = remote_ledger_node_with_error("remote getblockcount failed");
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
    assert_eq!(ready_json["status"], "starting");
    assert_eq!(ready_json["ready"], false);
    assert!(ready_json["ledger_height"].is_null());
    assert_eq!(ready_json["ledger_source"], "remote_rpc");
    assert_eq!(
        ready_json["remote_ledger_rpc"],
        "https://rpc.example.invalid"
    );
    assert_eq!(
        ready_json["remote_ledger_error"],
        "remote getblockcount failed"
    );
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
