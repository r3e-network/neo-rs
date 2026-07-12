use super::*;
use crate::{
    BlockBatchImportOutcome, BlockImport, BlockImportOutcome, BlockImportQueue, BlockOrigin,
    ImportedTip, Service, ServiceError,
};
use neo_payloads::{Block, Header, Witness};
use neo_primitives::{UInt160, UInt256};
use neo_storage::mdbx::MdbxStoreProvider;
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::persistence::read_only_store::RawReadOnlyStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::{
    IntoTableBytes, StoreMaintenanceBatch, TableEncode, TransactionalStore, WriteStore,
};
use neo_storage::rocksdb::RocksDBStoreProvider;
use parking_lot::Mutex;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn block(index: u32) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    Block::from_parts(header, vec![])
}

fn verified_header(index: u32, prev_hash: UInt256) -> Header {
    let mut header = Header::new();
    header.set_index(index);
    header.set_prev_hash(prev_hash);
    header.set_merkle_root(UInt256::from_bytes(&[index as u8; 32]).expect("merkle root"));
    header.set_timestamp(1_700_000_000_000 + u64::from(index));
    header.set_nonce(0x0102_0304_0506_0708 + u64::from(index));
    header.set_primary_index((index % 7) as u8);
    header.set_next_consensus(UInt160::from_bytes(&[index as u8; 20]).expect("next consensus"));
    header.witness = Witness::new();
    header
}

fn linked_headers(start_index: u32, count: u32, prev_hash: UInt256) -> Vec<Header> {
    let mut headers = Vec::with_capacity(count as usize);
    let mut previous_hash = prev_hash;
    for index in start_index..start_index + count {
        let header = verified_header(index, previous_hash);
        previous_hash = header.try_hash().expect("header hash");
        headers.push(header);
    }
    headers
}

#[test]
fn commit_policy_fires_on_first_reached_threshold() {
    let policy = CommitPolicy::new()
        .with_max_blocks(3)
        .with_max_changes(100)
        .with_max_cumulative_gas(1_000)
        .with_max_duration(Duration::from_secs(10));

    assert!(!policy.should_commit(StageProgress::blocks(2)));
    assert!(policy.should_commit(StageProgress::blocks(3)));
    assert!(policy.should_commit(StageProgress {
        blocks: 1,
        changes: 100,
        cumulative_gas: 0,
        elapsed: Duration::ZERO,
    }));
    assert!(policy.should_commit(StageProgress {
        blocks: 1,
        changes: 0,
        cumulative_gas: 1_000,
        elapsed: Duration::ZERO,
    }));
    assert!(policy.should_commit(StageProgress {
        blocks: 1,
        changes: 0,
        cumulative_gas: 0,
        elapsed: Duration::from_secs(10),
    }));
}

#[test]
fn in_memory_checkpoint_store_persists_stage_progress() {
    let store = InMemorySyncStageCheckpointStore::default();
    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Import, 42).with_counters(10, 1_024);

    store.put_checkpoint(checkpoint.clone()).expect("put");

    assert_eq!(
        store.checkpoint(SyncStageKind::Import).expect("get"),
        Some(checkpoint)
    );
    assert_eq!(SyncStageKind::Import.as_str(), "import");
}

#[test]
fn store_checkpoint_store_round_trips_and_overwrites_memory_backend() {
    let backing = MemoryStore::new();
    let store = StoreSyncStageCheckpointStore::new(backing.clone());
    let first = SyncStageCheckpoint::new(SyncStageKind::Import, 10).with_counters(10, 1_000);
    let second = SyncStageCheckpoint::new(SyncStageKind::Import, 12).with_counters(12, 2_000);

    assert_eq!(store.checkpoint(SyncStageKind::Import).expect("get"), None);

    store.put_checkpoint(first).expect("put first");
    store.put_checkpoint(second.clone()).expect("put second");

    assert_eq!(
        store.checkpoint(SyncStageKind::Import).expect("get"),
        Some(second)
    );
    assert_eq!(store.checkpoint(SyncStageKind::Bodies).expect("get"), None);
    assert_eq!(
        backing.try_get_bytes(&checkpoint_key(SyncStageKind::Import)),
        None,
        "sync metadata must not enter the normal Neo data table"
    );
    assert!(
        backing
            .maintenance_metadata(&checkpoint_key(SyncStageKind::Import))
            .expect("read isolated checkpoint metadata")
            .is_some()
    );
}

