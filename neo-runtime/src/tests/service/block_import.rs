use super::*;
use crate::{BlockImport, BlockOrigin, Service};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn block(index: u32) -> Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    Block::from_parts(header, vec![])
}

#[test]
fn imported_tip_uses_block_identity() {
    let mut header = neo_payloads::Header::new();
    header.set_index(42);
    header.set_timestamp(1_700_000);
    let block = Block::from_parts(header, vec![]);

    let tip = ImportedTip::from_block(&block).expect("tip");

    assert_eq!(tip.hash, block.try_hash().expect("hash"));
    assert_eq!(tip.height, 42);
    assert_eq!(tip.timestamp, 1_700_000);
}

#[test]
fn batch_outcome_preserves_processed_count() {
    assert_eq!(BlockBatchImportOutcome::new(3).processed, 3);
}

#[derive(Debug, Default)]
struct QueueRecordingImport {
    checked: Mutex<Vec<u32>>,
    imported: Mutex<Vec<u32>>,
    fail_checks: Vec<u32>,
    import_calls: AtomicUsize,
}

impl Service for QueueRecordingImport {}

impl BlockImport for QueueRecordingImport {
    async fn check(&self, block: &Block) -> Result<(), ServiceError> {
        if self.fail_checks.contains(&block.index()) {
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
        self.import_calls.fetch_add(1, Ordering::Relaxed);
        self.imported.lock().extend(blocks.iter().map(Block::index));
        Ok(BlockBatchImportOutcome::new(blocks.len()))
    }
}

#[tokio::test]
async fn block_import_queue_checks_then_imports_in_original_order() {
    let importer = Arc::new(QueueRecordingImport::default());
    let queue = BlockImportQueue::new(importer.clone(), 2);
    let blocks = vec![block(1), block(2), block(3)];

    let outcome = queue
        .push_blocks(blocks, BlockOrigin::Sync)
        .await
        .expect("queue import");

    assert_eq!(outcome.processed, 3);
    assert_eq!(importer.imported.lock().as_slice(), &[1, 2, 3]);
    let mut checked = importer.checked.lock().clone();
    checked.sort_unstable();
    assert_eq!(checked, vec![1, 2, 3]);
    assert_eq!(importer.import_calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn block_import_queue_check_failure_skips_import() {
    let importer = Arc::new(QueueRecordingImport {
        fail_checks: vec![2],
        ..QueueRecordingImport::default()
    });
    let queue = BlockImportQueue::new(importer.clone(), 2);

    let err = queue
        .push_blocks(vec![block(1), block(2), block(3)], BlockOrigin::Sync)
        .await
        .expect_err("check failure");

    assert!(err.to_string().contains("reject block 2"), "{err}");
    assert!(importer.imported.lock().is_empty());
    assert_eq!(importer.import_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn block_import_queue_empty_batch_is_a_noop() {
    let importer = Arc::new(QueueRecordingImport::default());
    let queue = BlockImportQueue::new(importer.clone(), 2);

    let outcome = queue
        .push_blocks(Vec::new(), BlockOrigin::Sync)
        .await
        .expect("empty queue import");

    assert_eq!(outcome.processed, 0);
    assert_eq!(importer.import_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn block_import_queue_filters_arc_candidates_without_cloning_blocks() {
    let importer = Arc::new(QueueRecordingImport {
        fail_checks: vec![2, 4],
        ..QueueRecordingImport::default()
    });
    let queue = BlockImportQueue::new(importer, 2);
    let first = Arc::new(block(1));
    let rejected = Arc::new(block(2));
    let third = Arc::new(block(3));
    let rejected_fourth = Arc::new(block(4));

    let checked = queue
        .check_blocks(vec![
            Arc::clone(&first),
            Arc::clone(&rejected),
            Arc::clone(&third),
            rejected_fourth,
        ])
        .await
        .expect("lossy preflight");

    assert_eq!(checked.accepted_len(), 2);
    assert_eq!(checked.rejected_len(), 2);
    assert!(Arc::ptr_eq(&checked.blocks()[0], &first));
    assert!(Arc::ptr_eq(&checked.blocks()[1], &third));
    assert_eq!(checked.rejected()[0].position(), 1);
    assert_eq!(checked.rejected()[0].error().category(), "invalid_input");
    assert_eq!(checked.rejected()[1].position(), 3);
}
