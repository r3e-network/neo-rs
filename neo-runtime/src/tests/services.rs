use super::*;
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

#[test]
fn traits_are_object_safe() {
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