#[test]
fn store_checkpoint_reads_do_not_touch_normal_data_rows() {
    let mut backing = MemoryStore::new();
    let unrelated_key = vec![0xF8, b's', SyncStageKind::Import.code()];
    let unrelated_value = b"normal-data-row".to_vec();
    backing
        .put(unrelated_key.clone(), unrelated_value.clone())
        .expect("seed normal data row");
    let store = StoreSyncStageCheckpointStore::new(backing.clone());

    assert_eq!(store.checkpoint(SyncStageKind::Import).expect("get"), None);
    assert_eq!(
        backing.try_get_bytes(&unrelated_key),
        Some(unrelated_value),
        "typed maintenance reads must not inspect or mutate normal Neo data"
    );
}

#[test]
fn shared_store_checkpoint_store_round_trips_memory_backend() {
    let backing = Arc::new(MemoryStore::new());
    let store = SharedStoreSyncStageCheckpointStore::new(Arc::clone(&backing));
    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Import, 77).with_counters(70, 4_096);

    assert_eq!(store.checkpoint(SyncStageKind::Import).expect("get"), None);

    store
        .put_checkpoint(checkpoint.clone())
        .expect("put checkpoint through shared store");

    let fresh_view = SharedStoreSyncStageCheckpointStore::new(backing);
    assert_eq!(
        fresh_view.checkpoint(SyncStageKind::Import).expect("get"),
        Some(checkpoint)
    );
}

#[test]
fn in_memory_verified_header_store_preserves_fixed_window_and_target_hash() {
    let store = InMemoryVerifiedHeaderStore::default();
    let window = store.begin_window(10, 13).expect("begin window");

    assert_eq!(
        window,
        HeaderStageWindow {
            base_height: 10,
            target_height: 13,
            target_hash: None,
        }
    );
    assert_eq!(store.window().expect("window read"), Some(window.clone()));
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint"),
        Some(SyncStageCheckpoint::new(SyncStageKind::Headers, 10))
    );

    assert_eq!(
        store.begin_window(10, 13).expect("idempotent begin"),
        window
    );

    let err = store
        .begin_window(10, 14)
        .expect_err("different target must not replace the fixed active window");
    assert!(
        err.to_string().contains("active header stage window"),
        "unexpected error: {err}"
    );

    let prefix = linked_headers(
        11,
        2,
        UInt256::from_bytes(&[0x11; 32]).expect("anchor hash"),
    );
    let checkpoint = store
        .commit_verified_headers(&prefix)
        .expect("commit verified prefix");
    assert_eq!(checkpoint.height, 12);
    assert_eq!(checkpoint.processed_blocks, 2);
    assert!(checkpoint.changed_bytes > 0);
    assert_eq!(
        store.window().expect("window after partial commit"),
        Some(HeaderStageWindow {
            base_height: 10,
            target_height: 13,
            target_hash: None,
        })
    );
    assert_eq!(
        store
            .header(11)
            .expect("header read")
            .expect("staged header")
            .index(),
        11
    );

    let final_header = linked_headers(
        13,
        1,
        prefix
            .last()
            .expect("prefix tail")
            .try_hash()
            .expect("tail hash"),
    );
    let final_checkpoint = store
        .commit_verified_headers(&final_header)
        .expect("commit final verified header");
    let final_hash = final_header[0].try_hash().expect("final hash");
    assert_eq!(final_checkpoint.height, 13);
    assert_eq!(final_checkpoint.processed_blocks, 3);
    assert_eq!(
        store.window().expect("window after target"),
        Some(HeaderStageWindow {
            base_height: 10,
            target_height: 13,
            target_hash: Some(final_hash),
        })
    );
}

