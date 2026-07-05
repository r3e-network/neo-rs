use super::*;
use crate::{BlockImport, BlockImportOutcome, BlockOrigin, ImportedTip};
use std::sync::Arc;

/// No-op service used to verify the traits are object-safe and can be
/// held behind an `Arc<dyn ...>`.
#[derive(Debug)]
struct DummyBlockImporter;

impl Service for DummyBlockImporter {}

#[async_trait]
impl BlockImport for DummyBlockImporter {
    async fn check(&self, _block: &Block) -> Result<(), ServiceError> {
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
}

#[test]
fn traits_are_object_safe() {
    fn _importer(_: &dyn BlockImport) {}
    fn _network(_: &dyn NetworkService) {}
}

#[tokio::test]
async fn dummy_block_importer_reports_imported_tip() {
    let importer: Arc<dyn BlockImport> = Arc::new(DummyBlockImporter);
    let block = Block::new();
    let expected_tip = ImportedTip::from_block(&block).expect("default block hash");

    importer.check(&block).await.expect("check");
    let outcome = importer
        .import(block, BlockOrigin::Sync)
        .await
        .expect("import");

    assert_eq!(outcome, BlockImportOutcome::Imported(expected_tip));
}
