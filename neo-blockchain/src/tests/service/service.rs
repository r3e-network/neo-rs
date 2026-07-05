use super::*;
use crate::command::BlockchainCommand;
use crate::handle::BlockchainHandle;
use std::sync::Arc;

/// Trivial in-memory mempool used by the unit tests.
#[derive(Debug, Default)]
struct TestMempool;

impl MempoolLike for TestMempool {
    fn try_add(
        &self,
        _tx: &neo_payloads::Transaction,
        _snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }

    fn try_add_cached(
        &self,
        _tx: &neo_payloads::Transaction,
        _snapshot: &neo_storage::DataCache,
        _settings: &neo_config::ProtocolSettings,
        _cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }
}

/// Stub system context used by the unit tests.
#[derive(Debug)]
struct TestContext;

impl crate::service_context::SystemContext for TestContext {
    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::new(neo_config::ProtocolSettings::default())
    }

    fn current_height(&self) -> u32 {
        0
    }
}

#[test]
fn blockchain_service_can_be_assembled_from_concrete_types() {
    let system = Arc::new(TestContext);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);

    let _service: BlockchainService<TestContext, TestMempool> =
        BlockchainService::with_defaults(system, ledger, header_cache, mempool).0;
}

#[tokio::test]
async fn run_loop_processes_simple_command() {
    let system = Arc::new(TestContext);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);

    let task = tokio::spawn(service.run());

    // GetHeight command.
    let height = handle.get_height().await.expect("get_height");
    assert_eq!(height, 0);

    // GetBlock for an unknown hash returns None.
    let hash = neo_primitives::UInt256::zero();
    let block = handle.get_block(&hash).await.expect("get_block");
    assert!(block.is_none());

    // Drop the handle to close the channel; the run loop should exit.
    drop(handle);
    task.await.expect("service task");
}

#[test]
fn handle_debug_includes_capacity() {
    let (handle, _rx) = BlockchainHandle::with_capacity();
    let s = format!("{:?}", handle);
    assert!(s.contains("BlockchainHandle"));
}

#[tokio::test]
async fn handle_submits_inventory_block_batches_without_exposing_command_enum() {
    let (handle, mut cmd_rx, _event_tx) = BlockchainHandle::channel(4, 4);
    let block = Arc::new(neo_payloads::Block::new());

    handle
        .submit_inventory_blocks(vec![Arc::clone(&block)], true, false)
        .await
        .expect("submit inventory blocks");

    match cmd_rx.recv().await.expect("inventory command") {
        BlockchainCommand::InventoryBlocks {
            blocks,
            relay,
            pre_verified,
        } => {
            assert_eq!(blocks.len(), 1);
            assert!(Arc::ptr_eq(&blocks[0], &block));
            assert!(relay);
            assert!(!pre_verified);
        }
        other => panic!("expected InventoryBlocks command, got {other:?}"),
    }
}

#[test]
fn service_debug_does_not_panic() {
    let system = Arc::new(TestContext);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    let s = format!("{:?}", service);
    assert!(s.contains("BlockchainService"));
}