#[test]
fn verified_header_window_is_bounded_and_must_reach_target_before_finish() {
    let store = InMemoryVerifiedHeaderStore::default();
    let err = store
        .begin_window(10, 10 + MAX_VERIFIED_HEADER_WINDOW + 1)
        .expect_err("oversized window must be rejected before metadata allocation");
    assert!(err.to_string().contains("window exceeds"), "{err}");

    store.begin_window(10, 12).expect("begin bounded window");
    let err = store
        .finish_window(12)
        .expect_err("canonical height alone must not complete an unverified window");
    assert!(err.to_string().contains("incomplete"), "{err}");
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint"),
        Some(SyncStageCheckpoint::new(SyncStageKind::Headers, 10))
    );

    let discarded = store
        .discard_window(12)
        .expect("canonical reconciliation may discard an incomplete sidecar");
    assert_eq!(discarded.height, 12);
    assert_eq!(store.window().expect("window after discard"), None);
}

#[test]
fn verified_header_store_rejects_gaps_broken_links_and_target_overflow_without_advancing() {
    let store = InMemoryVerifiedHeaderStore::default();
    store.begin_window(20, 22).expect("begin window");

    let first = linked_headers(
        21,
        1,
        UInt256::from_bytes(&[0x21; 32]).expect("anchor hash"),
    );
    store
        .commit_verified_headers(&first)
        .expect("commit first verified header");
    let checkpoint = store
        .checkpoint(SyncStageKind::Headers)
        .expect("checkpoint")
        .expect("headers checkpoint");

    let gap = linked_headers(
        23,
        1,
        first
            .last()
            .expect("first header")
            .try_hash()
            .expect("first hash"),
    );
    let err = store
        .commit_verified_headers(&gap)
        .expect_err("commit must reject a checkpoint gap");
    assert!(err.to_string().contains("expected header index 22"));
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint"),
        Some(checkpoint.clone())
    );

    let mut broken_link =
        linked_headers(22, 1, UInt256::from_bytes(&[0x44; 32]).expect("wrong prev"));
    broken_link[0].set_prev_hash(UInt256::from_bytes(&[0x55; 32]).expect("broken link"));
    let err = store
        .commit_verified_headers(&broken_link)
        .expect_err("commit must reject a broken prev-hash link");
    assert!(err.to_string().contains("prev-hash"));
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint"),
        Some(checkpoint.clone())
    );

    let overflow = linked_headers(
        22,
        2,
        first
            .last()
            .expect("first header")
            .try_hash()
            .expect("first hash"),
    );
    let err = store
        .commit_verified_headers(&overflow)
        .expect_err("commit must reject headers beyond the fixed target");
    assert!(err.to_string().contains("target"));
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint"),
        Some(checkpoint)
    );
}

#[test]
fn store_verified_header_store_persists_across_memory_reopen_and_finishes_pruned_window() {
    let backing = MemoryStore::new();
    let store = StoreVerifiedHeaderStore::new(backing.clone());
    let headers = linked_headers(
        31,
        2,
        UInt256::from_bytes(&[0x31; 32]).expect("anchor hash"),
    );

    store.begin_window(30, 32).expect("begin window");
    store
        .commit_verified_headers(&headers)
        .expect("commit verified headers");

    let reopened = StoreVerifiedHeaderStore::new(backing.clone());
    let final_hash = headers
        .last()
        .expect("target header")
        .try_hash()
        .expect("target hash");
    assert_eq!(
        reopened.window().expect("window after reopen"),
        Some(HeaderStageWindow {
            base_height: 30,
            target_height: 32,
            target_hash: Some(final_hash),
        })
    );
    assert_eq!(
        reopened
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint after reopen"),
        Some(
            SyncStageCheckpoint::new(SyncStageKind::Headers, 32).with_counters(
                2,
                u64::try_from(
                    headers
                        .iter()
                        .map(|header| header.try_to_bytes().expect("serialize header").len())
                        .sum::<usize>()
                )
                .expect("changed byte count"),
            )
        )
    );
    assert_eq!(
        backing.try_get_bytes(&verified_header_key(31)),
        None,
        "verified headers must not enter the normal Neo data table"
    );
    assert!(
        backing
            .maintenance_metadata(&verified_header_key(31))
            .expect("maintenance metadata read")
            .is_some()
    );

    let err = reopened
        .finish_window(31)
        .expect_err("finish must reject canonical heights below the target");
    assert!(
        err.to_string().contains("canonical height"),
        "unexpected error: {err}"
    );

    let finished = reopened.finish_window(34).expect("finish window");
    assert_eq!(
        finished,
        SyncStageCheckpoint::new(SyncStageKind::Headers, 34).with_counters(
            2,
            u64::try_from(
                headers
                    .iter()
                    .map(|header| header.try_to_bytes().expect("serialize header").len())
                    .sum::<usize>()
            )
            .expect("changed byte count"),
        )
    );
    assert_eq!(reopened.window().expect("window after finish"), None);
    assert!(reopened.header(31).expect("read header").is_none());
    assert!(reopened.header(32).expect("read header").is_none());
}

