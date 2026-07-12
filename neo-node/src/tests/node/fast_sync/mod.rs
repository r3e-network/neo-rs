//! # Fast Sync Tests
//!
//! Regression tests for built-in fast-sync package import helpers.
//!
//! ## Boundary
//!
//! These tests validate node orchestration around package selection, download,
//! extraction, import proofs, reports, and recovery markers.
//!
//! ## Contents
//!
//! Fixtures and tests for package caching, preflight, reference verification,
//! throughput classification, report serialization, and cleanup.

use super::cache_dir::fast_sync_cache_dir;
use super::local::{
    LocalStateRootTip, local_state_root_tip, validate_fast_sync_preflight,
    verify_fast_sync_import_tip,
};
use super::marker::{
    clear_fast_sync_import_marker, refuse_stale_fast_sync_import_marker,
    write_fast_sync_import_marker,
};
use super::package::FastSyncPackage;
use super::reference;
use super::report::{
    FastSyncReferenceReport, FastSyncReport, FastSyncThroughputStatus, fast_sync_throughput_status,
};
use super::write_fast_sync_report_sidecar;
use crate::node::chain_acc;
use crate::node::config::NodeConfig;
use neo_io::{BinaryWriter, Serializable};
use neo_storage::persistence::providers::memory_store::MemoryStore;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[test]
fn default_cache_dir_tracks_storage_path() {
    let config: NodeConfig = toml::from_str(
        r#"
[storage]
data_dir = "/var/lib/neo/mainnet"
"#,
    )
    .expect("config");

    assert_eq!(
        fast_sync_cache_dir(&config, None, None),
        PathBuf::from("/var/lib/neo/mainnet/fast-sync")
    );
    assert_eq!(
        fast_sync_cache_dir(&config, Some(Path::new("/override")), None),
        PathBuf::from("/override/fast-sync")
    );
    assert_eq!(
        fast_sync_cache_dir(&config, None, Some(Path::new("/cache"))),
        PathBuf::from("/cache")
    );
}

fn test_package(start: u32, end: u32) -> FastSyncPackage {
    FastSyncPackage {
        network_key: "n3mainnet",
        url: "https://example.invalid/chain.0.acc.zip".to_string(),
        md5: "ABCDEF0123456789ABCDEF0123456789".to_string(),
        start,
        end,
        filename: format!("chain.{start}.acc.zip"),
    }
}

fn memory_store_with_ledger_tip(tip: u32) -> Arc<MemoryStore> {
    use neo_storage::{StorageItem, StorageKey};

    let store = Arc::new(MemoryStore::new());
    let mut cache = neo_storage::persistence::StoreCache::new_from_store(Arc::clone(&store), false);
    let hash = neo_primitives::UInt256::from([tip as u8; 32]);
    let current = neo_native_contracts::LedgerContract::new()
        .serialize_hash_index_state(&hash, tip)
        .expect("serialize current ledger pointer");
    cache.data_cache().add(
        StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
        StorageItem::from_bytes(current),
    );
    cache.try_commit().expect("commit test Ledger tip");
    store
}

fn state_store_with_local_root(
    tip: u32,
) -> (Arc<neo_state_service::StateStore>, neo_primitives::UInt256) {
    let state_store = Arc::new(neo_state_service::StateStore::with_mpt(true));
    let mpt = state_store.mpt().expect("MPT store");
    let mut root_before = None;
    for index in 0..=tip {
        let root = mpt
            .apply_block_changes(index, root_before, &[])
            .expect("apply empty MPT changes");
        root_before = Some(root);
    }
    let root_hash = root_before.expect("root applied");
    assert_eq!(mpt.current_local_root(), Some((tip, root_hash)));
    (state_store, root_hash)
}

fn empty_block(index: u32) -> neo_payloads::Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    neo_payloads::Block::from_parts(header, Vec::new())
}

fn serialized_hex<T: Serializable>(payload: &T) -> String {
    let mut writer = BinaryWriter::new();
    payload.serialize(&mut writer).expect("serialize payload");
    hex::encode(writer.into_bytes())
}

