use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use neo_runtime::{ServiceResult, SyncStageKind};

use super::*;

#[derive(Clone)]
struct FakeCanonicalIndexSource {
    height: u32,
    blocks: Arc<BTreeMap<u32, Block>>,
}

impl FakeCanonicalIndexSource {
    fn contiguous(height: u32) -> Self {
        let mut blocks = BTreeMap::new();
        let mut previous_hash = UInt256::zero();
        for block_height in 0..=height {
            let mut block = test_block(block_height, block_height.saturating_add(1));
            block.header.set_prev_hash(previous_hash);
            previous_hash = block.try_hash().expect("linked block hash");
            blocks.insert(block_height, block);
        }
        Self {
            height,
            blocks: Arc::new(blocks),
        }
    }

    fn without(mut self, height: u32) -> Self {
        Arc::make_mut(&mut self.blocks).remove(&height);
        self
    }
}

#[derive(Clone)]
struct MovingTargetSource {
    initial: FakeCanonicalIndexSource,
    replacement: Block,
    target_reads: Arc<AtomicUsize>,
}

impl CanonicalIndexSource for MovingTargetSource {
    fn chain_height(&self) -> impl Future<Output = ServiceResult<u32>> + Send {
        std::future::ready(Ok(self.initial.height))
    }

    fn block_by_height(
        &self,
        height: u32,
    ) -> impl Future<Output = ServiceResult<Option<Block>>> + Send {
        let block = if height == self.initial.height
            && self.target_reads.fetch_add(1, Ordering::SeqCst) > 0
        {
            Some(self.replacement.clone())
        } else {
            self.initial.blocks.get(&height).cloned()
        };
        std::future::ready(Ok(block))
    }
}

impl CanonicalIndexSource for FakeCanonicalIndexSource {
    fn chain_height(&self) -> impl Future<Output = ServiceResult<u32>> + Send {
        std::future::ready(Ok(self.height))
    }

    fn block_by_height(
        &self,
        height: u32,
    ) -> impl Future<Output = ServiceResult<Option<Block>>> + Send {
        std::future::ready(Ok(self.blocks.get(&height).cloned()))
    }
}

#[tokio::test]
async fn index_stage_processes_canonical_blocks_in_bounded_batches() {
    let indexer = Arc::new(IndexerService::new());
    let stage = IndexStage::new(
        FakeCanonicalIndexSource::contiguous(4),
        Arc::clone(&indexer),
        |_| Vec::new(),
    )
    .with_batch_size(2);

    let outcome = stage.execute_to_tip().await.expect("execute index stage");

    assert_eq!(outcome.stage, SyncStageKind::Index);
    assert_eq!(outcome.start_height, Some(0));
    assert_eq!(outcome.target_height, 4);
    assert_eq!(outcome.processed_blocks, 5);
    assert_eq!(outcome.committed_batches, 3);
    assert!(outcome.checkpoint.is_synced_with(Some(4)));
    assert_eq!(outcome.checkpoint.indexed_blocks, 5);
}

#[tokio::test]
async fn index_stage_resumes_after_verified_contiguous_status() {
    let source = FakeCanonicalIndexSource::contiguous(4);
    let indexer = Arc::new(IndexerService::new());
    indexer
        .index_block(source.blocks.get(&0).expect("genesis"))
        .expect("index genesis");
    indexer
        .index_block(source.blocks.get(&1).expect("block one"))
        .expect("index block one");
    let stage = IndexStage::new(source, Arc::clone(&indexer), |_| Vec::new()).with_batch_size(2);

    let outcome = stage.execute_to_tip().await.expect("resume index stage");

    assert_eq!(outcome.start_height, Some(2));
    assert_eq!(outcome.processed_blocks, 3);
    assert_eq!(outcome.committed_batches, 2);
    assert!(outcome.checkpoint.is_synced_with(Some(4)));
}

#[tokio::test]
async fn index_stage_stops_at_gap_and_preserves_durable_prefix() {
    let indexer = Arc::new(IndexerService::new());
    let stage = IndexStage::new(
        FakeCanonicalIndexSource::contiguous(4).without(2),
        Arc::clone(&indexer),
        |_| Vec::new(),
    )
    .with_batch_size(2);

    let error = stage
        .execute_to_tip()
        .await
        .expect_err("missing canonical block must stop stage");

    assert!(matches!(
        error,
        IndexStageError::MissingCanonicalBlock { height: 2 }
    ));
    assert_eq!(indexer.status().indexed_height, Some(1));
    assert_eq!(indexer.status().indexed_blocks, 2);
    assert!(indexer.block_by_height(3).is_none());
}

#[tokio::test]
async fn index_stage_rejects_blocks_that_do_not_extend_the_batch_prefix() {
    let mut source = FakeCanonicalIndexSource::contiguous(4);
    Arc::make_mut(&mut source.blocks)
        .get_mut(&2)
        .expect("block two")
        .header
        .set_prev_hash(UInt256::from([0xA5; UInt256::LENGTH]));
    let indexer = Arc::new(IndexerService::new());
    let stage = IndexStage::new(source, Arc::clone(&indexer), |_| Vec::new()).with_batch_size(2);

    let error = stage
        .execute_to_tip()
        .await
        .expect_err("a mixed canonical view must not be published as successful");

    assert!(matches!(
        error,
        IndexStageError::ParentHashMismatch { height: 2, .. }
    ));
    assert_eq!(indexer.status().indexed_height, Some(1));
    assert_eq!(indexer.status().indexed_blocks, 2);
}