#[test]
fn store_verified_header_store_reset_window_prunes_old_range() {
    let backing = MemoryStore::new();
    let store = StoreVerifiedHeaderStore::new(backing);
    let first = linked_headers(6, 1, UInt256::from_bytes(&[0x06; 32]).expect("anchor hash"));

    store.begin_window(5, 7).expect("begin window");
    store
        .commit_verified_headers(&first)
        .expect("commit first header");

    let reset = store.reset_window(8, 10).expect("reset window");
    assert_eq!(
        reset,
        HeaderStageWindow {
            base_height: 8,
            target_height: 10,
            target_hash: None,
        }
    );
    assert!(store.header(6).expect("header after reset").is_none());
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint"),
        Some(SyncStageCheckpoint::new(SyncStageKind::Headers, 8))
    );
}

#[test]
fn store_verified_header_window_can_reset_a_corrupt_checkpoint() {
    let backing = MemoryStore::new();
    let store = StoreVerifiedHeaderStore::new(backing.clone());
    store.begin_window(5, 7).expect("begin window");

    let mut corruption = StoreMaintenanceBatch::new();
    corruption.put_metadata(checkpoint_key(SyncStageKind::Headers), b"corrupt".to_vec());
    backing
        .commit_maintenance(&corruption)
        .expect("inject malformed checkpoint");

    assert_eq!(
        store
            .window()
            .expect("window remains independently readable"),
        Some(HeaderStageWindow {
            base_height: 5,
            target_height: 7,
            target_hash: None,
        })
    );
    assert!(
        store.checkpoint(SyncStageKind::Headers).is_err(),
        "the malformed checkpoint must still be reported to recovery"
    );

    let reset = store
        .reset_window(6, 8)
        .expect("reset replaces malformed checkpoint atomically");
    assert_eq!(reset.base_height, 6);
    assert_eq!(reset.target_height, 8);
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("checkpoint after reset"),
        Some(SyncStageCheckpoint::new(SyncStageKind::Headers, 6))
    );
}

#[test]
fn store_verified_header_discard_recovers_a_corrupt_checkpoint() {
    let backing = MemoryStore::new();
    let store = StoreVerifiedHeaderStore::new(backing.clone());
    store.begin_window(5, 7).expect("begin window");

    let mut corruption = StoreMaintenanceBatch::new();
    corruption.put_metadata(checkpoint_key(SyncStageKind::Headers), b"corrupt".to_vec());
    backing
        .commit_maintenance(&corruption)
        .expect("inject malformed checkpoint");

    let discarded = store
        .discard_window(6)
        .expect("canonical recovery discards malformed checkpoint");
    assert_eq!(
        discarded,
        SyncStageCheckpoint::new(SyncStageKind::Headers, 6)
    );
    assert_eq!(store.window().expect("window after discard"), None);
    assert_eq!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("replacement checkpoint"),
        Some(discarded)
    );
}

