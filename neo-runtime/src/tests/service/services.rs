use super::*;
use crate::{BlockImport, BlockImportOutcome, BlockOrigin, ImportedTip};
use std::sync::Arc;

/// No-op service used to verify the trait is object-safe and can be
/// held behind an `Arc<dyn ...>`.
#[derive(Debug)]
struct DummyExecutor;

impl Service for DummyExecutor {}

#[async_trait]
impl BlockExecutor for DummyExecutor {
    async fn execute(&self, _block: &Block) -> Result<ExecutionOutcome, ServiceError> {
        Ok(ExecutionOutcome::default())
    }

    async fn validate(&self, _block: &Block) -> Result<(), ServiceError> {
        Ok(())
    }
}

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
    fn _executor(_: &dyn BlockExecutor) {}
    fn _network(_: &dyn NetworkService) {}
    fn _consensus(_: &dyn ConsensusService) {}
    fn _engine(_: &dyn NeoEngine) {}
}

#[tokio::test]
async fn dummy_executor_runs() {
    let exec: Arc<dyn BlockExecutor> = Arc::new(DummyExecutor);
    let block = Block::new();
    exec.execute(&block).await.expect("execute");
    exec.validate(&block).await.expect("validate");
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