fn import_report(
    imported: u64,
    last_imported_tip: Option<chain_acc::LocalLedgerTip>,
    elapsed_seconds: f64,
    average_blocks_per_second: f64,
) -> chain_acc::ChainAccImportReport {
    chain_acc::ChainAccImportReport {
        imported,
        last_imported_tip,
        elapsed_seconds,
        driver_elapsed_seconds: elapsed_seconds,
        chain_acc_read_seconds: 0.0,
        chain_acc_validate_seconds: 0.0,
        average_blocks_per_second,
        empty_blocks: imported,
        empty_only_blocks: imported,
        empty_block_import_seconds: elapsed_seconds,
        empty_blocks_per_second: average_blocks_per_second,
        transaction_blocks: 0,
        transactions: 0,
        transaction_block_import_seconds: 0.0,
        transaction_block_clone_seconds: 0.0,
        transaction_ledger_insert_seconds: 0.0,
        transaction_finalized_delivery_seconds: 0.0,
        transaction_blocks_per_second: 0.0,
        finalization_seconds: 0.0,
        finalization_commit_handlers_seconds: 0.0,
        finalization_store_commit_seconds: 0.0,
        unclassified_import_seconds: 0.0,
        hot_metrics: chain_acc::ImportHotMetrics::default(),
    }
}

fn import_report_with_hot_metrics(
    mut report: chain_acc::ChainAccImportReport,
    hot_metrics: chain_acc::ImportHotMetrics,
) -> chain_acc::ChainAccImportReport {
    report.hot_metrics = hot_metrics;
    report
}

fn import_report_with_composition(
    imported: u64,
    last_imported_tip: Option<chain_acc::LocalLedgerTip>,
    elapsed_seconds: f64,
    average_blocks_per_second: f64,
    empty_blocks: u64,
    transaction_blocks: u64,
    transactions: u64,
) -> chain_acc::ChainAccImportReport {
    chain_acc::ChainAccImportReport {
        imported,
        last_imported_tip,
        elapsed_seconds,
        driver_elapsed_seconds: elapsed_seconds,
        chain_acc_read_seconds: 0.0,
        chain_acc_validate_seconds: 0.0,
        average_blocks_per_second,
        empty_blocks,
        empty_only_blocks: if transaction_blocks > 0 {
            0
        } else {
            empty_blocks
        },
        empty_block_import_seconds: if transaction_blocks > 0 {
            0.0
        } else {
            elapsed_seconds
        },
        empty_blocks_per_second: if transaction_blocks > 0 || elapsed_seconds <= 0.0 {
            0.0
        } else {
            empty_blocks as f64 / elapsed_seconds
        },
        transaction_blocks,
        transactions,
        transaction_block_import_seconds: if transaction_blocks > 0 {
            elapsed_seconds
        } else {
            0.0
        },
        transaction_block_clone_seconds: 0.0,
        transaction_ledger_insert_seconds: 0.0,
        transaction_finalized_delivery_seconds: 0.0,
        transaction_blocks_per_second: if elapsed_seconds > 0.0 {
            transaction_blocks as f64 / elapsed_seconds
        } else {
            0.0
        },
        finalization_seconds: 0.0,
        finalization_commit_handlers_seconds: 0.0,
        finalization_store_commit_seconds: 0.0,
        unclassified_import_seconds: 0.0,
        hot_metrics: chain_acc::ImportHotMetrics::default(),
    }
}