#[test]
fn store_checkpoint_store_persists_across_mdbx_reopen() {
    let path = unique_test_path("sync-checkpoint-mdbx");
    let provider = MdbxStoreProvider::new(StorageConfig::default()).with_map_size(64 * 1024 * 1024);
    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Import, 99).with_counters(99, 16_384);

    {
        let mdbx = provider.get_mdbx_store(&path).expect("open mdbx");
        let store = StoreSyncStageCheckpointStore::new(mdbx);
        store
            .put_checkpoint(checkpoint.clone())
            .expect("persist checkpoint");
        assert_eq!(
            store
                .store()
                .try_get_bytes(&checkpoint_key(SyncStageKind::Import)),
            None
        );
        assert!(
            store
                .store()
                .maintenance_metadata(&checkpoint_key(SyncStageKind::Import))
                .expect("read MDBX checkpoint metadata")
                .is_some()
        );
    }

    {
        let mdbx = provider.get_mdbx_store(&path).expect("reopen mdbx");
        let store = StoreSyncStageCheckpointStore::new(mdbx);
        assert_eq!(
            store.checkpoint(SyncStageKind::Import).expect("read"),
            Some(checkpoint)
        );
    }

    let _ = fs::remove_dir_all(path);
}

#[test]
fn store_checkpoint_store_persists_across_rocksdb_reopen() {
    let path = unique_test_path("sync-checkpoint-rocksdb");
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: path.clone(),
        ..Default::default()
    });
    let checkpoint =
        SyncStageCheckpoint::new(SyncStageKind::Import, 101).with_counters(100, 32_768);

    {
        let rocksdb = provider
            .get_rocksdb_store(std::path::Path::new(""))
            .expect("open RocksDB");
        let store = StoreSyncStageCheckpointStore::new(rocksdb);
        store
            .put_checkpoint(checkpoint.clone())
            .expect("persist checkpoint");
        assert_eq!(
            store
                .store()
                .try_get_bytes(&checkpoint_key(SyncStageKind::Import)),
            None
        );
    }

    {
        let rocksdb = provider
            .get_rocksdb_store(std::path::Path::new(""))
            .expect("reopen RocksDB");
        let store = StoreSyncStageCheckpointStore::new(rocksdb);
        assert_eq!(
            store.checkpoint(SyncStageKind::Import).expect("read"),
            Some(checkpoint)
        );
    }

    let _ = fs::remove_dir_all(path);
}

#[test]
fn store_checkpoint_payload_rejects_wrong_stage() {
    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Bodies, 7).with_counters(3, 4);
    let encoded = encode_checkpoint(&checkpoint);

    let err = decode_checkpoint(SyncStageKind::Import, &encoded)
        .expect_err("stage mismatch must be rejected");

    assert!(
        err.to_string().contains("stage mismatch"),
        "unexpected error: {err}"
    );
}

#[test]
fn sync_metadata_tables_preserve_v1_key_and_value_bytes() {
    use super::tables::{TargetHashKeyCodec, VerifiedWindowKeyCodec, VerifiedWindowValueCodec};

    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Import, 7).with_counters(3, 4);
    let mut expected_checkpoint_key = b"neo.sync.stage-checkpoint.v1.".to_vec();
    expected_checkpoint_key.push(SyncStageKind::Import.code());
    assert_eq!(
        checkpoint_key(SyncStageKind::Import),
        expected_checkpoint_key
    );

    let mut expected_checkpoint = b"NRSCP1".to_vec();
    expected_checkpoint.push(SyncStageKind::Import.code());
    expected_checkpoint.extend_from_slice(&7_u32.to_be_bytes());
    expected_checkpoint.extend_from_slice(&3_u64.to_be_bytes());
    expected_checkpoint.extend_from_slice(&4_u64.to_be_bytes());
    assert_eq!(encode_checkpoint(&checkpoint), expected_checkpoint);

    let mut expected_header_key = b"neo.sync.verified-header.v1.header.".to_vec();
    expected_header_key.extend_from_slice(&0x0102_0304_u32.to_be_bytes());
    assert_eq!(verified_header_key(0x0102_0304), expected_header_key);

    let window_key = VerifiedWindowKeyCodec::encode(&())
        .expect("encode window key")
        .into_table_bytes();
    assert_eq!(window_key, b"neo.sync.verified-header.v1.window");
    let target_key = TargetHashKeyCodec::encode(&())
        .expect("encode target key")
        .into_table_bytes();
    assert_eq!(target_key, b"neo.sync.verified-header.v1.target-hash");

    let window = HeaderStageWindow {
        base_height: 10,
        target_height: 20,
        target_hash: None,
    };
    let encoded_window = VerifiedWindowValueCodec::encode(&window)
        .expect("encode header window")
        .into_table_bytes();
    let mut expected_window = b"NRSHW1".to_vec();
    expected_window.extend_from_slice(&10_u32.to_be_bytes());
    expected_window.extend_from_slice(&20_u32.to_be_bytes());
    assert_eq!(encoded_window, expected_window);
}

