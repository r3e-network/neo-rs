use super::*;
use crate::{
    BlockBatchImportOutcome, BlockImport, BlockImportOutcome, BlockImportQueue, BlockOrigin,
    ImportQueue, ImportedTip, Service, ServiceError,
};
use async_trait::async_trait;
use neo_payloads::{Block, Header};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn block(index: u32) -> Block {
    let mut header = Header::new();
    header.set_index(index);
    Block::from_parts(header, vec![])
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

#[derive(Debug, Default)]
struct RecordingImport {
    checked: Mutex<Vec<u32>>,
    fail_check_at: Option<u32>,
    imported_batches: AtomicUsize,
}

impl Service for RecordingImport {}

#[async_trait]
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
        Ok(BlockBatchImportOutcome::new(blocks.len()))
    }
}

fn _import_queue_trait_object(_: &dyn ImportQueue) {}

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
    _import_queue_trait_object(queue.as_ref());

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
