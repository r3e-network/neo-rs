use super::*;
use crate::command::BlockchainCommand;
use crate::handle::BlockchainHandle;
use neo_config::{ChainSpecProvider, NeoChainSpec};
use neo_mempool::{TransactionAdmissionError, TransactionAdmissionOutcome, TransactionOrigin};
use neo_payloads::Header;
use neo_primitives::verify_result::VerifyResult;
use neo_runtime::ServiceError;
use std::sync::Arc;

/// Trivial in-memory mempool used by the unit tests.
#[derive(Debug, Default)]
struct TestMempool;

impl MempoolLike for TestMempool {
    fn add_transaction<B, L>(
        &self,
        origin: TransactionOrigin,
        transaction: &neo_payloads::Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _ledger_provider: &L,
    ) -> TransactionAdmissionOutcome
    where
        B: neo_storage::CacheRead,
        L: neo_mempool::AdmissionLedgerProvider,
    {
        match transaction.try_hash() {
            Ok(hash) => TransactionAdmissionOutcome::Accepted { hash, origin },
            Err(error) => TransactionAdmissionOutcome::Error {
                hash: None,
                origin,
                error: TransactionAdmissionError::InvalidHash(error.to_string()),
            },
        }
    }
}

/// Stub system context used by the unit tests.
#[derive(Debug)]
struct TestContext;

impl ChainSpecProvider for TestContext {
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        neo_test_fixtures::test_chain_spec(neo_config::ProtocolSettings::default())
    }
}

impl crate::service_context::SystemContext for TestContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn current_height(&self) -> u32 {
        0
    }
}

#[derive(Debug)]
struct StopAfterCommandContext;

impl ChainSpecProvider for StopAfterCommandContext {
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        neo_test_fixtures::test_chain_spec(neo_config::ProtocolSettings::default())
    }
}

impl crate::service_context::SystemContext for StopAfterCommandContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn current_height(&self) -> u32 {
        0
    }

    fn should_stop_blockchain_service(&self) -> bool {
        true
    }
}

struct MismatchedGenesisContext {
    chain_spec: Arc<NeoChainSpec>,
    snapshot: Arc<neo_storage::DataCache>,
    native_persist_requests: Arc<std::sync::atomic::AtomicUsize>,
    commit_requests: Arc<std::sync::atomic::AtomicUsize>,
}

impl std::fmt::Debug for MismatchedGenesisContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MismatchedGenesisContext")
            .finish_non_exhaustive()
    }
}

impl ChainSpecProvider for MismatchedGenesisContext {
    type ChainSpec = NeoChainSpec;

    fn chain_spec(&self) -> Arc<Self::ChainSpec> {
        Arc::clone(&self.chain_spec)
    }
}

impl crate::service_context::SystemContext for MismatchedGenesisContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn current_height(&self) -> u32 {
        0
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_persist_resources(
        &self,
    ) -> Option<crate::native_persist::NativePersistResources<Self::NativeProvider>> {
        self.native_persist_requests
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        None
    }