fn serve_rpc_once(expected_method: &'static str, result: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test RPC");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let read = stream.read(&mut buf).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&request);
        assert!(
            text.contains(&format!(r#""method":"{expected_method}""#))
                || text.contains(&format!(r#""method": "{expected_method}""#)),
            "unexpected request: {text}"
        );
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": result,
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    });
    url
}

#[test]
fn fast_sync_preflight_allows_full_package_resume_on_existing_ledger() {
    let store = memory_store_with_ledger_tip(42);

    validate_fast_sync_preflight(&store, &test_package(0, 100))
        .expect("full fast-sync package can resume after an existing local tip");
}

#[test]
fn fast_sync_preflight_allows_full_package_already_imported() {
    let store = memory_store_with_ledger_tip(100);

    validate_fast_sync_preflight(&store, &test_package(0, 100))
        .expect("full fast-sync package can be a no-op when local tip is package end");
}

#[test]
fn fast_sync_preflight_rejects_full_package_behind_existing_ledger() {
    let store = memory_store_with_ledger_tip(101);

    let err = validate_fast_sync_preflight(&store, &test_package(0, 100))
        .expect_err("fast sync must not import a package behind the existing local ledger");

    assert!(
        err.to_string()
            .contains("local ledger is already at height 101"),
        "unexpected error: {err}"
    );
}

#[test]
fn fast_sync_preflight_allows_full_package_on_empty_or_genesis_ledger() {
    let empty = Arc::new(MemoryStore::new());
    validate_fast_sync_preflight(&empty, &test_package(0, 100))
        .expect("empty ledger can import a full fast-sync package");

    let genesis = memory_store_with_ledger_tip(0);
    validate_fast_sync_preflight(&genesis, &test_package(0, 100))
        .expect("genesis-only ledger can import a full fast-sync package");
}

#[test]
fn fast_sync_preflight_requires_previous_tip_for_partial_package() {
    let store = memory_store_with_ledger_tip(9);

    validate_fast_sync_preflight(&store, &test_package(10, 100))
        .expect("partial package can import when local tip is start - 1");

    let err = validate_fast_sync_preflight(&store, &test_package(11, 100))
        .expect_err("partial package must match the local pre-import tip");

    assert!(
        err.to_string().contains("expected tip 10"),
        "unexpected error: {err}"
    );
}

#[test]
fn stale_fast_sync_import_marker_blocks_retry() {
    let temp = tempfile::tempdir().expect("temp");
    let marker_path = temp.path().join(".neo-fast-sync-import-in-progress");
    std::fs::write(
        &marker_path,
        "network=n3mainnet\nstart=0\nend=100\npackage=chain.0.acc.zip\n",
    )
    .expect("stale marker");

    let err = refuse_stale_fast_sync_import_marker(temp.path())
        .expect_err("stale in-progress marker should block retry");

    assert!(
        err.to_string()
            .contains("previous fast-sync import did not finish cleanly"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains(&marker_path.display().to_string()),
        "operator error should identify the marker to inspect/remove: {err}"
    );
    assert!(
        err.to_string()
            .contains("restore a checkpoint or remove the local ledger"),
        "operator error should require storage recovery before retry: {err}"
    );
}

#[test]
fn fast_sync_import_marker_records_package_and_is_removed_on_success() {
    let temp = tempfile::tempdir().expect("temp");
    let package = test_package(0, 100);
    let chain_path = temp.path().join("chain.0.acc");
    let marker_path =
        write_fast_sync_import_marker(temp.path(), &package, &chain_path).expect("marker");

    let marker = std::fs::read_to_string(&marker_path).expect("read marker");
    assert!(marker.contains("network=n3mainnet"));
    assert!(marker.contains("start=0"));
    assert!(marker.contains("end=100"));
    assert!(marker.contains("package=chain.0.acc.zip"));
    assert!(marker.contains(&format!("chain={}", chain_path.display())));

    clear_fast_sync_import_marker(&marker_path).expect("clear marker");

    assert!(
        !marker_path.exists(),
        "successful fast-sync import should remove the in-progress marker"
    );
}

#[test]
fn fast_sync_throughput_status_classifies_target_window() {
    assert_eq!(
        fast_sync_throughput_status(&import_report(0, None, 0.0, 0.0)),
        FastSyncThroughputStatus::NoImport
    );
    assert_eq!(
        fast_sync_throughput_status(&import_report(10, None, 0.01, 50_000.0)),
        FastSyncThroughputStatus::NoTransactionProof
    );
    assert_eq!(
        fast_sync_throughput_status(&import_report_with_composition(
            10, None, 1.0, 10.0, 0, 10, 10,
        )),
        FastSyncThroughputStatus::BelowTarget
    );
    assert_eq!(
        fast_sync_throughput_status(&import_report_with_composition(
            1500, None, 1.0, 1500.0, 0, 1500, 1500,
        )),
        FastSyncThroughputStatus::MeetsFloor
    );
    assert_eq!(
        fast_sync_throughput_status(&import_report_with_composition(
            2000, None, 1.0, 2000.0, 0, 2000, 2000,
        )),
        FastSyncThroughputStatus::MeetsFloor
    );
    assert_eq!(
        fast_sync_throughput_status(&import_report_with_composition(
            2001, None, 1.0, 2001.0, 0, 2001, 2001,
        )),
        FastSyncThroughputStatus::MeetsFloor
    );
}

#[test]
fn empty_block_dominated_fast_sync_has_no_upper_bps_cap() {
    assert_eq!(
        fast_sync_throughput_status(&import_report(100_000, None, 2.0, 50_000.0)),
        FastSyncThroughputStatus::NoTransactionProof
    );

    let package = test_package(0, 99_999);
    let import_tip = chain_acc::LocalLedgerTip {
        height: 99_999,
        hash: neo_primitives::UInt256::from([99; 32]),
    };
    let report = FastSyncReport::from_parts(
        &package,
        Path::new("/cache/chain.0.acc.zip"),
        Path::new("/cache/chain.0.acc/chain.0.acc"),
        import_report(100_000, Some(import_tip), 2.0, 50_000.0),
        None,
    );

    assert_eq!(
        report.import.throughput_status,
        FastSyncThroughputStatus::NoTransactionProof
    );
    assert_eq!(report.import.empty_blocks_per_second, 50_000.0);
}

#[test]
fn fast_sync_report_preserves_package_and_import_proof() {
    let package = test_package(0, 100);
    let import_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([100; 32]),
    };
    let import = import_report(101, Some(import_tip), 0.0505, 2000.0);
    let report = FastSyncReport::from_parts(
        &package,
        Path::new("/cache/chain.0.acc.zip"),
        Path::new("/cache/chain.0.acc/chain.0.acc"),
        import_report_with_hot_metrics(
            import,
            chain_acc::ImportHotMetrics {
                state_service_mpt_apply_attempts: 101,
                state_service_mpt_apply_failures: 1,
                state_service_mpt_apply_height: 100,
                state_service_mpt_avg_total_us: 2_000,
                state_service_mpt_avg_project_us: 210,
                state_service_mpt_avg_trie_us: 2_100,
                state_service_mpt_avg_changes: 12,
                state_service_mpt_enqueue_blocking_avg_us: 99,
                state_service_mpt_queue_wait_avg_us: 111,
                state_service_mpt_mutate_changes_avg_us: 310,
                state_service_mpt_root_hash_avg_us: 410,
                state_service_mpt_trie_commit_avg_us: 1_200,
                state_service_mpt_backing_commit_avg_us: 1_300,
                state_service_mpt_publish_generation_avg_us: 140,
                state_service_mpt_overlay_entries_avg: 22,
                state_service_mpt_batch_blocks_avg: 7,
                native_persist_avg_total_us: 3_000,
                native_persist_tx_hot_stage: "application",
                native_persist_tx_hot_stage_avg_us: 1_700,
                rocksdb_batch_avg_flush_duration_ms: 11,
                rocksdb_batch_pending_operations: 19,
            },
        ),
        None,
    );

    assert_eq!(report.package.network, "n3mainnet");
    assert_eq!(report.package.start_height, 0);
    assert_eq!(report.package.end_height, 100);
    assert_eq!(report.package.filename, "chain.0.acc.zip");
    assert_eq!(report.package.md5, "ABCDEF0123456789ABCDEF0123456789");
    assert_eq!(report.package.zip_path, "/cache/chain.0.acc.zip");
    assert_eq!(report.package.chain_path, "/cache/chain.0.acc/chain.0.acc");
    assert_eq!(report.import.imported_blocks, 101);
    assert_eq!(report.import.final_height, Some(100));
    assert_eq!(report.import.elapsed_seconds, 0.0505);
    assert_eq!(report.import.driver_elapsed_seconds, 0.0505);
    assert_eq!(report.import.chain_acc_read_seconds, 0.0);
    assert_eq!(report.import.chain_acc_validate_seconds, 0.0);
    assert_eq!(report.import.average_blocks_per_second, 2000.0);
    assert_eq!(report.import.empty_blocks, 101);
    assert_eq!(report.import.empty_only_blocks, 101);
    assert_eq!(report.import.empty_block_import_seconds, 0.0505);
    assert_eq!(report.import.empty_blocks_per_second, 2000.0);
    assert_eq!(report.import.transaction_blocks, 0);
    assert_eq!(report.import.transactions, 0);
    assert_eq!(report.import.transaction_block_import_seconds, 0.0);
    assert_eq!(report.import.transaction_blocks_per_second, 0.0);
    assert_eq!(
        report.import.throughput_status,
        FastSyncThroughputStatus::NoTransactionProof
    );
    assert_eq!(report.hot_metrics.state_service_mpt_apply_attempts, 101);
    assert_eq!(report.hot_metrics.state_service_mpt_apply_failures, 1);
    assert_eq!(report.hot_metrics.state_service_mpt_apply_height, 100);
    assert_eq!(report.hot_metrics.state_service_mpt_avg_total_us, 2_000);
    assert_eq!(report.hot_metrics.state_service_mpt_avg_project_us, 210);
    assert_eq!(report.hot_metrics.state_service_mpt_avg_trie_us, 2_100);
    assert_eq!(report.hot_metrics.state_service_mpt_avg_changes, 12);
    assert_eq!(
        report.hot_metrics.state_service_mpt_enqueue_blocking_avg_us,
        99
    );
    assert_eq!(report.hot_metrics.state_service_mpt_queue_wait_avg_us, 111);
    assert_eq!(
        report.hot_metrics.state_service_mpt_mutate_changes_avg_us,
        310
    );
    assert_eq!(report.hot_metrics.state_service_mpt_root_hash_avg_us, 410);
    assert_eq!(
        report.hot_metrics.state_service_mpt_trie_commit_avg_us,
        1_200
    );
    assert_eq!(
        report.hot_metrics.state_service_mpt_backing_commit_avg_us,
        1_300
    );
    assert_eq!(
        report
            .hot_metrics
            .state_service_mpt_publish_generation_avg_us,
        140
    );
    assert_eq!(report.hot_metrics.state_service_mpt_overlay_entries_avg, 22);
    assert_eq!(report.hot_metrics.state_service_mpt_batch_blocks_avg, 7);
    assert_eq!(report.hot_metrics.native_persist_avg_total_us, 3_000);
    assert_eq!(
        report.hot_metrics.native_persist_tx_hot_stage,
        "application"
    );
    assert_eq!(report.hot_metrics.native_persist_tx_hot_stage_avg_us, 1_700);
    assert!(
        report
            .hot_metrics
            .native_persist_tx_stages
            .iter()
            .any(|stage| stage.stage == "load_execute"),
        "fast-sync hot metrics should preserve the native transaction stage series"
    );
    assert_eq!(report.hot_metrics.rocksdb_batch_avg_flush_duration_ms, 11);
    assert_eq!(report.hot_metrics.rocksdb_batch_pending_operations, 19);
}

#[test]
fn write_fast_sync_report_sidecar_serializes_machine_readable_proof() {
    let temp = tempfile::tempdir().expect("temp");
    let package = test_package(0, 100);
    let import_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([100; 32]),
    };
    let report = FastSyncReport::from_parts(
        &package,
        &temp.path().join("chain.0.acc.zip"),
        &temp.path().join("chain.0.acc/chain.0.acc"),
        import_report_with_hot_metrics(
            import_report(101, Some(import_tip), 0.0505, 2000.0),
            chain_acc::ImportHotMetrics {
                state_service_mpt_apply_attempts: 101,
                state_service_mpt_apply_failures: 1,
                state_service_mpt_apply_height: 100,
                state_service_mpt_avg_total_us: 2_000,
                state_service_mpt_avg_project_us: 210,
                state_service_mpt_avg_trie_us: 2_100,
                state_service_mpt_avg_changes: 12,
                state_service_mpt_enqueue_blocking_avg_us: 99,
                state_service_mpt_queue_wait_avg_us: 111,
                state_service_mpt_mutate_changes_avg_us: 310,
                state_service_mpt_root_hash_avg_us: 410,
                state_service_mpt_trie_commit_avg_us: 1_200,
                state_service_mpt_backing_commit_avg_us: 1_300,
                state_service_mpt_publish_generation_avg_us: 140,
                state_service_mpt_overlay_entries_avg: 22,
                state_service_mpt_batch_blocks_avg: 7,
                native_persist_avg_total_us: 3_000,
                native_persist_tx_hot_stage: "application",
                native_persist_tx_hot_stage_avg_us: 1_700,
                rocksdb_batch_avg_flush_duration_ms: 11,
                rocksdb_batch_pending_operations: 19,
            },
        ),
        None,
    );
    let path = temp.path().join("proof.json");

    write_fast_sync_report_sidecar(&path, &report).expect("write sidecar");

    let payload: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read sidecar")).expect("json");
    assert_eq!(payload["package"]["network"], "n3mainnet");
    assert_eq!(payload["package"]["start_height"], 0);
    assert_eq!(payload["package"]["end_height"], 100);
    assert_eq!(payload["import"]["imported_blocks"], 101);
    assert_eq!(payload["import"]["final_height"], 100);
    assert_eq!(payload["import"]["driver_elapsed_seconds"], 0.0505);
    assert_eq!(payload["import"]["chain_acc_read_seconds"], 0.0);
    assert_eq!(payload["import"]["chain_acc_validate_seconds"], 0.0);
    assert_eq!(payload["import"]["empty_blocks"], 101);
    assert_eq!(payload["import"]["empty_only_blocks"], 101);
    assert_eq!(payload["import"]["empty_block_import_seconds"], 0.0505);
    assert_eq!(payload["import"]["empty_blocks_per_second"], 2000.0);
    assert_eq!(payload["import"]["transaction_blocks"], 0);
    assert_eq!(payload["import"]["transactions"], 0);
    assert_eq!(payload["import"]["transaction_block_import_seconds"], 0.0);
    assert_eq!(payload["import"]["transaction_block_clone_seconds"], 0.0);
    assert_eq!(payload["import"]["transaction_ledger_insert_seconds"], 0.0);
    assert_eq!(
        payload["import"]["transaction_finalized_delivery_seconds"],
        0.0
    );
    assert_eq!(payload["import"]["transaction_blocks_per_second"], 0.0);
    assert_eq!(payload["import"]["finalization_seconds"], 0.0);
    assert_eq!(
        payload["import"]["finalization_commit_handlers_seconds"],
        0.0
    );
    assert_eq!(payload["import"]["finalization_store_commit_seconds"], 0.0);
    assert_eq!(payload["import"]["unclassified_import_seconds"], 0.0);
    assert_eq!(
        payload["import"]["throughput_status"],
        "no-transaction-proof"
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_avg_total_us"],
        2000
    );
    assert!(
        payload["hot_metrics"]["native_persist_tx_stages"]
            .as_array()
            .expect("native tx stage metrics")
            .iter()
            .any(|stage| stage["stage"] == "load_execute")
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_apply_attempts"],
        101
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_apply_failures"],
        1
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_apply_height"],
        100
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_avg_project_us"],
        210
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_avg_trie_us"],
        2100
    );
    assert_eq!(payload["hot_metrics"]["state_service_mpt_avg_changes"], 12);
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_enqueue_blocking_avg_us"],
        99
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_queue_wait_avg_us"],
        111
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_mutate_changes_avg_us"],
        310
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_root_hash_avg_us"],
        410
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_trie_commit_avg_us"],
        1200
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_backing_commit_avg_us"],
        1300
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_publish_generation_avg_us"],
        140
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_overlay_entries_avg"],
        22
    );
    assert_eq!(
        payload["hot_metrics"]["state_service_mpt_batch_blocks_avg"],
        7
    );
    assert_eq!(payload["hot_metrics"]["native_persist_avg_total_us"], 3000);
    assert_eq!(
        payload["hot_metrics"]["native_persist_tx_hot_stage"],
        "application"
    );
    assert_eq!(
        payload["hot_metrics"]["native_persist_tx_hot_stage_avg_us"],
        1700
    );
    assert_eq!(
        payload["hot_metrics"]["rocksdb_batch_avg_flush_duration_ms"],
        11
    );
    assert_eq!(
        payload["hot_metrics"]["rocksdb_batch_pending_operations"],
        19
    );
}