#[derive(Debug, Default)]
struct RecordingImport {
    checked: Mutex<Vec<u32>>,
    fail_check_at: Option<u32>,
    processed_override: Option<usize>,
    imported_batches: AtomicUsize,
}

impl Service for RecordingImport {}

impl BlockImport for RecordingImport {
    async fn check(&self, block: &Block) -> Result<(), ServiceError> {
        if self.fail_check_at == Some(block.index()) {
            return Err(ServiceError::invalid_input(format!(
                "reject block {}",
                block.index()
            )));
        }
        self.checked.lock().push(block.index());
        Ok(())
    }

    async fn import(
        &self,
        block: Block,
        _origin: BlockOrigin,
    ) -> Result<BlockImportOutcome, ServiceError> {
        Ok(BlockImportOutcome::Imported(ImportedTip::from_block(
            &block,
        )?))
    }

    async fn import_many(
        &self,
        blocks: Vec<Block>,
        _origin: BlockOrigin,
    ) -> Result<BlockBatchImportOutcome, ServiceError> {
        self.imported_batches.fetch_add(1, Ordering::Relaxed);
        Ok(BlockBatchImportOutcome::new(
            self.processed_override.unwrap_or(blocks.len()),
        ))
    }
}

#[tokio::test]
async fn sync_pipeline_driver_imports_contiguous_batches_and_checkpoints() {
    let importer = Arc::new(RecordingImport::default());
    let queue = Arc::new(BlockImportQueue::new(importer.clone(), 2));
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    let mut driver = SyncPipelineDriver::new(
        queue.clone(),
        checkpoints.clone(),
        CommitPolicy::new().with_max_blocks(2),
        BlockOrigin::Sync,
    )
    .expect("driver");

    let outcome = driver
        .push_batch(SyncBlockBatch::new(1, vec![block(1), block(2)]))
        .await
        .expect("import batch");

    assert_eq!(outcome.imported.processed, 2);
    assert_eq!(outcome.next_height, Some(3));
    assert_eq!(
        outcome.checkpoint,
        Some(SyncStageCheckpoint::new(SyncStageKind::Import, 2).with_counters(2, 0))
    );
    assert_eq!(importer.imported_batches.load(Ordering::Relaxed), 1);
    assert_eq!(
        checkpoints
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint"),
        outcome.checkpoint
    );
}

#[tokio::test]
async fn sync_pipeline_driver_resumes_from_store_checkpoint() {
    let checkpoints = Arc::new(StoreSyncStageCheckpointStore::new(MemoryStore::new()));
    checkpoints
        .put_checkpoint(SyncStageCheckpoint::new(SyncStageKind::Import, 42).with_counters(40, 512))
        .expect("seed checkpoint");
    let importer = Arc::new(RecordingImport::default());
    let queue = Arc::new(BlockImportQueue::new(importer, 2));
    let mut driver = SyncPipelineDriver::new(
        queue,
        checkpoints,
        CommitPolicy::new().with_max_blocks(1),
        BlockOrigin::Sync,
    )
    .expect("driver");

    let outcome = driver
        .push_batch(SyncBlockBatch::new(43, vec![block(43)]))
        .await
        .expect("resume batch");

    assert_eq!(outcome.next_height, Some(44));
    assert_eq!(
        outcome.checkpoint,
        Some(SyncStageCheckpoint::new(SyncStageKind::Import, 43).with_counters(1, 0))
    );
}

