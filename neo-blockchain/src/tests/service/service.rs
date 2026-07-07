use super::*;
use crate::command::BlockchainCommand;
use crate::handle::BlockchainHandle;
use neo_primitives::verify_result::VerifyResult;
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

#[tokio::test]
async fn handle_shutdown_stops_service_run_loop() {
    let system = Arc::new(TestContext);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(async move {
        service.run().await;
        let _ = done_tx.send(());
    });

    handle.shutdown().await.expect("shutdown request");
    tokio::task::spawn_blocking(move || done_rx.recv_timeout(std::time::Duration::from_secs(1)))
        .await
        .expect("shutdown wait task")
        .expect("service run loop should stop after shutdown");
    task.await.expect("service task");
}

#[test]
fn handle_debug_includes_capacity() {
    let (handle, _rx) = BlockchainHandle::with_capacity();
    let s = format!("{:?}", handle);
    assert!(s.contains("BlockchainHandle"));
}

#[test]
fn handle_subscribe_creates_independent_event_receivers() {
    let (handle, _rx, event_tx) = BlockchainHandle::channel(4, 4);
    let mut first = handle.subscribe();
    let mut second = handle.subscribe();

    event_tx
        .send(crate::RuntimeEvent::Shutdown)
        .expect("event receivers");

    assert_eq!(
        first.try_recv().expect("first event"),
        crate::RuntimeEvent::Shutdown
    );
    assert_eq!(
        second.try_recv().expect("second event"),
        crate::RuntimeEvent::Shutdown
    );
}

#[tokio::test]
async fn handle_block_import_check_rejects_bad_empty_block_merkle_root() {
    let (handle, _rx) = BlockchainHandle::with_capacity();
    let mut header = neo_payloads::Header::new();
    header.set_index(1);
    header.set_merkle_root(neo_primitives::UInt256::from([0x42; 32]));
    let block = neo_payloads::Block::from_parts(header, Vec::new());

    let importer: &dyn neo_runtime::BlockImport = &handle;
    let err = importer
        .check(&block)
        .await
        .expect_err("bad empty-block merkle root must fail preverification");

    assert!(
        err.to_string().contains("Merkle root mismatch"),
        "error should name merkle-root mismatch: {err}"
    );
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

#[tokio::test]
async fn handle_submits_single_inventory_block_without_exposing_command_enum() {
    let (handle, mut cmd_rx, _event_tx) = BlockchainHandle::channel(4, 4);
    let block = Arc::new(neo_payloads::Block::new());

    handle
        .submit_inventory_block(Arc::clone(&block), true, true)
        .await
        .expect("submit inventory block");

    match cmd_rx.recv().await.expect("inventory command") {
        BlockchainCommand::InventoryBlock {
            block: submitted,
            relay,
            pre_verified,
        } => {
            assert!(Arc::ptr_eq(&submitted, &block));
            assert!(relay);
            assert!(pre_verified);
        }
        other => panic!("expected InventoryBlock command, got {other:?}"),
    }
}

#[tokio::test]
async fn handle_submits_extensible_inventory_without_exposing_command_enum() {
    let (handle, mut cmd_rx, _event_tx) = BlockchainHandle::channel(4, 4);
    let payload = neo_payloads::ExtensiblePayload::default();

    handle
        .submit_inventory_extensible(payload.clone(), true)
        .await
        .expect("submit extensible inventory");

    match cmd_rx.recv().await.expect("inventory command") {
        BlockchainCommand::InventoryExtensible {
            payload: submitted,
            relay,
        } => {
            assert_eq!(submitted.category, payload.category);
            assert_eq!(submitted.valid_block_start, payload.valid_block_start);
            assert_eq!(submitted.valid_block_end, payload.valid_block_end);
            assert_eq!(submitted.sender, payload.sender);
            assert_eq!(submitted.data, payload.data);
            assert!(relay);
        }
        other => panic!("expected InventoryExtensible command, got {other:?}"),
    }
}

#[tokio::test]
async fn handle_add_transaction_round_trips_service_reply() {
    let (handle, mut cmd_rx, _event_tx) = BlockchainHandle::channel(4, 4);
    let transaction = neo_payloads::Transaction::new();
    let expected_hash = transaction.try_hash().expect("transaction hash");

    let task = tokio::spawn(async move {
        handle
            .add_transaction(transaction)
            .await
            .expect("add transaction reply")
    });

    match cmd_rx.recv().await.expect("add transaction command") {
        BlockchainCommand::AddTransaction { transaction, reply } => {
            assert_eq!(
                transaction.try_hash().expect("submitted transaction hash"),
                expected_hash
            );
            reply
                .send(crate::command::AddTransactionReply {
                    result: VerifyResult::Succeed,
                    hash: expected_hash,
                })
                .expect("reply send");
        }
        other => panic!("expected AddTransaction command, got {other:?}"),
    }

    let reply = task.await.expect("add transaction task");
    assert_eq!(reply.result, VerifyResult::Succeed);
    assert_eq!(reply.hash, expected_hash);
}

#[tokio::test]
async fn handle_initializes_without_exposing_command_enum() {
    let (handle, mut cmd_rx, _event_tx) = BlockchainHandle::channel(4, 4);

    handle.initialize().await.expect("initialize");

    match cmd_rx.recv().await.expect("initialize command") {
        BlockchainCommand::Initialize => {}
        other => panic!("expected Initialize command, got {other:?}"),
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