#[test]
fn fast_sync_report_preserves_transaction_bearing_throughput_proof() {
    let package = test_package(0, 100);
    let import_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([100; 32]),
    };
    let report = FastSyncReport::from_parts(
        &package,
        Path::new("/cache/chain.0.acc.zip"),
        Path::new("/cache/chain.0.acc/chain.0.acc"),
        import_report_with_composition(101, Some(import_tip), 0.25, 404.0, 81, 20, 45),
        None,
    );

    assert_eq!(report.import.imported_blocks, 101);
    assert_eq!(report.import.empty_blocks, 81);
    assert_eq!(report.import.empty_only_blocks, 0);
    assert_eq!(report.import.empty_block_import_seconds, 0.0);
    assert_eq!(report.import.empty_blocks_per_second, 0.0);
    assert_eq!(report.import.transaction_blocks, 20);
    assert_eq!(report.import.transactions, 45);
    assert_eq!(report.import.transaction_block_import_seconds, 0.25);
    assert_eq!(report.import.transaction_block_clone_seconds, 0.0);
    assert_eq!(report.import.transaction_ledger_insert_seconds, 0.0);
    assert_eq!(report.import.transaction_finalized_delivery_seconds, 0.0);
    assert_eq!(report.import.transaction_blocks_per_second, 80.0);
    assert_eq!(report.import.finalization_seconds, 0.0);
    assert_eq!(report.import.finalization_commit_handlers_seconds, 0.0);
    assert_eq!(report.import.finalization_store_commit_seconds, 0.0);
    assert_eq!(report.import.unclassified_import_seconds, 0.0);
    assert_eq!(
        report.import.throughput_status,
        FastSyncThroughputStatus::BelowTarget
    );
}

