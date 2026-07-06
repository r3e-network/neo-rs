use super::*;
use crate::{BlockImport, BlockOrigin, Service};
use async_trait::async_trait;
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
    fail_check_at: Option<u32>,
    import_calls: AtomicUsize,
}

impl Service for QueueRecordingImport {}

#[async_trait]
impl BlockImport for QueueRecordingImport {
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
        fail_check_at: Some(2),
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