#[tokio::test]
async fn index_stage_rechecks_the_fixed_target_before_reporting_success() {
    let initial = FakeCanonicalIndexSource::contiguous(2);
    let parent_hash = initial
        .blocks
        .get(&1)
        .expect("block one")
        .try_hash()
        .expect("block one hash");
    let mut replacement = test_block(2, 9_999);
    replacement.header.set_prev_hash(parent_hash);
    let source = MovingTargetSource {
        initial,
        replacement,
        target_reads: Arc::new(AtomicUsize::new(0)),
    };
    let indexer = Arc::new(IndexerService::new());
    let stage = IndexStage::new(source, Arc::clone(&indexer), |_| Vec::new()).with_batch_size(2);

    let error = stage
        .execute_to_tip()
        .await
        .expect_err("a moving fixed target must abort the stage run");

    assert!(matches!(
        error,
        IndexStageError::CanonicalTargetMoved { height: 2, .. }
    ));
    assert_eq!(
        indexer.status().indexed_height,
        None,
        "a target that moved during the run must not leave a purported canonical checkpoint"
    );
}

#[tokio::test]
async fn invalid_checkpoint_is_cleared_before_rebuild_batches_become_durable() {
    let indexer = Arc::new(IndexerService::new());
    for height in 0..=4 {
        indexer
            .index_block(&test_block(height, height.saturating_add(100)))
            .expect("index stale block");
    }

    let canonical_source = FakeCanonicalIndexSource::contiguous(4).without(2);
    let stage =
        IndexStage::new(canonical_source, Arc::clone(&indexer), |_| Vec::new()).with_batch_size(2);

    let error = stage
        .execute_to_tip()
        .await
        .expect_err("canonical gap must stop the checkpoint rebuild");

    assert!(matches!(
        error,
        IndexStageError::MissingCanonicalBlock { height: 2 }
    ));
    assert_eq!(
        indexer.status().indexed_height,
        Some(1),
        "rows beyond the rebuilt durable prefix must not survive checkpoint reset"
    );
    assert_eq!(indexer.status().indexed_blocks, 2);
    assert!(indexer.block_by_height(2).is_none());
    assert!(indexer.block_by_height(4).is_none());
}

#[tokio::test]
async fn index_stage_rebuilds_a_hash_mismatched_checkpoint_from_genesis() {
    let indexer = Arc::new(IndexerService::new());
    for height in 0..=4 {
        indexer
            .index_block(&test_block(height, height.saturating_add(100)))
            .expect("index stale block");
    }
    let stale_tip_hash = indexer.block_by_height(4).expect("stale tip").hash;
    let source = FakeCanonicalIndexSource::contiguous(4);
    let canonical_tip_hash = source
        .blocks
        .get(&4)
        .expect("canonical tip")
        .try_hash()
        .expect("canonical tip hash");
    let stage = IndexStage::new(source, Arc::clone(&indexer), |_| Vec::new()).with_batch_size(2);

    let outcome = stage.execute_to_tip().await.expect("rebuild index stage");

    assert_eq!(outcome.start_height, Some(0));
    assert_eq!(outcome.processed_blocks, 5);
    assert_eq!(outcome.committed_batches, 3);
    assert_eq!(outcome.checkpoint.indexed_hash, Some(canonical_tip_hash));
    assert!(indexer.block_by_hash(&stale_tip_hash).is_none());
}

#[tokio::test]
async fn index_stage_prunes_an_ahead_checkpoint_before_resuming() {
    let full_source = FakeCanonicalIndexSource::contiguous(4);
    let indexer = Arc::new(IndexerService::new());
    for block in full_source.blocks.values() {
        indexer.index_block(block).expect("index canonical block");
    }
    let ahead_tip_hash = full_source
        .blocks
        .get(&4)
        .expect("ahead tip")
        .try_hash()
        .expect("ahead tip hash");
    let source = FakeCanonicalIndexSource {
        height: 2,
        blocks: Arc::clone(&full_source.blocks),
    };
    let stage = IndexStage::new(source, Arc::clone(&indexer), |_| Vec::new());

    let outcome = stage.execute_to_tip().await.expect("reconcile ahead index");

    assert_eq!(outcome.processed_blocks, 0);
    assert_eq!(outcome.checkpoint.indexed_height, Some(2));
    assert_eq!(outcome.checkpoint.indexed_blocks, 3);
    assert!(indexer.block_by_hash(&ahead_tip_hash).is_none());
    assert!(indexer.block_by_height(3).is_none());
}

#[tokio::test]
async fn verified_checkpoint_preserves_precommit_notifications() {
    let source = FakeCanonicalIndexSource::contiguous(0);
    let block = source.blocks.get(&0).expect("genesis");
    let block_hash = block.try_hash().expect("genesis hash");
    let notification = NotificationIndexRecord {
        block_hash,
        block_height: 0,
        tx_hash: None,
        execution_index: 0,
        notification_index: 0,
        contract_hash: UInt160::from_bytes(&[7; UInt160::LENGTH]).expect("contract"),
        event_name: "Transfer".to_string(),
        trigger: "Application".to_string(),
        state_item_count: 0,
        state: Vec::new(),
        accounts: Vec::new(),
    };
    let indexer = Arc::new(IndexerService::new());
    indexer
        .index_block_with_notification_records(block, vec![notification])
        .expect("index live notification");
    let notification_reads = Arc::new(AtomicUsize::new(0));
    let reads = Arc::clone(&notification_reads);
    let stage = IndexStage::new(source, Arc::clone(&indexer), move |_| {
        reads.fetch_add(1, Ordering::Relaxed);
        Vec::new()
    });

    let outcome = stage.execute_to_tip().await.expect("verify checkpoint");

    assert_eq!(outcome.processed_blocks, 0);
    assert_eq!(notification_reads.load(Ordering::Relaxed), 0);
    assert_eq!(indexer.notifications_for_block(&block_hash, 0, 10).len(), 1);
}