#[test]
fn fast_sync_report_serializes_reference_verification_provenance() {
    let temp = tempfile::tempdir().expect("temp");
    let package = test_package(0, 100);
    let import_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([100; 32]),
    };
    let report = FastSyncReport::from_parts(
        &package,
        &temp.path().join("chain.0.acc.zip"),
        &temp.path().join("chain.0.acc/chain.0.acc"),
        import_report(101, Some(import_tip), 0.0505, 2000.0),
        Some(FastSyncReferenceReport {
            endpoint: "https://seed1.neo.org:10332".to_string(),
            block_height: 100,
            block_hash: import_tip.hash.to_string(),
            state_root_height: Some(100),
            state_root_hash: Some(neo_primitives::UInt256::from([7; 32]).to_string()),
        }),
    );
    let path = temp.path().join("proof.json");

    write_fast_sync_report_sidecar(&path, &report).expect("write sidecar");

    let payload: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read sidecar")).expect("json");
    assert_eq!(
        payload["reference"]["endpoint"],
        "https://seed1.neo.org:10332"
    );
    assert_eq!(payload["reference"]["block_height"], 100);
    assert_eq!(
        payload["reference"]["block_hash"],
        import_tip.hash.to_string()
    );
    assert_eq!(payload["reference"]["state_root_height"], 100);
    assert_eq!(
        payload["reference"]["state_root_hash"],
        neo_primitives::UInt256::from([7; 32]).to_string()
    );
}