#[tokio::test]
async fn sync_pipeline_driver_can_align_forward_to_live_chain_tip() {
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    checkpoints
        .put_checkpoint(SyncStageCheckpoint::new(SyncStageKind::Import, 2).with_counters(2, 0))
        .expect("seed checkpoint");
    let importer = Arc::new(RecordingImport::default());
    let queue = Arc::new(BlockImportQueue::new(importer, 2));
    let mut driver = SyncPipelineDriver::new(
        queue,
        checkpoints,
        CommitPolicy::new().with_max_blocks(1),
        BlockOrigin::Sync,
    )
    .expect("driver");

    driver.align_next_height_to_chain_tip(5);

    let err = driver
        .push_batch(SyncBlockBatch::new(3, vec![block(3)]))
        .await
        .expect_err("alignment must not move the cursor backward to stale checkpoint height");
    assert!(
        err.to_string().contains("expected height 6"),
        "unexpected error: {err}"
    );

    let outcome = driver
        .push_batch(SyncBlockBatch::new(6, vec![block(6)]))
        .await
        .expect("aligned live-height batch");

    assert_eq!(outcome.next_height, Some(7));
    assert_eq!(
        outcome.checkpoint,
        Some(SyncStageCheckpoint::new(SyncStageKind::Import, 6).with_counters(1, 0))
    );
}

#[tokio::test]
async fn sync_pipeline_driver_rejects_height_gaps() {
    let importer = Arc::new(RecordingImport::default());
    let queue = Arc::new(BlockImportQueue::new(importer, 2));
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    let mut driver = SyncPipelineDriver::new(
        queue,
        checkpoints,
        CommitPolicy::new().with_max_blocks(2),
        BlockOrigin::Sync,
    )
    .expect("driver");

    driver
        .push_batch(SyncBlockBatch::new(1, vec![block(1)]))
        .await
        .expect("first batch");
    let err = driver
        .push_batch(SyncBlockBatch::new(3, vec![block(3)]))
        .await
        .expect_err("gap must be rejected");

    assert!(
        err.to_string().contains("non-contiguous sync batch"),
        "{err}"
    );
}

#[tokio::test]
async fn sync_pipeline_driver_uses_import_queue_preflight_before_import() {
    let importer = Arc::new(RecordingImport {
        fail_check_at: Some(2),
        ..RecordingImport::default()
    });
    let queue = Arc::new(BlockImportQueue::new(importer.clone(), 2));
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    let mut driver = SyncPipelineDriver::new(
        queue,
        checkpoints,
        CommitPolicy::new().with_max_blocks(2),
        BlockOrigin::Sync,
    )
    .expect("driver");

    let err = driver
        .push_batch(SyncBlockBatch::new(1, vec![block(1), block(2)]))
        .await
        .expect_err("preflight failure");

    assert!(err.to_string().contains("reject block 2"), "{err}");
    assert_eq!(importer.imported_batches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn sync_pipeline_driver_rejects_partial_imported_batches_without_advancing() {
    let importer = Arc::new(RecordingImport {
        processed_override: Some(1),
        ..RecordingImport::default()
    });
    let queue = Arc::new(BlockImportQueue::new(importer.clone(), 2));
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::default());
    let mut driver = SyncPipelineDriver::new(
        queue,
        Arc::clone(&checkpoints),
        CommitPolicy::new().with_max_blocks(1),
        BlockOrigin::Sync,
    )
    .expect("driver");

    let err = driver
        .push_batch(SyncBlockBatch::new(1, vec![block(1), block(2)]))
        .await
        .expect_err("partial import must not advance sync cursor");

    assert!(err.to_string().contains("processed 1 of 2 blocks"), "{err}");
    assert_eq!(importer.imported_batches.load(Ordering::Relaxed), 1);
    assert_eq!(
        checkpoints
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint read after partial import"),
        None,
        "partial batch progress must not advance the durable import checkpoint"
    );

    let err = driver
        .push_batch(SyncBlockBatch::new(3, vec![block(3)]))
        .await
        .expect_err("cursor should not advance after partial import");
    assert!(
        err.to_string().contains("non-contiguous sync batch"),
        "{err}"
    );
}

fn unique_test_path(name: &str) -> PathBuf {
    let unique = format!(
        "neo-runtime-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}
