use std::time::Duration;

use super::*;
use crate::{
    BlockBatchImportOutcome, BlockOrigin, ImportQueue, Service, ServiceError, ServiceResult,
};
use async_trait::async_trait;
use neo_payloads::Block;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct RecordingImportQueue {
    imported: Mutex<Vec<u32>>,
}

impl RecordingImportQueue {
    fn imported(&self) -> Vec<u32> {
        self.imported.lock().expect("imported lock").clone()
    }
}

impl Service for RecordingImportQueue {}

#[async_trait]
impl ImportQueue for RecordingImportQueue {
    async fn push_blocks(
        &self,
        blocks: Vec<Block>,
        _origin: BlockOrigin,
    ) -> ServiceResult<BlockBatchImportOutcome> {
        self.imported
            .lock()
            .expect("imported lock")
            .extend(blocks.iter().map(Block::index));
        Ok(BlockBatchImportOutcome::new(blocks.len()))
    }
}

fn block(index: u32) -> Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    Block::from_parts(header, vec![])
}

#[test]
fn commit_policy_uses_first_reached_threshold() {
    let policy = CommitPolicy::default()
        .with_max_blocks(10)
        .with_max_changes(100)
        .with_max_cumulative_gas(1_000)
        .with_max_duration(Duration::from_secs(5));

    assert_eq!(
        policy.evaluate(StageProgress {
            blocks: 10,
            changes: 100,
            cumulative_gas: 1_000,
            elapsed: Duration::from_secs(5),
        }),
        CommitDecision::commit(CommitTrigger::Blocks)
    );

    assert_eq!(
        policy.evaluate(StageProgress {
            blocks: 9,
            changes: 100,
            cumulative_gas: 1_000,
            elapsed: Duration::from_secs(5),
        }),
        CommitDecision::commit(CommitTrigger::Changes)
    );
}

#[test]
fn commit_policy_zero_thresholds_are_disabled() {
    let policy = CommitPolicy::default()
        .with_max_blocks(0)
        .with_max_changes(0)
        .with_max_cumulative_gas(0)
        .with_max_duration(Duration::ZERO);

    assert_eq!(
        policy.evaluate(StageProgress {
            blocks: u64::MAX,
            changes: u64::MAX,
            cumulative_gas: u64::MAX,
            elapsed: Duration::from_secs(u64::MAX),
        }),
        CommitDecision::continue_stage()
    );
}

#[test]
fn per_block_policy_commits_after_one_block() {
    let policy = CommitPolicy::per_block();

    assert_eq!(
        policy.evaluate(StageProgress::blocks(0)),
        CommitDecision::continue_stage()
    );
    assert_eq!(
        policy.evaluate(StageProgress::blocks(1)),
        CommitDecision::commit(CommitTrigger::Blocks)
    );
}

#[test]
fn in_memory_checkpoint_store_replaces_stage_checkpoint() {
    let store = InMemorySyncStageCheckpointStore::new();
    assert!(
        store
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint lookup")
            .is_none()
    );

    store
        .save_checkpoint(SyncStageCheckpoint::new(SyncStageKind::Import, 10).with_counters(10, 512))
        .expect("save first checkpoint");
    store
        .save_checkpoint(
            SyncStageCheckpoint::new(SyncStageKind::Import, 42).with_counters(42, 2048),
        )
        .expect("save replacement checkpoint");

    assert_eq!(
        store
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint lookup"),
        Some(SyncStageCheckpoint {
            stage: SyncStageKind::Import,
            height: 42,
            processed_blocks: 42,
            changed_bytes: 2048,
        })
    );
    assert!(
        store
            .checkpoint(SyncStageKind::Headers)
            .expect("headers lookup")
            .is_none()
    );
}

#[test]
fn stage_names_are_stable_metric_labels() {
    assert_eq!(SyncStageKind::Headers.as_str(), "headers");
    assert_eq!(SyncStageKind::Bodies.as_str(), "bodies");
    assert_eq!(SyncStageKind::Preverify.as_str(), "preverify");
    assert_eq!(SyncStageKind::Import.as_str(), "import");
    assert_eq!(SyncStageKind::Execute.as_str(), "execute");
    assert_eq!(SyncStageKind::StateRoot.as_str(), "state_root");
    assert_eq!(SyncStageKind::Index.as_str(), "index");
    assert_eq!(SyncStageKind::Prune.as_str(), "prune");
}