#[test]
fn fast_sync_post_import_tip_proof_accepts_matching_durable_tip() {
    let store = memory_store_with_ledger_tip(100);
    let package = test_package(0, 100);
    let report = import_report(
        101,
        chain_acc::local_ledger_tip(Some(&store)).expect("read tip"),
        1.0,
        101.0,
    );

    verify_fast_sync_import_tip(&store, &package, &report).expect("matching tip");
}

#[test]
fn fast_sync_post_import_tip_proof_rejects_mismatched_durable_tip() {
    let store = memory_store_with_ledger_tip(99);
    let package = test_package(0, 100);
    let imported_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([100; 32]),
    };
    let report = import_report(101, Some(imported_tip), 1.0, 101.0);

    let err = verify_fast_sync_import_tip(&store, &package, &report)
        .expect_err("mismatched durable tip must fail");

    assert!(
        err.to_string()
            .contains("fast-sync local ledger tip mismatch"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("expected imported tip height 100"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("local durable tip height 99"),
        "unexpected error: {err}"
    );
}

#[test]
fn fast_sync_post_import_state_root_proof_accepts_matching_local_root() {
    let (state_store, root_hash) = state_store_with_local_root(100);
    let imported_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([0xAB; 32]),
    };

    let proof = local_state_root_tip(Some(&state_store), &test_package(0, 100), imported_tip)
        .expect("local state root proof")
        .expect("state root enabled");

    assert_eq!(proof.index, 100);
    assert_eq!(proof.root_hash, root_hash);
}