    fn commit_to_store(&self) -> Result<(), String> {
        self.commit_requests
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

fn header_fixture(index: u32) -> Header {
    let mut header = Header::new();
    header.set_index(index);
    header
}

fn service_fixture() -> (
    BlockchainService<TestContext, TestMempool>,
    BlockchainHandle,
    Arc<HeaderCache>,
) {
    let system = Arc::new(TestContext);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) =
        BlockchainService::with_defaults(system, ledger, Arc::clone(&header_cache), mempool);
    (service, handle, header_cache)
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

#[tokio::test]
async fn fatal_persistence_state_drops_queued_commands_immediately() {
    let system = Arc::new(StopAfterCommandContext);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);

    let (first_reply, first_response) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(BlockchainCommand::GetHeight { reply: first_reply })
        .await
        .expect("queue first command");
    let (second_reply, second_response) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(BlockchainCommand::GetHeight {
            reply: second_reply,
        })
        .await
        .expect("queue second command");

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let task = tokio::spawn(async move {
        service.run().await;
        let _ = done_tx.send(());
    });
    assert_eq!(first_response.await.expect("first command reply"), 0);
    assert!(
        second_response.await.is_err(),
        "queued commands after a fatal persistence result must be dropped"
    );
    tokio::task::spawn_blocking(move || done_rx.recv_timeout(std::time::Duration::from_secs(1)))
        .await
        .expect("fatal stop wait task")
        .expect("fatal service stop timeout");
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

    let err = neo_runtime::BlockImport::check(&handle, &block)
        .await
        .expect_err("bad empty-block merkle root must fail preverification");

    assert!(
        err.to_string().contains("Merkle root mismatch"),
        "error should name merkle-root mismatch: {err}"
    );
}

#[tokio::test]
async fn handle_submits_checker_typed_inventory_batches_without_exposing_command_enum() {
    let (handle, mut cmd_rx, _event_tx) = BlockchainHandle::channel(4, 4);
    let block = Arc::new(neo_payloads::Block::new());
    let queue = neo_runtime::BlockImportQueue::new(Arc::new(handle.clone()), 1);
    let checked = queue
        .check_blocks(vec![Arc::clone(&block)])
        .await
        .expect("check inventory blocks");

    handle
        .submit_checked_inventory_blocks(checked, true)
        .await
        .expect("submit inventory blocks");

    match cmd_rx.recv().await.expect("inventory command") {
        BlockchainCommand::CheckedInventoryBlocks { checked, relay } => {
            let (blocks, rejected) = checked.into_parts();
            assert_eq!(blocks.len(), 1);
            assert!(Arc::ptr_eq(&blocks[0], &block));
            assert!(rejected.is_empty());
            assert!(relay);
        }
        other => panic!("expected CheckedInventoryBlocks command, got {other:?}"),
    }
}

#[tokio::test]
async fn handle_submits_consensus_block_without_exposing_command_enum() {
    let (handle, mut cmd_rx, _event_tx) = BlockchainHandle::channel(4, 4);
    let block = Arc::new(neo_payloads::Block::new());

    handle
        .submit_consensus_block(Arc::clone(&block), true)
        .await
        .expect("submit consensus block");

    match cmd_rx.recv().await.expect("consensus command") {
        BlockchainCommand::ConsensusBlock {
            block: submitted,
            relay,
        } => {
            assert!(Arc::ptr_eq(&submitted, &block));
            assert!(relay);
        }
        other => panic!("expected ConsensusBlock command, got {other:?}"),
    }
}

#[tokio::test]
async fn handle_validate_headers_returns_valid_prefix_count() {
    let (service, handle, header_cache) = service_fixture();
    let task = tokio::spawn(service.run());

    let outcome = handle
        .validate_headers(vec![header_fixture(1), header_fixture(2)])
        .await
        .expect("validate headers");

    assert_eq!(outcome.accepted, 2);
    assert_eq!(outcome.frontier.as_ref().map(Header::index), Some(2));
    assert_eq!(header_cache.count(), 2);

    drop(handle);
    task.await.expect("service task");
}

#[tokio::test]
async fn handle_validate_headers_truncates_invalid_suffix() {
    let (service, handle, header_cache) = service_fixture();
    let task = tokio::spawn(service.run());

    let outcome = handle
        .validate_headers(vec![header_fixture(1), header_fixture(3)])
        .await
        .expect("validate headers");

    assert_eq!(outcome.accepted, 1);
    assert_eq!(outcome.frontier.as_ref().map(Header::index), Some(1));
    assert_eq!(header_cache.count(), 1);

    drop(handle);
    task.await.expect("service task");
}

#[tokio::test]
async fn handle_validate_headers_retries_cached_duplicates_idempotently() {
    let (service, handle, header_cache) = service_fixture();
    let task = tokio::spawn(service.run());
    let headers = vec![header_fixture(1), header_fixture(2)];

    let first = handle
        .validate_headers(headers.clone())
        .await
        .expect("first validation");
    let second = handle
        .validate_headers(headers)
        .await
        .expect("duplicate retry validation");

    assert_eq!(first.accepted, 2);
    assert_eq!(second.accepted, 2);
    assert_eq!(second.frontier.as_ref().map(Header::index), Some(2));
    assert_eq!(
        header_cache.count(),
        2,
        "retrying an already cached prefix must not duplicate cache entries"
    );

    drop(handle);
    task.await.expect("service task");
}

#[tokio::test]
async fn handle_validate_headers_reports_command_channel_closure() {
    let (handle, cmd_rx) = BlockchainHandle::with_capacity();
    drop(cmd_rx);

    let err = handle
        .validate_headers(vec![header_fixture(1)])
        .await
        .expect_err("closed command channel must surface as ServiceError");

    assert!(matches!(err, ServiceError::ServiceUnavailable(_)));
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
            .add_transaction(TransactionOrigin::Local, transaction)
            .await
            .expect("add transaction reply")
    });

    match cmd_rx.recv().await.expect("add transaction command") {
        BlockchainCommand::AddTransaction {
            transaction,
            origin,
            reply,
        } => {
            assert_eq!(
                transaction.try_hash().expect("submitted transaction hash"),
                expected_hash
            );
            assert_eq!(origin, TransactionOrigin::Local);
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

    let task = tokio::spawn(async move { handle.initialize().await });

    match cmd_rx.recv().await.expect("initialize command") {
        BlockchainCommand::Initialize { reply } => {
            reply.send(Ok(())).expect("initialize reply send");
        }
        other => panic!("expected Initialize command, got {other:?}"),
    }
    task.await
        .expect("initialize task")
        .expect("initialize result");
}

#[tokio::test]
async fn handle_initialization_surfaces_durable_genesis_failure() {
    let (handle, mut cmd_rx, _event_tx) = BlockchainHandle::channel(4, 4);
    let task = tokio::spawn(async move { handle.initialize().await });

    match cmd_rx.recv().await.expect("initialize command") {
        BlockchainCommand::Initialize { reply } => {
            reply
                .send(Err("injected genesis commit failure".to_string()))
                .expect("initialize reply send");
        }
        other => panic!("expected Initialize command, got {other:?}"),
    }

    let error = task
        .await
        .expect("initialize task")
        .expect_err("initialization error must reach the handle caller");
    assert_eq!(error.category(), "internal");
    assert!(error.to_string().contains("genesis commit failure"));
}

#[tokio::test]
async fn embedded_service_rejects_mismatched_genesis_pin_before_persistence() {
    let chain_spec = Arc::new(
        NeoChainSpec::private(
            "mismatched-genesis-test",
            neo_config::ProtocolSettings::default(),
            neo_config::GenesisConfig::mainnet(),
            Some(neo_primitives::UInt256::zero()),
        )
        .expect("valid pinned private chain specification"),
    );
    let native_persist_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let commit_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let system = Arc::new(MismatchedGenesisContext {
        chain_spec,
        snapshot: Arc::new(neo_storage::DataCache::new(false)),
        native_persist_requests: Arc::clone(&native_persist_requests),
        commit_requests: Arc::clone(&commit_requests),
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, _handle) =
        BlockchainService::with_defaults(system, Arc::clone(&ledger), header_cache, mempool);

    let error = service
        .initialize()
        .await
        .expect_err("mismatched genesis identity must fail closed");

    assert!(error.contains("genesis identity validation failed"));
    assert!(error.contains("genesis hash mismatch"));
    assert_eq!(
        native_persist_requests.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "native persistence resources must not be requested before the identity fence"
    );
    assert_eq!(
        commit_requests.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a mismatched genesis must never reach the durable commit fence"
    );
    assert_eq!(ledger.current_height(), 0);
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