#[tokio::test]
async fn sync_pipeline_driver_imports_contiguous_batches_and_checkpoints_by_policy() {
    let queue = Arc::new(RecordingImportQueue::default());
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::new());
    let mut driver = SyncPipelineDriver::new(
        queue.clone(),
        checkpoints.clone(),
        CommitPolicy::default().with_max_blocks(3),
        BlockOrigin::Sync,
    )
    .expect("driver");

    let first = driver
        .import_batch(SyncBlockBatch::new(1, vec![block(1), block(2)]))
        .await
        .expect("first batch");
    assert_eq!(first.imported.processed, 2);
    assert_eq!(first.next_height, Some(3));
    assert!(first.checkpoint.is_none());
    assert!(
        checkpoints
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint lookup")
            .is_none()
    );

    let second = driver
        .import_batch(SyncBlockBatch::new(3, vec![block(3)]))
        .await
        .expect("second batch");

    assert_eq!(queue.imported(), vec![1, 2, 3]);
    assert_eq!(second.next_height, Some(4));
    assert_eq!(
        second.checkpoint,
        Some(SyncStageCheckpoint::new(SyncStageKind::Import, 3).with_counters(3, 0))
    );
    assert_eq!(
        checkpoints
            .checkpoint(SyncStageKind::Import)
            .expect("checkpoint lookup"),
        second.checkpoint
    );
}

#[tokio::test]
async fn sync_pipeline_driver_checkpoints_explicit_changed_bytes() {
    let queue = Arc::new(RecordingImportQueue::default());
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::new());
    let mut driver = SyncPipelineDriver::new(
        queue,
        checkpoints,
        CommitPolicy::default().with_max_changes(64),
        BlockOrigin::Sync,
    )
    .expect("driver");

    let outcome = driver
        .import_batch(SyncBlockBatch::new(1, vec![block(1)]).with_changed_bytes(64))
        .await
        .expect("batch");

    assert_eq!(
        outcome.checkpoint,
        Some(SyncStageCheckpoint::new(SyncStageKind::Import, 1).with_counters(1, 64))
    );
}

#[tokio::test]
async fn sync_pipeline_driver_rejects_height_gaps_after_first_batch() {
    let queue = Arc::new(RecordingImportQueue::default());
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::new());
    let mut driver = SyncPipelineDriver::new(
        queue,
        checkpoints,
        CommitPolicy::default().with_max_blocks(10),
        BlockOrigin::Sync,
    )
    .expect("driver");

    driver
        .import_batch(SyncBlockBatch::new(10, vec![block(10)]))
        .await
        .expect("first batch");
    let error = driver
        .import_batch(SyncBlockBatch::new(12, vec![block(12)]))
        .await
        .expect_err("gap should be rejected");

    assert!(matches!(error, ServiceError::InvalidState(_)));
    assert!(error.to_string().contains("expected block height 11"));
}

#[tokio::test]
async fn sync_pipeline_driver_resumes_expected_height_from_checkpoint() {
    let queue = Arc::new(RecordingImportQueue::default());
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::new());
    checkpoints
        .save_checkpoint(SyncStageCheckpoint::new(SyncStageKind::Import, 10))
        .expect("save checkpoint");
    let mut driver = SyncPipelineDriver::new(
        queue,
        checkpoints,
        CommitPolicy::default().with_max_blocks(10),
        BlockOrigin::Sync,
    )
    .expect("driver");

    let error = driver
        .import_batch(SyncBlockBatch::new(12, vec![block(12)]))
        .await
        .expect_err("checkpoint resume gap should be rejected");

    assert!(matches!(error, ServiceError::InvalidState(_)));
    assert!(error.to_string().contains("expected block height 11"));
}

#[tokio::test]
async fn sync_pipeline_driver_rejects_block_index_mismatch_inside_batch() {
    let queue = Arc::new(RecordingImportQueue::default());
    let checkpoints = Arc::new(InMemorySyncStageCheckpointStore::new());
    let mut driver = SyncPipelineDriver::new(
        queue,
        checkpoints,
        CommitPolicy::default().with_max_blocks(10),
        BlockOrigin::Sync,
    )
    .expect("driver");

    let error = driver
        .import_batch(SyncBlockBatch::new(5, vec![block(5), block(7)]))
        .await
        .expect_err("index mismatch should be rejected");

    assert!(matches!(error, ServiceError::InvalidInput(_)));
    assert!(error.to_string().contains("batch declares height 6"));
}