#[test]
fn fast_sync_post_import_state_root_proof_rejects_missing_local_root() {
    let state_store = Arc::new(neo_state_service::StateStore::with_mpt(true));
    let imported_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([0xAB; 32]),
    };

    let err = local_state_root_tip(Some(&state_store), &test_package(0, 100), imported_tip)
        .expect_err("missing local state root must fail");

    assert!(
        err.to_string()
            .contains("StateService has no local state root"),
        "unexpected error: {err}"
    );
}

#[test]
fn fast_sync_post_import_state_root_proof_rejects_stale_local_root() {
    let (state_store, _root_hash) = state_store_with_local_root(99);
    let imported_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([0xAB; 32]),
    };

    let err = local_state_root_tip(Some(&state_store), &test_package(0, 100), imported_tip)
        .expect_err("stale local state root must fail");

    assert!(
        err.to_string().contains(
            "local state-root tip height 99 does not match imported block tip height 100"
        ),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn fast_sync_reference_block_tip_proof_accepts_matching_upstream_block() {
    let block = empty_block(100);
    let imported_tip = chain_acc::LocalLedgerTip {
        height: block.index(),
        hash: block.try_hash().expect("block hash"),
    };
    let endpoint = serve_rpc_once("getblock", serde_json::json!(serialized_hex(&block)));

    reference::verify_block_tip(&endpoint, &test_package(0, 100), imported_tip)
        .await
        .expect("matching upstream raw block");
}

#[tokio::test]
async fn fast_sync_reference_block_tip_proof_rejects_mismatched_upstream_block_hash() {
    let imported_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: neo_primitives::UInt256::from([0xAB; 32]),
    };
    let upstream_block = empty_block(100);
    let upstream_hash = upstream_block.try_hash().expect("block hash");
    let endpoint = serve_rpc_once(
        "getblock",
        serde_json::json!(serialized_hex(&upstream_block)),
    );

    let err = reference::verify_block_tip(&endpoint, &test_package(0, 100), imported_tip)
        .await
        .expect_err("mismatched upstream raw block hash must fail");

    assert!(
        err.to_string()
            .contains("fast-sync reference block hash mismatch"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("height 100"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains(&imported_tip.hash.to_string()),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains(&upstream_hash.to_string()),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn fast_sync_reference_block_tip_proof_rejects_wrong_upstream_block_height() {
    let upstream_block = empty_block(99);
    let imported_tip = chain_acc::LocalLedgerTip {
        height: 100,
        hash: upstream_block.try_hash().expect("block hash"),
    };
    let endpoint = serve_rpc_once(
        "getblock",
        serde_json::json!(serialized_hex(&upstream_block)),
    );

    let err = reference::verify_block_tip(&endpoint, &test_package(0, 100), imported_tip)
        .await
        .expect_err("wrong upstream block height must fail");

    assert!(
        err.to_string()
            .contains("fast-sync reference block height mismatch"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("imported height 100"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("upstream block height 99"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn fast_sync_reference_state_root_proof_accepts_matching_upstream_root() {
    let root_hash = neo_primitives::UInt256::from([0x44; 32]);
    let local_root = LocalStateRootTip {
        index: 100,
        root_hash,
    };
    let endpoint = serve_rpc_once(
        "getstateroot",
        serde_json::json!({
            "version": 0,
            "index": 100,
            "roothash": root_hash.to_string(),
        }),
    );

    reference::verify_state_root_tip(&endpoint, &test_package(0, 100), local_root)
        .await
        .expect("matching upstream state root");
}

#[tokio::test]
async fn fast_sync_reference_state_root_proof_rejects_mismatched_upstream_root() {
    let local_root = LocalStateRootTip {
        index: 100,
        root_hash: neo_primitives::UInt256::from([0x44; 32]),
    };
    let upstream_root = neo_primitives::UInt256::from([0x55; 32]);
    let endpoint = serve_rpc_once(
        "getstateroot",
        serde_json::json!({
            "version": 0,
            "index": 100,
            "roothash": upstream_root.to_string(),
        }),
    );

    let err = reference::verify_state_root_tip(&endpoint, &test_package(0, 100), local_root)
        .await
        .expect_err("mismatched upstream state root must fail");

    assert!(
        err.to_string()
            .contains("fast-sync reference state root mismatch"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains(&local_root.root_hash.to_string()),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains(&upstream_root.to_string()),
        "unexpected error: {err}"
    );
}
