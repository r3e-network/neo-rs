use super::*;
use crate::{BlockImport, BlockImportOutcome, BlockOrigin, ImportedTip};

/// No-op service used to verify concrete service implementations.
#[derive(Debug)]
struct DummyBlockImporter;

impl Service for DummyBlockImporter {}

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

#[tokio::test]
async fn dummy_block_importer_reports_imported_tip() {
    let importer = DummyBlockImporter;
    let block = Block::new();
    let expected_tip = ImportedTip::from_block(&block).expect("default block hash");

    importer.check(&block).await.expect("check");
    let outcome = importer
        .import(block, BlockOrigin::Sync)
        .await
        .expect("import");

    assert_eq!(outcome, BlockImportOutcome::Imported(expected_tip));
}
