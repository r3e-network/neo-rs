use super::*;
use crate::{BlockImport, BlockImportOutcome, BlockOrigin, ImportQueue, ImportedTip};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct RecordingImporter {
    checked: Mutex<Vec<u32>>,
    imported: Mutex<Vec<u32>>,
    checks_seen_by_import: Mutex<Vec<usize>>,
    fail_check_at: Option<u32>,
}

impl RecordingImporter {
    fn new(fail_check_at: Option<u32>) -> Self {
        Self {
            checked: Mutex::new(Vec::new()),
            imported: Mutex::new(Vec::new()),
            checks_seen_by_import: Mutex::new(Vec::new()),
            fail_check_at,
        }
    }

    fn checked(&self) -> Vec<u32> {
        self.checked.lock().expect("checked lock").clone()
    }

    fn imported(&self) -> Vec<u32> {
        self.imported.lock().expect("imported lock").clone()
    }

    fn checks_seen_by_import(&self) -> Vec<usize> {
        self.checks_seen_by_import
            .lock()
            .expect("checks seen lock")
            .clone()
    }
}

impl Service for RecordingImporter {}

#[async_trait]
impl BlockImport for RecordingImporter {
    async fn check(&self, block: &Block) -> Result<(), ServiceError> {
        self.checked
            .lock()
            .expect("checked lock")
            .push(block.index());
        if self.fail_check_at == Some(block.index()) {
            return Err(ServiceError::invalid_input(format!(
                "bad block {}",
                block.index()
            )));
        }
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
        let checked_count = self.checked.lock().expect("checked lock").len();
        self.checks_seen_by_import
            .lock()
            .expect("checks seen lock")
            .push(checked_count);

        let mut imported = self.imported.lock().expect("imported lock");
        imported.extend(blocks.iter().map(Block::index));
        Ok(BlockBatchImportOutcome::new(blocks.len()))
    }
}

fn block(index: u32) -> Block {
    let mut header = neo_payloads::Header::new();
    header.set_index(index);
    Block::from_parts(header, vec![])
}

#[tokio::test]
async fn import_queue_checks_entire_batch_before_importing() {
    let importer = Arc::new(RecordingImporter::new(None));
    let queue = BlockImportQueue::new(importer.clone(), 4);

    let outcome = queue
        .push_blocks(vec![block(1), block(2), block(3)], BlockOrigin::Sync)
        .await
        .expect("queue import");

    assert_eq!(outcome.processed, 3);
    let mut checked = importer.checked();
    checked.sort_unstable();
    assert_eq!(checked, vec![1, 2, 3]);
    assert_eq!(importer.checks_seen_by_import(), vec![3]);
    assert_eq!(importer.imported(), vec![1, 2, 3]);
}

#[tokio::test]
async fn import_queue_does_not_import_when_preflight_fails() {
    let importer = Arc::new(RecordingImporter::new(Some(2)));
    let queue = BlockImportQueue::new(importer.clone(), 4);

    let err = queue
        .push_blocks(vec![block(1), block(2), block(3)], BlockOrigin::Sync)
        .await
        .expect_err("preflight should reject the batch");

    assert!(err.to_string().contains("bad block 2"));
    assert!(importer.imported().is_empty());
    assert!(importer.checks_seen_by_import().is_empty());
}
