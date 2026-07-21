use super::*;
use neo_payloads::header::Header;

fn first_two_empty_blocks() -> (Block, Block) {
    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, Vec::new());
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block 1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());

    (block1, Block::from_parts(header2, Vec::new()))
}

struct FailingSecondCommitContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    commit_attempts: Arc<AtomicUsize>,
    commit_to_store_calls: Arc<AtomicUsize>,
    abort_store_commit_calls: Arc<AtomicUsize>,
    fatal_on_rejection: bool,
}

impl std::fmt::Debug for FailingSecondCommitContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FailingSecondCommitContext")
            .finish_non_exhaustive()
    }
}

impl SystemContext for FailingSecondCommitContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        0
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<NativeProviderArc> {
        Some(standard_native_provider())
    }

    fn block_committing(
        &self,
        _block: &Block,
        _snapshot: &neo_storage::DataCache,
        _application_executed_list: &[neo_payloads::ApplicationExecuted],
    ) -> bool {
        self.commit_attempts.fetch_add(1, Ordering::SeqCst) == 0
    }

    fn abort_store_commit(&self) {
        self.abort_store_commit_calls.fetch_add(1, Ordering::SeqCst);
        self.snapshot.reset();
    }

    fn should_stop_blockchain_service(&self) -> bool {
        self.fatal_on_rejection && self.commit_attempts.load(Ordering::SeqCst) >= 2
    }

    fn commit_to_store(&self) -> Result<(), String> {
        self.commit_to_store_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
    ) -> crate::SyncBatchCommitPolicy {
        crate::SyncBatchCommitPolicy::DeferredLive
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }
}

struct FailingBulkFlushContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    flush_calls: Arc<AtomicUsize>,
    commit_to_store_calls: Arc<AtomicUsize>,
}

struct FailingDurableCommitContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    fail_commits: Arc<std::sync::atomic::AtomicBool>,
    commit_calls: Arc<AtomicUsize>,
    committed_heights: Arc<parking_lot::Mutex<Vec<u32>>>,
}

struct FailingFinalizedDeliveryContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    fail_delivery: Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for FailingFinalizedDeliveryContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FailingFinalizedDeliveryContext")
            .finish_non_exhaustive()
    }
}

impl SystemContext for FailingFinalizedDeliveryContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        0
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<NativeProviderArc> {
        Some(standard_native_provider())
    }

    fn commit_to_store(&self) -> Result<(), String> {
        Ok(())
    }

    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
    ) -> crate::SyncBatchCommitPolicy {
        crate::SyncBatchCommitPolicy::PerBlock
    }

    async fn block_finalized(
        &self,
        _finalized: crate::FinalizedBlock<Self::CacheBacking>,
    ) -> Result<(), String> {
        if self.fail_delivery.load(Ordering::SeqCst) {
            Err("injected finalized delivery failure".to_string())
        } else {
            Ok(())
        }
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }

    fn should_stop_blockchain_service(&self) -> bool {
        self.fail_delivery.load(Ordering::SeqCst)
    }
}

struct StopAfterDurableCommitContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    arm_stop: Arc<std::sync::atomic::AtomicBool>,
    stop_requested: Arc<std::sync::atomic::AtomicBool>,
    commit_calls: Arc<AtomicUsize>,
    committed_heights: Arc<parking_lot::Mutex<Vec<u32>>>,
}

impl std::fmt::Debug for StopAfterDurableCommitContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StopAfterDurableCommitContext")
            .finish_non_exhaustive()
    }
}

impl SystemContext for StopAfterDurableCommitContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        0
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<NativeProviderArc> {
        Some(standard_native_provider())
    }

    fn commit_to_store(&self) -> Result<(), String> {
        self.commit_calls.fetch_add(1, Ordering::SeqCst);
        if self.arm_stop.load(Ordering::SeqCst) {
            self.stop_requested.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
    ) -> crate::SyncBatchCommitPolicy {
        crate::SyncBatchCommitPolicy::DeferredLive
    }

    fn should_stop_blockchain_service(&self) -> bool {
        self.stop_requested.load(Ordering::SeqCst)
    }

    async fn block_finalized(
        &self,
        finalized: crate::FinalizedBlock<Self::CacheBacking>,
    ) -> Result<(), String> {
        self.committed_heights
            .lock()
            .push(finalized.block().index());
        Ok(())
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }
}

impl std::fmt::Debug for FailingDurableCommitContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FailingDurableCommitContext")
            .finish_non_exhaustive()
    }
}

impl SystemContext for FailingDurableCommitContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        0
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<NativeProviderArc> {
        Some(standard_native_provider())
    }

    fn commit_to_store(&self) -> Result<(), String> {
        self.commit_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_commits.load(Ordering::SeqCst) {
            Err("injected durable store failure".to_string())
        } else {
            Ok(())
        }
    }

    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
    ) -> crate::SyncBatchCommitPolicy {
        crate::SyncBatchCommitPolicy::DeferredLive
    }

    async fn block_finalized(
        &self,
        finalized: crate::FinalizedBlock<Self::CacheBacking>,
    ) -> Result<(), String> {
        self.committed_heights
            .lock()
            .push(finalized.block().index());
        Ok(())
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }
}

fn failing_durable_commit_fixture() -> (
    BlockchainService<FailingDurableCommitContext, TestMempool>,
    Arc<std::sync::atomic::AtomicBool>,
    Arc<AtomicUsize>,
    Arc<parking_lot::Mutex<Vec<u32>>>,
) {
    let fail_commits = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let commit_calls = Arc::new(AtomicUsize::new(0));
    let committed_heights = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let system = Arc::new(FailingDurableCommitContext {
        snapshot: Arc::new(neo_storage::DataCache::new(false)),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        fail_commits: Arc::clone(&fail_commits),
        commit_calls: Arc::clone(&commit_calls),
        committed_heights: Arc::clone(&committed_heights),
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, fail_commits, commit_calls, committed_heights)
}

impl std::fmt::Debug for FailingBulkFlushContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FailingBulkFlushContext")
            .finish_non_exhaustive()
    }
}

impl SystemContext for FailingBulkFlushContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        0
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<NativeProviderArc> {
        Some(standard_native_provider())
    }

    fn flush_deferred_commit_handlers(&self) -> Result<(), String> {
        self.flush_calls.fetch_add(1, Ordering::SeqCst);
        Err("state-root worker reported a failed operation".to_string())
    }

    fn abort_store_commit(&self) {
        self.snapshot.reset();
    }

    fn commit_to_store(&self) -> Result<(), String> {
        self.commit_to_store_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }
}

struct StateServiceEmptyFastPathContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    state_service: Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>,
    fast_path_checks: Arc<AtomicUsize>,
    committing_heights: Arc<parking_lot::Mutex<Vec<u32>>>,
}

impl std::fmt::Debug for StateServiceEmptyFastPathContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateServiceEmptyFastPathContext")
            .finish_non_exhaustive()
    }
}

impl SystemContext for StateServiceEmptyFastPathContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        0
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<NativeProviderArc> {
        Some(standard_native_provider())
    }

    fn block_committing(
        &self,
        block: &Block,
        snapshot: &neo_storage::DataCache,
        _application_executed_list: &[neo_payloads::ApplicationExecuted],
    ) -> bool {
        self.state_service.on_committing(block.index(), snapshot)
    }

    fn block_committing_with_context(
        &self,
        block: &Block,
        snapshot: &neo_storage::DataCache,
        _application_executed_list: &[neo_payloads::ApplicationExecuted],
        _context: crate::service_context::BlockPersistContext,
    ) -> bool {
        self.committing_heights.lock().push(block.index());
        self.state_service
            .on_committing_deferred(block.index(), snapshot)
    }

    fn flush_deferred_commit_handlers(&self) -> Result<(), String> {
        self.state_service
            .flush_result()
            .map_err(|err| err.to_string())
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }

    fn allows_empty_block_committing_fast_forward(&self) -> bool {
        self.fast_path_checks.fetch_add(1, Ordering::SeqCst);
        true
    }
}

#[tokio::test]
async fn initialize_bootstraps_genesis_once_and_inventory_runs_native_hooks() {
    let (service, _handle, snapshot, state_store) = store_fixture_with_state_service();

    // C# Blockchain.OnInitialize: an uninitialized store gets the
    // genesis block persisted (native deploy seeds + mints).
    service.initialize().await.expect("initialize");
    assert!(crate::native_persist::chain_state_initialized(&snapshot));
    assert_eq!(
        neo_total_supply(&snapshot),
        Some(num_bigint::BigInt::from(100_000_000)),
        "genesis minted the NEO total supply"
    );
    assert!(
        service.ledger.block_hash_at(0).is_some(),
        "genesis cached in the ledger"
    );
    assert!(
        state_store
            .mpt()
            .expect("state store exposes MPT")
            .get_state_root(0)
            .is_some(),
        "genesis writes the local state-root record for block 0"
    );

    // Re-initializing must NOT re-persist (the initialized probe
    // guards the C# `Ledger.Initialized` branch): the supply stays
    // 100M instead of doubling.
    service.initialize().await.expect("initialize");
    assert_eq!(
        neo_total_supply(&snapshot),
        Some(num_bigint::BigInt::from(100_000_000))
    );

    // An inventory block at the next height runs the OnPersist /
    // PostPersist native hooks over the same store: block 1 mints
    // the 0.5-GAS committee reward to standby_committee[1 % 21].
    // The synthetic header carries no real consensus witness, so it goes
    // through the pre-verified path (the consensus-driver submission route);
    // witness verification of peer-relayed blocks has its own tests below.
    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, vec![]));
    service
        .handle_block_inventory(block, false, true)
        .await
        .expect("inventory block persists");
    assert_eq!(service.ledger.current_height(), 1);

    let settings = neo_config::ProtocolSettings::default();
    let member = &settings.standby_committee[1];
    let script = neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
        &member.to_bytes(),
    );
    let account = neo_primitives::UInt160::from_script(&script);
    let mut key = vec![20u8]; // shared NEP-17 Prefix_Account
    key.extend_from_slice(&account.to_bytes());
    assert!(
        snapshot
            .get(&neo_storage::StorageKey::new(
                neo_native_contracts::GasToken::ID,
                key
            ))
            .is_some(),
        "block-1 PostPersist minted the rotating committee reward"
    );
}

#[tokio::test]
async fn state_service_failure_aborts_before_chain_tip_advances() {
    let (service, _handle, snapshot, state_store) = store_fixture_with_state_service();
    service.initialize().await.expect("initialize");
    assert_eq!(service.ledger.current_height(), 0);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("genesis ledger current index"),
        0
    );

    state_store
        .mpt()
        .expect("state store exposes MPT")
        .revert_local_roots(0, 0)
        .expect("revert current root pointer");
    assert_eq!(
        state_store
            .mpt()
            .expect("state store exposes MPT")
            .current_local_root_index(),
        None,
        "state-service root is intentionally invalidated before the next block"
    );

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_next_consensus(*genesis.header.next_consensus());

    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(header, vec![])), false, true)
        .await
        .expect_err("non-contiguous StateService root must abort block persistence");

    assert!(
        err.to_string()
            .contains("native persistence pipeline failed"),
        "unexpected error: {err}"
    );
    assert_eq!(
        service.ledger.current_height(),
        0,
        "in-memory chain height must not advance after StateService failure"
    );
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("durable ledger current index"),
        0,
        "durable ledger height must not advance after StateService failure"
    );
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(mpt.current_local_root_index(), None);
    assert!(mpt.get_state_root(5).is_none());
}

#[tokio::test]
async fn bulk_import_flush_failure_aborts_batch_before_durable_store_commit() {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let flush_calls = Arc::new(AtomicUsize::new(0));
    let commit_to_store_calls = Arc::new(AtomicUsize::new(0));
    let system = Arc::new(FailingBulkFlushContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        flush_calls: Arc::clone(&flush_calls),
        commit_to_store_calls: Arc::clone(&commit_to_store_calls),
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    service.initialize().await.expect("initialize");
    flush_calls.store(0, Ordering::SeqCst);
    commit_to_store_calls.store(0, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());

    let imported = service
        .handle_import(Import {
            blocks: vec![genesis, block1, Block::from_parts(header2, vec![])],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(
        imported.imported, 1,
        "only the already-present genesis block remains durable after rewind"
    );
    assert!(
        imported
            .error
            .as_deref()
            .is_some_and(|error| error.contains("state-root worker")),
        "bulk import should return the StateService finalization error, got {:?}",
        imported.error
    );
    assert_eq!(
        flush_calls.load(Ordering::SeqCst),
        1,
        "bulk import should flush pending async commit handlers at the batch boundary"
    );
    assert_eq!(
        commit_to_store_calls.load(Ordering::SeqCst),
        0,
        "durable chain store must not commit after StateService MPT flush failure"
    );
    assert!(
        imported.stats.finalization_commit_handlers_elapsed > std::time::Duration::ZERO,
        "failed bulk finalization should attribute time to commit-handler flush"
    );
    assert_eq!(
        imported.stats.finalization_store_commit_elapsed,
        std::time::Duration::ZERO,
        "failed bulk finalization must not report durable-store commit time"
    );
    assert!(
        imported.stats.finalization_elapsed >= imported.stats.finalization_commit_handlers_elapsed,
        "aggregate finalization must cover commit-handler flush time"
    );
    assert_eq!(
        service.ledger.current_height(),
        0,
        "failed bulk finalization must rewind the in-memory canonical tip"
    );
}

#[tokio::test]
async fn live_commit_failure_does_not_publish_or_advance_the_in_memory_tip() {
    let (service, fail_commits, commit_calls, committed_heights) = failing_durable_commit_fixture();
    service.initialize().await.expect("initialize");
    committed_heights.lock().clear();
    fail_commits.store(true, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_next_consensus(*genesis.header.next_consensus());

    let error = service
        .handle_block_inventory(Arc::new(Block::from_parts(header, Vec::new())), false, true)
        .await
        .expect_err("injected commit failure must reject the block");

    assert!(error.to_string().contains("durable store commit failed"));
    assert_eq!(commit_calls.load(Ordering::SeqCst), 2);
    assert_eq!(service.ledger.current_height(), 0);
    assert!(
        committed_heights.lock().is_empty(),
        "finalized consumers must not run after a failed durable commit"
    );
}

#[tokio::test]
async fn non_bulk_import_reply_surfaces_durable_commit_failure() {
    let (service, fail_commits, _commit_calls, committed_heights) =
        failing_durable_commit_fixture();
    service.initialize().await.expect("initialize");
    committed_heights.lock().clear();
    fail_commits.store(true, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_next_consensus(*genesis.header.next_consensus());

    let imported = service
        .handle_import(Import {
            blocks: vec![Block::from_parts(header, Vec::new())],
            mode: ImportMode::Live { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 0);
    assert!(
        imported
            .error
            .as_deref()
            .is_some_and(|error| error.contains("durable store commit failed"))
    );
    assert_eq!(service.ledger.current_height(), 0);
    assert!(committed_heights.lock().is_empty());
}

#[tokio::test]
async fn non_bulk_finalized_delivery_failure_reports_the_durable_prefix() {
    let fail_delivery = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let system = Arc::new(FailingFinalizedDeliveryContext {
        snapshot: Arc::new(neo_storage::DataCache::new(false)),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        fail_delivery: Arc::clone(&fail_delivery),
    });
    let (service, _handle) = BlockchainService::with_defaults(
        system,
        Arc::new(LedgerContext::default()),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    service.initialize().await.expect("initialize");
    fail_delivery.store(true, Ordering::SeqCst);

    let (block, _) = first_two_empty_blocks();
    let reply = service
        .handle_import(Import {
            blocks: vec![block],
            mode: ImportMode::Live { verify: false },
        })
        .await;

    assert_eq!(reply.imported, 1, "the Ledger block is already durable");
    assert!(
        reply
            .error
            .as_deref()
            .is_some_and(|error| error.contains("finalized delivery failed"))
    );
    assert_eq!(service.ledger.current_height(), 1);
}

#[tokio::test]
async fn non_bulk_import_stops_after_durable_block_requests_writer_shutdown() {
    let arm_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_requested = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let commit_calls = Arc::new(AtomicUsize::new(0));
    let committed_heights = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let system = Arc::new(StopAfterDurableCommitContext {
        snapshot: Arc::new(neo_storage::DataCache::new(false)),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        arm_stop: Arc::clone(&arm_stop),
        stop_requested: Arc::clone(&stop_requested),
        commit_calls: Arc::clone(&commit_calls),
        committed_heights: Arc::clone(&committed_heights),
    });
    let (service, _handle) = BlockchainService::with_defaults(
        system,
        Arc::new(LedgerContext::default()),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    service.initialize().await.expect("initialize");
    commit_calls.store(0, Ordering::SeqCst);
    committed_heights.lock().clear();
    arm_stop.store(true, Ordering::SeqCst);

    let (block1, block2) = first_two_empty_blocks();
    let imported = service
        .handle_import(Import {
            blocks: vec![block1, block2],
            mode: ImportMode::Live { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 1, "the first block is already durable");
    assert!(
        imported
            .error
            .as_deref()
            .is_some_and(|error| error.contains("writer shutdown requested"))
    );
    assert_eq!(commit_calls.load(Ordering::SeqCst), 1);
    assert_eq!(service.ledger.current_height(), 1);
    assert_eq!(committed_heights.lock().as_slice(), &[1]);
    assert!(stop_requested.load(Ordering::SeqCst));
}

#[tokio::test]
async fn bulk_import_reports_shutdown_requested_after_successful_durable_fence() {
    let arm_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_requested = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let commit_calls = Arc::new(AtomicUsize::new(0));
    let committed_heights = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let system = Arc::new(StopAfterDurableCommitContext {
        snapshot: Arc::new(neo_storage::DataCache::new(false)),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        arm_stop: Arc::clone(&arm_stop),
        stop_requested: Arc::clone(&stop_requested),
        commit_calls: Arc::clone(&commit_calls),
        committed_heights: Arc::clone(&committed_heights),
    });
    let (service, _handle) = BlockchainService::with_defaults(
        system,
        Arc::new(LedgerContext::default()),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    service.initialize().await.expect("initialize");
    commit_calls.store(0, Ordering::SeqCst);
    committed_heights.lock().clear();
    arm_stop.store(true, Ordering::SeqCst);

    let (block1, block2) = first_two_empty_blocks();
    let imported = service
        .handle_import(Import {
            blocks: vec![block1, block2],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(
        imported.imported, 2,
        "the atomic batch completed its durable fence"
    );
    assert!(
        imported
            .error
            .as_deref()
            .is_some_and(|error| error.contains("writer shutdown was requested"))
    );
    assert_eq!(commit_calls.load(Ordering::SeqCst), 1);
    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(committed_heights.lock().as_slice(), &[1, 2]);
    assert!(stop_requested.load(Ordering::SeqCst));
}

#[tokio::test]
async fn bulk_commit_failure_rewinds_staged_tip_and_suppresses_post_commit_hooks() {
    let (service, fail_commits, commit_calls, committed_heights) = failing_durable_commit_fixture();
    service.initialize().await.expect("initialize");
    committed_heights.lock().clear();
    fail_commits.store(true, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, Vec::new());
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block 1 hash");
    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, Block::from_parts(header2, Vec::new())],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(
        imported.imported, 0,
        "a failed atomic bulk commit leaves no durable imported prefix"
    );
    assert!(
        imported
            .error
            .as_deref()
            .is_some_and(|error| error.contains("injected durable store failure"))
    );
    assert_eq!(commit_calls.load(Ordering::SeqCst), 2);
    assert_eq!(service.ledger.current_height(), 0);
    assert!(committed_heights.lock().is_empty());
}

#[tokio::test]
async fn inventory_batch_commit_failure_rewinds_tip_before_publishing_blocks() {
    let (service, fail_commits, commit_calls, committed_heights) = failing_durable_commit_fixture();
    service.initialize().await.expect("initialize");
    committed_heights.lock().clear();
    fail_commits.store(true, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_next_consensus(*genesis.header.next_consensus());

    let error = service
        .handle_block_inventory_batch(
            vec![Arc::new(Block::from_parts(header, Vec::new()))],
            false,
            true,
        )
        .await
        .expect_err("batch commit failure must reach the caller");

    assert!(error.to_string().contains("durable store commit failed"));
    assert_eq!(commit_calls.load(Ordering::SeqCst), 2);
    assert_eq!(service.ledger.current_height(), 0);
    assert!(committed_heights.lock().is_empty());
}

#[tokio::test]
async fn future_inventory_block_is_parked_then_drained_after_parent_persists() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Arc::new(Block::from_parts(header1, vec![]));
    let block1_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(block1.as_ref())
            .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Arc::new(Block::from_parts(header2, vec![]));

    service
        .handle_block_inventory(Arc::clone(&block2), false, true)
        .await
        .expect("future block is parked, not rejected");
    assert_eq!(service.ledger.current_height(), 0);
    assert_eq!(service.unverified_block_count(), 1);
    assert!(service.ledger.block_hash_at(2).is_none());

    service
        .handle_block_inventory(block1, false, true)
        .await
        .expect("parent block persists and drains child");

    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(service.unverified_block_count(), 0);
    assert!(service.ledger.block_hash_at(1).is_some());
    assert!(service.ledger.block_hash_at(2).is_some());
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        2
    );
}

#[test]
fn unverified_cache_evicts_exact_block_fraction_when_one_height_is_flooded() {
    const CACHE_CAPACITY: usize = 50_000;
    let (service, _handle, _snapshot) = store_fixture();
    let mut header = Header::new();
    header.set_index(10_000);
    let block = Arc::new(Block::from_parts(header, vec![]));

    for _ in 0..CACHE_CAPACITY {
        assert!(service.park_unverified_block(
            Arc::clone(&block),
            false,
            false,
            crate::internal::BlockIntegrity::Unchecked,
        ));
    }
    assert_eq!(service.unverified_block_count(), CACHE_CAPACITY);

    assert!(service.park_unverified_block(
        block,
        false,
        false,
        crate::internal::BlockIntegrity::Unchecked,
    ));
    assert_eq!(
        service.unverified_block_count(),
        CACHE_CAPACITY - CACHE_CAPACITY / 4 + 1,
        "overflow must evict 25% of blocks, not the entire flooded height bucket"
    );
}

#[tokio::test]
async fn inventory_block_batch_persists_consecutive_blocks_through_inventory_path() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Arc::new(Block::from_parts(header1, vec![]));
    let block1_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(block1.as_ref())
            .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Arc::new(Block::from_parts(header2, vec![]));

    let imported = service
        .handle_block_inventory_batch(vec![block1, block2], false, true)
        .await
        .expect("inventory batch commit");

    assert_eq!(imported, 2);
    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        2
    );
}

#[tokio::test]
async fn inventory_block_batch_continues_after_rejected_block_like_individual_commands() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut bad_header = Header::new();
    bad_header.set_index(1);
    bad_header.set_prev_hash(neo_primitives::UInt256::from([0xAB; 32]));
    bad_header.set_timestamp(genesis.header.timestamp() + 15_000);
    bad_header.set_next_consensus(*genesis.header.next_consensus());

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());

    let imported = service
        .handle_block_inventory_batch(
            vec![
                Arc::new(Block::from_parts(bad_header, vec![])),
                Arc::new(Block::from_parts(header1, vec![])),
            ],
            false,
            true,
        )
        .await
        .expect("inventory batch commit");

    assert_eq!(imported, 1);
    assert_eq!(service.ledger.current_height(), 1);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        1
    );
}

#[tokio::test]
async fn checked_peer_batch_still_requires_consensus_witness_verification() {
    let (service, handle, _snapshot) = store_fixture();
    service.initialize().await.expect("initialize");
    let (unsigned_block, _) = first_two_empty_blocks();
    let queue = neo_runtime::BlockImportQueue::new(Arc::new(handle), 1);
    let checked = queue
        .check_blocks(vec![Arc::new(unsigned_block)])
        .await
        .expect("stateless preflight");

    let imported = service
        .handle_checked_block_inventory_batch(checked, false)
        .await
        .expect("witness rejection is isolated within the live batch");

    assert_eq!(imported, 0);
    assert_eq!(service.ledger.current_height(), 0);
}

#[tokio::test]
async fn checked_future_block_keeps_preflight_proof_while_parked() {
    let (service, handle, _snapshot) = store_fixture();
    service.initialize().await.expect("initialize");
    let (_, future_block) = first_two_empty_blocks();
    let queue = neo_runtime::BlockImportQueue::new(Arc::new(handle), 1);
    let checked = queue
        .check_blocks(vec![Arc::new(future_block)])
        .await
        .expect("stateless preflight");

    let imported = service
        .handle_checked_block_inventory_batch(checked, false)
        .await
        .expect("future block should park");

    assert_eq!(imported, 0);
    let parked = service
        .unverified_blocks
        .lock()
        .pop_front(2)
        .expect("future block remains parked");
    assert_eq!(parked.integrity, crate::internal::BlockIntegrity::Checked);
}

#[tokio::test]
async fn stop_height_allows_target_block_and_rejects_later_blocks() {
    let (mut service, _handle, _snapshot) = store_fixture();
    service.set_stop_at_height(Some(1));
    service.initialize().await.expect("initialize");

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Arc::new(Block::from_parts(header1, vec![]));
    let block1_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(block1.as_ref())
            .expect("block1 hash");

    service
        .handle_block_inventory(block1, false, true)
        .await
        .expect("target stop-height block persists");
    assert_eq!(service.ledger.current_height(), 1);

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(header2, vec![])), false, true)
        .await
        .expect_err("block after stop height must not persist");
    assert!(
        err.to_string().contains("stop height 1"),
        "error should name the configured stop height: {err}"
    );
    assert_eq!(service.ledger.current_height(), 1);
    assert!(service.ledger.block_hash_at(2).is_none());
}

#[tokio::test]
async fn future_block_with_cached_header_hash_mismatch_is_rejected_not_parked() {
    let (service, _handle) = fixture();
    let mut header1 = Header::new();
    header1.set_index(1);
    let mut header2 = Header::new();
    header2.set_index(2);
    let outcome = service.handle_headers(vec![header1, header2.clone()]);
    assert_eq!(
        outcome.accepted, 2,
        "both headers are accepted into the cache"
    );
    assert_eq!(
        outcome.frontier.as_ref().map(neo_payloads::Header::index),
        Some(2)
    );
    assert_eq!(service.header_cache.count(), 2, "headers cached first");

    let mut competing = header2;
    competing.set_nonce(0xAA55_AA55_AA55_AA55);
    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(competing, vec![])), false, true)
        .await
        .expect_err("future block hash must match an existing cached header");
    assert!(
        err.to_string().contains("cached header"),
        "rejection should name cached header mismatch: {err}"
    );
    assert_eq!(
        service.unverified_block_count(),
        0,
        "mismatched future block must not be parked"
    );
}

#[tokio::test]
async fn import_verify_true_rejects_invalid_header_like_csharp() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp());
    header.set_next_consensus(*genesis.header.next_consensus());

    service
        .handle_import(Import {
            blocks: vec![Block::from_parts(header, vec![])],
            mode: ImportMode::Live { verify: true },
        })
        .await;

    assert_eq!(
        service.ledger.current_height(),
        0,
        "C# OnImport(verify: true) stops before persisting an invalid header"
    );
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        0
    );
}

#[tokio::test]
async fn import_verify_true_rejects_invalid_transaction_merkle_root() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_next_consensus(*genesis.header.next_consensus());

    let reply = service
        .handle_import(Import {
            // Intentionally do not rebuild the merkle root after adding the
            // transaction. `verify: true` must run the shared validate stage
            // before persistence, so the stale zero root is rejected.
            blocks: vec![Block::from_parts(header, vec![transaction_with_nonce(42)])],
            mode: ImportMode::Live { verify: true },
        })
        .await;

    assert_eq!(reply.imported, 0);
    assert_eq!(
        service.ledger.current_height(),
        0,
        "verified import must reject blocks whose transaction merkle root is invalid"
    );
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        0
    );
}

#[tokio::test]
async fn bulk_import_uses_batch_block_before_parked_duplicate_height() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");
    fund_test_signer_gas(&snapshot, 100_0000_0000);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut parked_header2 = Header::new();
    parked_header2.set_index(2);
    parked_header2.set_prev_hash(block1_hash);
    parked_header2.set_timestamp(genesis.header.timestamp() + 30_000);
    parked_header2.set_next_consensus(*genesis.header.next_consensus());
    let parked_block2 = Arc::new(Block::from_parts(parked_header2, vec![]));
    let parked_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(parked_block2.as_ref())
            .expect("parked block2 hash");

    service
        .handle_block_inventory(Arc::clone(&parked_block2), false, true)
        .await
        .expect("future block is parked");
    assert_eq!(service.unverified_block_count(), 1);

    let mut import_header2 = Header::new();
    import_header2.set_index(2);
    import_header2.set_prev_hash(block1_hash);
    import_header2.set_timestamp(genesis.header.timestamp() + 45_000);
    import_header2.set_next_consensus(*genesis.header.next_consensus());
    let mut import_block2 = Block::from_parts(import_header2, vec![transaction_with_nonce(2)]);
    import_block2
        .try_rebuild_merkle_root()
        .expect("transaction merkle root");
    let import_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(&import_block2)
            .expect("import block2 hash");
    assert_ne!(
        parked_hash, import_hash,
        "fixture must distinguish the parked peer block from the trusted bulk block"
    );

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, import_block2],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(
        service.ledger.block_hash_at(2),
        Some(import_hash),
        "trusted bulk import batch must win over a parked duplicate-height peer block"
    );
    assert_eq!(
        service.unverified_block_count(),
        0,
        "bulk import should discard stale parked blocks at or below the imported tip"
    );
}

#[tokio::test]
async fn import_verify_false_skips_header_verification_like_csharp() {
    let (service, _handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp());
    header.set_next_consensus(*genesis.header.next_consensus());
    assert!(
        service.header_cache.add(header.clone()),
        "test starts with a header-first cached block"
    );

    service
        .handle_import(Import {
            blocks: vec![Block::from_parts(header, vec![])],
            mode: ImportMode::Live { verify: false },
        })
        .await;

    assert_eq!(
        service.ledger.current_height(),
        1,
        "C# OnImport(verify: false) bypasses Block.Verify and persists the next block"
    );
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        1
    );
    assert_eq!(
        service.header_cache.count(),
        0,
        "C# Persist removes the first cached header after committing the block"
    );
}

#[tokio::test]
async fn explicit_bulk_import_skips_replay_artifacts_but_normal_import_keeps_them() {
    let (normal_service, _normal_handle, _normal_snapshot, normal_lengths) =
        store_fixture_recording_application_executed_lengths();
    normal_service.initialize().await.expect("initialize");
    normal_lengths.lock().clear();

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut normal_header = Header::new();
    normal_header.set_index(1);
    normal_header.set_prev_hash(genesis.hash());
    normal_header.set_timestamp(genesis.header.timestamp());
    normal_header.set_next_consensus(*genesis.header.next_consensus());

    let imported = normal_service
        .handle_import(Import {
            blocks: vec![Block::from_parts(normal_header, vec![])],
            mode: ImportMode::Live { verify: false },
        })
        .await;
    assert_eq!(imported.imported, 1);
    assert_eq!(
        normal_lengths.lock().as_slice(),
        &[2],
        "normal import must keep OnPersist/PostPersist ApplicationExecuted replay records"
    );

    let (bulk_service, _bulk_handle, _bulk_snapshot, bulk_lengths) =
        store_fixture_recording_application_executed_lengths();
    bulk_service.initialize().await.expect("initialize");
    bulk_lengths.lock().clear();

    let mut bulk_header = Header::new();
    bulk_header.set_index(1);
    bulk_header.set_prev_hash(genesis.hash());
    bulk_header.set_timestamp(genesis.header.timestamp());
    bulk_header.set_next_consensus(*genesis.header.next_consensus());

    let imported = bulk_service
        .handle_import(Import {
            blocks: vec![Block::from_parts(bulk_header, vec![])],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;
    assert_eq!(imported.imported, 1);
    assert_eq!(
        bulk_lengths.lock().as_slice(),
        &[0],
        "bulk import should skip replay-only ApplicationExecuted artifacts"
    );
}

#[tokio::test]
async fn live_import_skips_replay_artifacts_when_composition_has_no_consumer() {
    let (service, _handle, _snapshot, lengths) =
        store_fixture_recording_application_executed_lengths_with_policy(false);
    service.initialize().await.expect("initialize");
    lengths.lock().clear();

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp());
    header.set_next_consensus(*genesis.header.next_consensus());

    let imported = service
        .handle_import(Import {
            blocks: vec![Block::from_parts(header, vec![])],
            mode: ImportMode::Live { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 1);
    assert_eq!(
        lengths.lock().as_slice(),
        &[0],
        "observer-free live import must not copy OnPersist/PostPersist replay records"
    );
}

#[tokio::test]
async fn bulk_import_flushes_store_once_per_accepted_batch() {
    let (normal_service, _normal_handle, _normal_snapshot, normal_commits) =
        store_fixture_counting_commits();
    normal_service.initialize().await.expect("initialize");
    normal_commits.store(0, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut normal_header1 = Header::new();
    normal_header1.set_index(1);
    normal_header1.set_prev_hash(genesis.hash());
    normal_header1.set_timestamp(genesis.header.timestamp() + 15_000);
    normal_header1.set_next_consensus(*genesis.header.next_consensus());
    let normal_block1 = Block::from_parts(normal_header1, vec![]);
    let normal_block1_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(&normal_block1)
            .expect("normal block1 hash");

    let mut normal_header2 = Header::new();
    normal_header2.set_index(2);
    normal_header2.set_prev_hash(normal_block1_hash);
    normal_header2.set_timestamp(genesis.header.timestamp() + 30_000);
    normal_header2.set_next_consensus(*genesis.header.next_consensus());

    let imported = normal_service
        .handle_import(Import {
            blocks: vec![normal_block1, Block::from_parts(normal_header2, vec![])],
            mode: ImportMode::Live { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert_eq!(
        normal_commits.load(Ordering::SeqCst),
        2,
        "normal imports keep per-block durable flush behavior"
    );

    let (bulk_service, _bulk_handle, _bulk_snapshot, bulk_commits) =
        store_fixture_counting_commits();
    bulk_service.initialize().await.expect("initialize");
    bulk_commits.store(0, Ordering::SeqCst);

    let mut bulk_header1 = Header::new();
    bulk_header1.set_index(1);
    bulk_header1.set_prev_hash(genesis.hash());
    bulk_header1.set_timestamp(genesis.header.timestamp() + 15_000);
    bulk_header1.set_next_consensus(*genesis.header.next_consensus());
    assert!(
        bulk_service.header_cache.add(bulk_header1.clone()),
        "bulk import test starts with cached header 1"
    );
    let bulk_block1 = Block::from_parts(bulk_header1, vec![]);
    let bulk_block1_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(&bulk_block1)
            .expect("bulk block1 hash");

    let mut bulk_header2 = Header::new();
    bulk_header2.set_index(2);
    bulk_header2.set_prev_hash(bulk_block1_hash);
    bulk_header2.set_timestamp(genesis.header.timestamp() + 30_000);
    bulk_header2.set_next_consensus(*genesis.header.next_consensus());
    assert!(
        bulk_service.header_cache.add(bulk_header2.clone()),
        "bulk import test starts with cached header 2"
    );

    let imported = bulk_service
        .handle_import(Import {
            blocks: vec![bulk_block1, Block::from_parts(bulk_header2, vec![])],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert_eq!(
        bulk_commits.load(Ordering::SeqCst),
        1,
        "bulk import should flush the durable store once for the accepted batch"
    );
    assert_eq!(
        bulk_service.header_cache.count(),
        0,
        "bulk import should clear consumed cached headers after the accepted batch"
    );
}

#[tokio::test]
async fn bulk_import_reuses_store_snapshot_for_accepted_batch() {
    let (service, _handle, _snapshot, snapshot_calls, commit_calls) =
        store_fixture_counting_snapshot_and_commits();
    service.initialize().await.expect("initialize");
    snapshot_calls.store(0, Ordering::SeqCst);
    commit_calls.store(0, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, Block::from_parts(header2, vec![])],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert_eq!(
        snapshot_calls.load(Ordering::SeqCst),
        1,
        "bulk import should reuse the shared store snapshot across accepted blocks"
    );
    assert_eq!(
        commit_calls.load(Ordering::SeqCst),
        1,
        "bulk import should still flush the durable store once after the accepted batch"
    );
}

#[tokio::test]
async fn bulk_import_fast_forwards_empty_run_when_no_per_block_observers_are_active() {
    let (service, _handle, snapshot, snapshot_calls, commit_calls) =
        store_fixture_counting_snapshot_and_commits();
    service.initialize().await.expect("initialize");
    snapshot_calls.store(0, Ordering::SeqCst);
    commit_calls.store(0, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Block::from_parts(header2, vec![]);

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, block2],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert_eq!(
        snapshot_calls.load(Ordering::SeqCst),
        1,
        "fast-forward should reuse the batch snapshot"
    );
    assert_eq!(
        commit_calls.load(Ordering::SeqCst),
        1,
        "fast-forwarded bulk import still flushes the durable store once"
    );
    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        2
    );
    assert!(
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&snapshot, 1)
            .expect("block hash 1")
            .is_some(),
        "fast-forward must preserve ledger history for block 1"
    );
    assert!(
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&snapshot, 2)
            .expect("block hash 2")
            .is_some(),
        "fast-forward must preserve ledger history for block 2"
    );
}

#[tokio::test]
async fn bulk_import_fast_forwards_short_empty_bursts_around_transaction_blocks() {
    let (service, _handle, snapshot, snapshot_calls, commit_calls, committed_heights) =
        store_fixture_counting_snapshot_commits_and_committed_heights();
    service.initialize().await.expect("initialize");
    fund_test_signer_gas(&snapshot, 100_0000_0000);
    snapshot_calls.store(0, Ordering::SeqCst);
    commit_calls.store(0, Ordering::SeqCst);
    committed_heights.lock().clear();

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut blocks = Vec::new();
    let mut prev_hash = genesis.hash();
    let mut timestamp = genesis.header.timestamp();
    let next_consensus = *genesis.header.next_consensus();

    for index in 1..=65u32 {
        timestamp += 15_000;
        let mut header = Header::new();
        header.set_index(index);
        header.set_prev_hash(prev_hash);
        header.set_timestamp(timestamp);
        header.set_next_consensus(next_consensus);
        let transactions = if index == 33 {
            vec![transaction_with_nonce(index)]
        } else {
            Vec::new()
        };
        let mut block = Block::from_parts(header, transactions);
        if !block.transactions.is_empty() {
            block
                .try_rebuild_merkle_root()
                .expect("transaction merkle root");
        }
        prev_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block)
            .expect("block hash");
        blocks.push(block);
    }

    let transaction_block_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(&blocks[32])
            .expect("transaction block hash");

    let imported = service
        .handle_import(Import {
            blocks,
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 65);
    assert_eq!(imported.stats.empty_blocks, 64);
    assert!(
        imported.stats.empty_elapsed > std::time::Duration::ZERO,
        "empty fast-forward timing should be reported for mixed bulk imports"
    );
    assert_eq!(imported.stats.transaction_blocks, 1);
    assert!(
        imported.stats.transaction_elapsed > std::time::Duration::ZERO,
        "transaction timing should be reported independently from empty fast-forward timing"
    );
    assert!(
        imported.stats.finalization_elapsed > std::time::Duration::ZERO,
        "bulk finalization timing should be reported separately"
    );
    assert!(
        imported.stats.finalization_commit_handlers_elapsed
            + imported.stats.finalization_store_commit_elapsed
            <= imported.stats.finalization_elapsed,
        "bulk finalization components must fit inside aggregate timing"
    );
    assert_eq!(
        snapshot_calls.load(Ordering::SeqCst),
        1,
        "short empty bursts and the transaction block should share one batch snapshot"
    );
    assert_eq!(
        commit_calls.load(Ordering::SeqCst),
        1,
        "mixed bulk import should still flush once"
    );
    assert_eq!(service.ledger.current_height(), 65);
    assert_eq!(
        service.ledger.block_hash_at(33),
        Some(transaction_block_hash),
        "transaction-bearing blocks still populate the hot ledger cache"
    );
    assert_eq!(
        committed_heights.lock().as_slice(),
        &[33],
        "transaction-bearing blocks keep normal finalized delivery while surrounding empty bursts are fast-forwarded"
    );
    assert!(
        service.ledger.block_hash_at(1).is_none(),
        "fast-forwarded empty blocks should not populate the hot height hash cache"
    );
    assert!(
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&snapshot, 1)
            .expect("empty block hash")
            .is_some(),
        "fast-forwarded empty block history remains queryable from durable ledger records"
    );
    assert!(
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&snapshot, 65)
            .expect("last empty block hash")
            .is_some(),
        "second empty burst history remains queryable from durable ledger records"
    );
}

#[tokio::test]
async fn bulk_import_fast_forward_chunks_empty_runs_beyond_one_internal_batch() {
    let (service, _handle, snapshot, snapshot_calls, commit_calls) =
        store_fixture_counting_snapshot_and_commits();
    service.initialize().await.expect("initialize");
    snapshot_calls.store(0, Ordering::SeqCst);
    commit_calls.store(0, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let block_count = crate::empty_block_fast_forward::MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS + 2;
    let mut blocks = Vec::with_capacity(block_count);
    let mut prev_hash = genesis.hash();
    let mut timestamp = genesis.header.timestamp();
    let next_consensus = *genesis.header.next_consensus();

    for index in 1..=block_count as u32 {
        timestamp += 15_000;
        let mut header = Header::new();
        header.set_index(index);
        header.set_prev_hash(prev_hash);
        header.set_timestamp(timestamp);
        header.set_next_consensus(next_consensus);
        let block = Block::from_parts(header, vec![]);
        prev_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block)
            .expect("empty block hash");
        blocks.push(block);
    }

    let imported = service
        .handle_import(Import {
            blocks,
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, block_count);
    assert_eq!(
        snapshot_calls.load(Ordering::SeqCst),
        1,
        "multi-chunk fast-forward should keep reusing the batch snapshot"
    );
    assert_eq!(
        commit_calls.load(Ordering::SeqCst),
        1,
        "outer bulk import should still flush the durable store once"
    );
    assert_eq!(service.ledger.current_height(), block_count as u32);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        block_count as u32
    );
    assert!(
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&snapshot, block_count as u32)
            .expect("last block hash")
            .is_some(),
        "durable ledger history must include the final fast-forwarded block"
    );
}

#[tokio::test]
async fn bulk_import_fast_forward_skips_observer_callbacks_when_fast_path_is_allowed() {
    let (service, _handle, _snapshot, committed_heights) =
        store_fixture_recording_committed_heights();
    service.initialize().await.expect("initialize");
    committed_heights.lock().clear();

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Block::from_parts(header2, vec![]);

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, block2],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert!(
        committed_heights.lock().is_empty(),
        "fast-forward is only allowed when per-block observers are inactive"
    );
}

#[tokio::test]
async fn bulk_import_falls_back_when_per_block_committing_observer_is_active() {
    let (service, _handle, _snapshot, lengths) =
        store_fixture_recording_application_executed_lengths();
    service.initialize().await.expect("initialize");
    lengths.lock().clear();

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Block::from_parts(header2, vec![]);

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, block2],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert_eq!(
        lengths.lock().as_slice(),
        &[0, 0],
        "observer contexts must receive per-block committing calls and therefore disable fast-forward"
    );
}

#[tokio::test]
async fn bulk_import_uses_empty_fast_path_when_only_state_service_is_loaded() {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let state_store = Arc::new(neo_state_service::StateStore::with_mpt(true));
    let state_service = Arc::new(
        neo_state_service::commit_handlers::StateServiceCommitHandlers::new(Arc::clone(
            &state_store,
        )),
    );
    let fast_path_checks = Arc::new(AtomicUsize::new(0));
    let committing_heights = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let system = Arc::new(StateServiceEmptyFastPathContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        state_service,
        fast_path_checks: Arc::clone(&fast_path_checks),
        committing_heights: Arc::clone(&committing_heights),
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    service.initialize().await.expect("initialize");
    fast_path_checks.store(0, Ordering::SeqCst);
    committing_heights.lock().clear();

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Block::from_parts(header2, vec![]);

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, block2],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert!(
        fast_path_checks.load(Ordering::SeqCst) > 0,
        "bulk import should consult the StateService-compatible empty-block fast path"
    );
    assert_eq!(
        committing_heights.lock().as_slice(),
        &[1, 2],
        "StateService-compatible fast path must still run per-block committing hooks"
    );
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert!(
        mpt.get_state_root(1).is_some(),
        "loaded StateService must observe block 1 even when empty native persistence is optimized"
    );
    assert!(
        mpt.get_state_root(2).is_some(),
        "loaded StateService must observe block 2 even when empty native persistence is optimized"
    );
    assert_eq!(service.ledger.current_height(), 2);
    assert!(
        service.ledger.block_hash_at(1).is_some(),
        "StateService-compatible fast path keeps the normal hot ledger cache updates"
    );
    assert!(
        service.ledger.block_hash_at(2).is_some(),
        "StateService-compatible fast path keeps the normal hot ledger cache updates"
    );

    let (normal_service, _normal_handle, _normal_snapshot, normal_state_store) =
        store_fixture_with_state_service();
    normal_service.initialize().await.expect("initialize");
    let mut normal_header1 = Header::new();
    normal_header1.set_index(1);
    normal_header1.set_prev_hash(genesis.hash());
    normal_header1.set_timestamp(genesis.header.timestamp() + 15_000);
    normal_header1.set_next_consensus(*genesis.header.next_consensus());
    let normal_block1 = Block::from_parts(normal_header1, vec![]);
    let normal_block1_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(&normal_block1)
            .expect("normal block1 hash");

    let mut normal_header2 = Header::new();
    normal_header2.set_index(2);
    normal_header2.set_prev_hash(normal_block1_hash);
    normal_header2.set_timestamp(genesis.header.timestamp() + 30_000);
    normal_header2.set_next_consensus(*genesis.header.next_consensus());
    let normal_block2 = Block::from_parts(normal_header2, vec![]);

    let normal_imported = normal_service
        .handle_import(Import {
            blocks: vec![normal_block1, normal_block2],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;
    assert_eq!(normal_imported.imported, 2);

    let normal_mpt = normal_state_store.mpt().expect("normal MPT");
    for height in [1, 2] {
        let fast_root = mpt
            .get_state_root(height)
            .expect("fast-path state root")
            .root_hash;
        let normal_root = normal_mpt
            .get_state_root(height)
            .expect("normal state root")
            .root_hash;
        assert_eq!(
            fast_root, normal_root,
            "empty-block fast path must match normal StateService root at height {height}"
        );
    }
}

#[tokio::test]
async fn bulk_import_verify_true_validates_against_prior_batch_block() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");

    let (service, _handle, snapshot, snapshot_calls, commit_calls) =
        store_fixture_counting_snapshot_and_commits_with(settings.clone());
    service.initialize().await.expect("initialize");
    snapshot_calls.store(0, Ordering::SeqCst);
    commit_calls.store(0, Ordering::SeqCst);
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_primary_index(0);
    header1.set_next_consensus(*genesis.header.next_consensus());
    header1.witness = sign_header_for_test(&header1, network, &private_key, &verification);
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_primary_index(0);
    header2.set_next_consensus(*genesis.header.next_consensus());
    header2.witness = sign_header_for_test(&header2, network, &private_key, &verification);

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, Block::from_parts(header2, vec![])],
            mode: ImportMode::TrustedReplay { verify: true },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        2,
        "block 2 verification must see block 1 committed into the shared snapshot"
    );
    assert_eq!(
        snapshot_calls.load(Ordering::SeqCst),
        1,
        "verified bulk import should still reuse the parent snapshot across the batch"
    );
    assert_eq!(commit_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn bulk_import_keeps_accepted_prefix_when_second_block_committing_fails() {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let commit_attempts = Arc::new(AtomicUsize::new(0));
    let commit_to_store_calls = Arc::new(AtomicUsize::new(0));
    let abort_store_commit_calls = Arc::new(AtomicUsize::new(0));
    let system = Arc::new(FailingSecondCommitContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        commit_attempts: Arc::clone(&commit_attempts),
        commit_to_store_calls: Arc::clone(&commit_to_store_calls),
        abort_store_commit_calls: Arc::clone(&abort_store_commit_calls),
        fatal_on_rejection: false,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    service.initialize().await.expect("initialize");
    commit_attempts.store(0, Ordering::SeqCst);
    commit_to_store_calls.store(0, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, Block::from_parts(header2, vec![])],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(
        imported.imported, 1,
        "bulk import should keep the accepted prefix and stop at the failing block"
    );
    assert_eq!(service.ledger.current_height(), 1);
    assert_eq!(commit_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(abort_store_commit_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        commit_to_store_calls.load(Ordering::SeqCst),
        1,
        "the accepted prefix must pass through the durable batch fence before it is returned"
    );
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        1,
        "accepted block 1 must stay committed to the canonical snapshot"
    );
    assert!(
        snapshot
            .get(&neo_storage::StorageKey::new(
                neo_native_contracts::LedgerContract::ID,
                vec![12]
            ))
            .is_some()
    );
}

#[tokio::test]
async fn fatal_bulk_precommit_failure_aborts_staged_prefix_without_finalizing() {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let commit_attempts = Arc::new(AtomicUsize::new(0));
    let commit_to_store_calls = Arc::new(AtomicUsize::new(0));
    let abort_store_commit_calls = Arc::new(AtomicUsize::new(0));
    let system = Arc::new(FailingSecondCommitContext {
        snapshot,
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        commit_attempts: Arc::clone(&commit_attempts),
        commit_to_store_calls: Arc::clone(&commit_to_store_calls),
        abort_store_commit_calls: Arc::clone(&abort_store_commit_calls),
        fatal_on_rejection: true,
    });
    let (service, _handle) = BlockchainService::with_defaults(
        system,
        Arc::new(LedgerContext::default()),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    service.initialize().await.expect("initialize");
    commit_attempts.store(0, Ordering::SeqCst);
    commit_to_store_calls.store(0, Ordering::SeqCst);

    let (block1, block2) = first_two_empty_blocks();
    let imported = service
        .handle_import(Import {
            blocks: vec![block1, block2],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(
        imported.imported, 0,
        "fatal bulk rejection discards the staged prefix"
    );
    assert!(
        imported
            .error
            .as_deref()
            .is_some_and(|error| error.contains("native persistence pipeline failed"))
    );
    assert_eq!(commit_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        commit_to_store_calls.load(Ordering::SeqCst),
        0,
        "fatal bulk failure must not durably finalize the accepted prefix"
    );
    assert_eq!(abort_store_commit_calls.load(Ordering::SeqCst), 1);
    assert_eq!(service.ledger.current_height(), 0);
}

#[tokio::test]
async fn fatal_inventory_batch_failure_aborts_without_processing_later_blocks() {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let commit_attempts = Arc::new(AtomicUsize::new(0));
    let commit_to_store_calls = Arc::new(AtomicUsize::new(0));
    let abort_store_commit_calls = Arc::new(AtomicUsize::new(0));
    let system = Arc::new(FailingSecondCommitContext {
        snapshot,
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        commit_attempts: Arc::clone(&commit_attempts),
        commit_to_store_calls: Arc::clone(&commit_to_store_calls),
        abort_store_commit_calls: Arc::clone(&abort_store_commit_calls),
        fatal_on_rejection: true,
    });
    let (service, _handle) = BlockchainService::with_defaults(
        system,
        Arc::new(LedgerContext::default()),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    service.initialize().await.expect("initialize");
    commit_attempts.store(0, Ordering::SeqCst);
    commit_to_store_calls.store(0, Ordering::SeqCst);

    let (block1, block2) = first_two_empty_blocks();
    let block2 = Arc::new(block2);
    let error = service
        .handle_block_inventory_batch(
            vec![Arc::new(block1), Arc::clone(&block2), block2],
            false,
            true,
        )
        .await
        .expect_err("fatal second-block rejection must abort the inventory command");

    assert!(
        error
            .to_string()
            .contains("native persistence pipeline failed")
    );
    assert_eq!(
        commit_attempts.load(Ordering::SeqCst),
        2,
        "the duplicate candidate after the fatal block must not be processed"
    );
    assert_eq!(
        commit_to_store_calls.load(Ordering::SeqCst),
        0,
        "fatal inventory failure must not commit the staged first block"
    );
    assert_eq!(abort_store_commit_calls.load(Ordering::SeqCst), 1);
    assert_eq!(service.ledger.current_height(), 0);
}

#[tokio::test]
async fn inventory_batch_honors_per_block_observer_durability_policy() {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let commit_calls = Arc::new(AtomicUsize::new(0));
    let artifact_lengths = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: Some(Arc::clone(&artifact_lengths)),
        committed_heights: None,
        store_snapshot_calls: None,
        commit_to_store_calls: Some(Arc::clone(&commit_calls)),
    });
    let (service, _handle) = BlockchainService::with_defaults(
        system,
        Arc::new(LedgerContext::default()),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    service.initialize().await.expect("initialize");
    commit_calls.store(0, Ordering::SeqCst);
    artifact_lengths.lock().clear();
    let (block1, block2) = first_two_empty_blocks();

    assert_eq!(
        service
            .handle_block_inventory_batch(vec![Arc::new(block1), Arc::new(block2)], false, true,)
            .await
            .expect("inventory batch"),
        2
    );
    assert_eq!(commit_calls.load(Ordering::SeqCst), 2);
    assert_eq!(artifact_lengths.lock().len(), 2);
}

#[tokio::test]
async fn inventory_block_batch_flushes_store_once_for_contiguous_accepted_burst() {
    let (service, _handle, snapshot, commit_calls) = store_fixture_counting_commits();
    service.initialize().await.expect("initialize");
    commit_calls.store(0, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Arc::new(Block::from_parts(header1, vec![]));
    let block1_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(block1.as_ref())
            .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Arc::new(Block::from_parts(header2, vec![]));

    let imported = service
        .handle_block_inventory_batch(vec![block1, block2], false, true)
        .await
        .expect("inventory batch commit");

    assert_eq!(imported, 2);
    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        2
    );
    assert_eq!(
        commit_calls.load(Ordering::SeqCst),
        1,
        "contiguous inventory bursts should commit the shared store once after the accepted batch"
    );
}

#[tokio::test]
async fn inventory_block_batch_counts_and_flushes_drained_parked_children() {
    let (service, _handle, snapshot, commit_calls) = store_fixture_counting_commits();
    service.initialize().await.expect("initialize");
    commit_calls.store(0, Ordering::SeqCst);

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Arc::new(Block::from_parts(header1, vec![]));
    let block1_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(block1.as_ref())
            .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());
    let block2 = Arc::new(Block::from_parts(header2, vec![]));
    let block2_hash =
        BlockchainService::<StoreContext, TestMempool>::try_block_hash(block2.as_ref())
            .expect("block2 hash");

    let mut header3 = Header::new();
    header3.set_index(3);
    header3.set_prev_hash(block2_hash);
    header3.set_timestamp(genesis.header.timestamp() + 45_000);
    header3.set_next_consensus(*genesis.header.next_consensus());
    let block3 = Arc::new(Block::from_parts(header3, vec![]));

    service
        .handle_block_inventory(Arc::clone(&block3), false, true)
        .await
        .expect("future child is parked before parents arrive");
    assert_eq!(service.unverified_block_count(), 1);
    commit_calls.store(0, Ordering::SeqCst);

    let imported = service
        .handle_block_inventory_batch(vec![block1, block2], false, true)
        .await
        .expect("inventory batch commit");

    assert_eq!(
        imported, 3,
        "batch result should include direct imports and parked children drained by the batch"
    );
    assert_eq!(service.ledger.current_height(), 3);
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        3
    );
    assert_eq!(service.unverified_block_count(), 0);
    assert_eq!(
        commit_calls.load(Ordering::SeqCst),
        2,
        "direct burst flushes once; drained parked children keep the normal per-block flush path"
    );
}

#[tokio::test]
async fn bulk_import_skips_per_block_mempool_maintenance() {
    let settings = neo_config::ProtocolSettings::default();
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let system = Arc::new(StoreContext {
        snapshot,
        settings: Arc::new(settings.clone()),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: None,
        committed_heights: None,
        store_snapshot_calls: None,
        commit_to_store_calls: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let reverify_calls = Arc::new(AtomicUsize::new(0));
    let block_persisted_calls = Arc::new(AtomicUsize::new(0));
    let mempool = Arc::new(RecordingMempool {
        reverify_calls: Arc::clone(&reverify_calls),
        has_unverified_transactions: true,
        block_persisted_calls: Some(Arc::clone(&block_persisted_calls)),
    });
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    service.initialize().await.expect("initialize");

    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_next_consensus(*genesis.header.next_consensus());
    let block1 = Block::from_parts(header1, vec![]);
    let block1_hash = BlockchainService::<StoreContext, TestMempool>::try_block_hash(&block1)
        .expect("block1 hash");

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1_hash);
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_next_consensus(*genesis.header.next_consensus());

    let imported = service
        .handle_import(Import {
            blocks: vec![block1, Block::from_parts(header2, vec![])],
            mode: ImportMode::TrustedReplay { verify: false },
        })
        .await;

    assert_eq!(imported.imported, 2);
    assert_eq!(service.ledger.current_height(), 2);
    assert_eq!(
        block_persisted_calls.load(Ordering::SeqCst),
        0,
        "trusted bulk sync should not run per-block mempool eviction in the cold-import hot loop"
    );
    assert_eq!(
        reverify_calls.load(Ordering::SeqCst),
        0,
        "trusted bulk sync should not reverify mempool transactions while importing a local chain.acc package"
    );
}

#[tokio::test]
async fn import_blocks_handle_waits_until_batch_is_processed() {
    let (service, handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");
    let service_task = tokio::spawn(service.run());

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp());
    header.set_next_consensus(*genesis.header.next_consensus());

    let imported = handle
        .import_blocks(vec![Block::from_parts(header, vec![])], false)
        .await
        .expect("batch import command completes");

    assert_eq!(imported, 1, "reply reports the processed block count");
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        1,
        "import_blocks must not resolve before the durable import is visible"
    );

    drop(handle);
    service_task.await.expect("service task joins");
}

#[tokio::test]
async fn import_blocks_counts_duplicate_prefix_as_processed() {
    let (service, handle, snapshot) = store_fixture();
    service.initialize().await.expect("initialize");
    let service_task = tokio::spawn(service.run());

    let settings = neo_config::ProtocolSettings::default();
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp());
    header.set_next_consensus(*genesis.header.next_consensus());

    let processed = handle
        .import_blocks(
            vec![genesis.clone(), Block::from_parts(header, vec![])],
            false,
        )
        .await
        .expect("batch import command completes");

    assert_eq!(
        processed, 2,
        "duplicate genesis is a processed prefix item and must not stop chain.acc import"
    );
    assert_eq!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("ledger current index"),
        1,
        "only block 1 advances the durable tip"
    );

    drop(handle);
    service_task.await.expect("service task joins");
}

#[tokio::test]
async fn persisted_inventory_block_removes_cached_header_after_mempool_update() {
    let settings = neo_config::ProtocolSettings::default();
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(settings.clone()),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: None,
        committed_heights: None,
        store_snapshot_calls: None,
        commit_to_store_calls: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let reverify_calls = Arc::new(AtomicUsize::new(0));
    let mempool = Arc::new(RecordingMempool {
        reverify_calls: Arc::clone(&reverify_calls),
        has_unverified_transactions: true,
        block_persisted_calls: None,
    });
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, Arc::clone(&header_cache), mempool);

    service.initialize().await.expect("initialize");
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_next_consensus(*genesis.header.next_consensus());
    assert!(header_cache.add(header.clone()));

    service
        .handle_block_inventory(Arc::new(Block::from_parts(header, vec![])), false, true)
        .await
        .expect("cached-header block persists");

    assert_eq!(service.ledger.current_height(), 1);
    assert_eq!(
        reverify_calls.load(Ordering::SeqCst),
        0,
        "C# MemoryPool.UpdatePoolForBlockPersisted skips reverify while future headers are still cached"
    );
    assert_eq!(
        service.header_cache.count(),
        0,
        "C# Blockchain.Persist removes the consumed header after mempool update"
    );
}

/// End-to-end consensus-witness verification of a peer-relayed block: a
/// block signed by the network's validator (1-of-1 multisig over the C#
/// sign data = network magic LE + header hash) is accepted, and the same
/// block with a tampered signature is rejected. Proves the whole
/// `Header.Verify` path (prev-block lookup, timestamp/primary checks,
/// script-hash match against prev `NextConsensus`, CheckMultisig over the
/// header sign data) so live sync cannot be stalled by a broken verifier.
#[tokio::test]
async fn peer_block_witness_verification_accepts_valid_and_rejects_tampered() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    // Block 1 over genesis (no transactions; merkle root stays zero).
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_primary_index(0);
    header.set_next_consensus(*genesis.header.next_consensus());

    // C# sign data: network magic (LE) + header hash (witness excluded).
    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&header.hash().to_bytes());
    let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let invocation = |sig: &[u8]| {
        let mut script = vec![0x0C, 64]; // PUSHDATA1 64
        script.extend_from_slice(sig);
        script
    };

    // Tampered signature -> rejected, nothing persisted.
    let mut tampered_sig = signature;
    tampered_sig[10] ^= 0xFF;
    let mut tampered = header.clone();
    tampered.witness =
        neo_payloads::Witness::new_with_scripts(invocation(&tampered_sig), verification.clone());
    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(tampered, vec![])), false, false)
        .await
        .expect_err("tampered consensus witness must be rejected");
    assert!(
        err.to_string().contains("witness"),
        "rejection names the witness: {err}"
    );
    assert_eq!(service.ledger.current_height(), 0);

    // Valid signature -> accepted and persisted.
    header.witness = neo_payloads::Witness::new_with_scripts(invocation(&signature), verification);
    service
        .handle_block_inventory(Arc::new(Block::from_parts(header, vec![])), false, false)
        .await
        .expect("validly signed peer block is accepted");
    assert_eq!(service.ledger.current_height(), 1);
}

#[tokio::test]
async fn optimistic_inventory_batch_verifies_next_header_while_persisting_current_block() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let (mut service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");
    service.set_optimistic_signature_verification(Some(Arc::new(
        crate::pipeline::signature_verification::SignatureVerificationPool::new(
            crate::pipeline::signature_verification::SignatureVerificationPoolConfig {
                workers: 1,
                queue_capacity: 2,
            },
        )
        .expect("pool"),
    )));
    let genesis =
        crate::native_persist::genesis_block(&chain_spec_for_settings(&settings)).expect("genesis");

    let sign = |header: &Header| {
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&header.hash().to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
        let mut invocation = vec![0x0C, 64];
        invocation.extend_from_slice(&signature);
        neo_payloads::Witness::new_with_scripts(invocation, verification.clone())
    };

    let mut header1 = Header::new();
    header1.set_index(1);
    header1.set_prev_hash(genesis.hash());
    header1.set_timestamp(genesis.header.timestamp() + 15_000);
    header1.set_primary_index(0);
    header1.set_next_consensus(*genesis.header.next_consensus());
    header1.witness = sign(&header1);
    let block1 = Arc::new(Block::from_parts(header1, Vec::new()));

    let mut header2 = Header::new();
    header2.set_index(2);
    header2.set_prev_hash(block1.hash());
    header2.set_timestamp(genesis.header.timestamp() + 30_000);
    header2.set_primary_index(0);
    header2.set_next_consensus(*genesis.header.next_consensus());
    header2.witness = sign(&header2);
    let block2 = Arc::new(Block::from_parts(header2, Vec::new()));

    let imported = service
        .handle_block_inventory_batch(vec![block1, block2], false, false)
        .await
        .expect("optimistic batch import");
    assert_eq!(imported, 2);
    assert_eq!(service.ledger.current_height(), 2);
}

#[tokio::test]
async fn optimistic_inventory_window_stops_at_an_invalid_deep_header() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let (mut service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");
    let pool = Arc::new(
        crate::pipeline::signature_verification::SignatureVerificationPool::new(
            crate::pipeline::signature_verification::SignatureVerificationPoolConfig {
                workers: 2,
                queue_capacity: 4,
            },
        )
        .expect("pool"),
    );
    service.set_optimistic_signature_verification(Some(Arc::clone(&pool)));
    let genesis =
        crate::native_persist::genesis_block(&chain_spec_for_settings(&settings)).expect("genesis");

    let sign = |header: &Header| {
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&header.hash().to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
        let mut invocation = vec![0x0C, 64];
        invocation.extend_from_slice(&signature);
        neo_payloads::Witness::new_with_scripts(invocation, verification.clone())
    };

    let mut blocks = Vec::with_capacity(4);
    let mut previous_hash = genesis.hash();
    let mut previous_timestamp = genesis.header.timestamp();
    for index in 1..=4 {
        let mut header = Header::new();
        header.set_index(index);
        header.set_prev_hash(previous_hash);
        header.set_timestamp(previous_timestamp + 15_000);
        header.set_primary_index(0);
        header.set_next_consensus(*genesis.header.next_consensus());
        header.witness = if index == 3 {
            neo_payloads::Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH0.byte()])
        } else {
            sign(&header)
        };
        let block = Arc::new(Block::from_parts(header, Vec::new()));
        previous_hash = block.hash();
        previous_timestamp += 15_000;
        blocks.push(block);
    }

    let imported = service
        .handle_block_inventory_batch(blocks, false, false)
        .await
        .expect("optimistic batch import keeps the valid prefix");
    assert_eq!(imported, 2);
    assert_eq!(service.ledger.current_height(), 2);
    let metrics = pool.metrics_snapshot();
    assert!(
        metrics.submitted >= 3,
        "look-ahead should fill multiple tickets"
    );
    assert!(
        metrics.invalid >= 1,
        "the deep invalid witness must reach a worker"
    );
}

#[tokio::test]
async fn canonical_import_does_not_add_transaction_signature_gate() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");

    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let commit_attempts = Arc::new(AtomicUsize::new(0));
    let commit_to_store_calls = Arc::new(AtomicUsize::new(0));
    let abort_store_commit_calls = Arc::new(AtomicUsize::new(0));
    let system = Arc::new(FailingSecondCommitContext {
        snapshot: Arc::clone(&snapshot),
        chain_spec: chain_spec_for_settings(&settings),
        commit_attempts: Arc::clone(&commit_attempts),
        commit_to_store_calls: Arc::clone(&commit_to_store_calls),
        abort_store_commit_calls: Arc::clone(&abort_store_commit_calls),
        fatal_on_rejection: false,
    });
    let (mut service, _handle) = BlockchainService::with_defaults(
        system,
        Arc::new(LedgerContext::default()),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    service.initialize().await.expect("initialize");
    commit_attempts.store(0, Ordering::SeqCst);
    commit_to_store_calls.store(0, Ordering::SeqCst);

    let genesis =
        crate::native_persist::genesis_block(&chain_spec_for_settings(&settings)).expect("genesis");
    let transaction_verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&[2u8; 33]);
    let transaction_account = neo_primitives::UInt160::from_script(&transaction_verification);
    let mut transaction_invocation = vec![neo_vm::OpCode::PUSHDATA1.byte(), 64];
    transaction_invocation.extend_from_slice(&[0u8; 64]);
    let mut transaction = Transaction::new();
    transaction.set_script(vec![neo_vm::OpCode::PUSH1.byte()]);
    transaction.set_signers(vec![neo_payloads::Signer::new(
        transaction_account,
        neo_primitives::WitnessScope::NONE,
    )]);
    transaction.set_witnesses(vec![neo_payloads::Witness::new_with_scripts(
        transaction_invocation,
        transaction_verification,
    )]);

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_primary_index(0);
    header.set_next_consensus(*genesis.header.next_consensus());
    let mut block = Block::from_parts(header, vec![transaction]);
    block.try_rebuild_merkle_root().expect("merkle root");
    block.header.witness =
        sign_header_for_test(&block.header, network, &private_key, &verification);

    service.set_optimistic_signature_verification(Some(Arc::new(
        crate::pipeline::signature_verification::SignatureVerificationPool::new(
            crate::pipeline::signature_verification::SignatureVerificationPoolConfig {
                workers: 1,
                queue_capacity: 1,
            },
        )
        .expect("pool"),
    )));

    let imported = service
        .handle_import(Import {
            blocks: vec![block],
            mode: ImportMode::Sync,
        })
        .await;

    assert_eq!(imported.imported, 1);
    assert!(
        imported.error.is_none(),
        "canonical import must not add a transaction-signature gate: {imported:?}"
    );
    assert_eq!(service.ledger.current_height(), 1);
    assert_eq!(abort_store_commit_calls.load(Ordering::SeqCst), 0);
    assert_eq!(commit_to_store_calls.load(Ordering::SeqCst), 1);
    assert!(
        neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .is_ok(),
        "canonical import should publish the block after header verification"
    );
}

#[tokio::test]
async fn peer_block_witness_verification_does_not_trust_catchup_peer_tip() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    neo_runtime::sync_metrics::set_peer_live_tip(1_000_000);

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_primary_index(0);
    header.set_next_consensus(*genesis.header.next_consensus());

    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&header.hash().to_bytes());
    let mut signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
    signature[10] ^= 0xFF;
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let mut invocation = vec![0x0C, 64];
    invocation.extend_from_slice(&signature);
    header.witness = neo_payloads::Witness::new_with_scripts(invocation, verification);

    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(header, vec![])), false, false)
        .await
        .expect_err("catch-up peer tip must not bypass consensus witness verification");
    assert!(
        err.to_string().contains("witness"),
        "rejection names the witness: {err}"
    );
    assert_eq!(
        service.ledger.current_height(),
        0,
        "tampered peer block must not be persisted during catch-up"
    );
}

/// C# `Blockchain.OnNewBlock` rejects a full block whose height is already
/// represented in `HeaderCache` unless its hash equals the cached header
/// (`Blockchain.cs:241-243`).
#[tokio::test]
async fn peer_block_must_match_cached_header_hash() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, _handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");

    let sign_header = |header: &Header| {
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&header.hash().to_bytes());
        let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
        let mut invocation = vec![0x0C, 64];
        invocation.extend_from_slice(&signature);
        neo_payloads::Witness::new_with_scripts(invocation, verification.clone())
    };

    let mut cached = Header::new();
    cached.set_index(1);
    cached.set_prev_hash(genesis.hash());
    cached.set_timestamp(genesis.header.timestamp() + 15_000);
    cached.set_primary_index(0);
    cached.set_next_consensus(*genesis.header.next_consensus());
    cached.witness = sign_header(&cached);
    let outcome = service.handle_headers(vec![cached.clone()]);
    assert_eq!(
        outcome.accepted, 1,
        "cached header first enters the validated prefix"
    );
    assert_eq!(
        outcome.frontier.as_ref().map(neo_payloads::Header::hash),
        Some(cached.hash())
    );
    assert_eq!(service.header_cache.count(), 1, "header cached first");

    let mut competing = cached;
    competing.set_nonce(0xAA55_AA55_AA55_AA55);
    competing.witness = sign_header(&competing);

    let err = service
        .handle_block_inventory(Arc::new(Block::from_parts(competing, vec![])), false, false)
        .await
        .expect_err("block hash must match the cached header at the same height");
    assert!(
        err.to_string().contains("cached header"),
        "rejection should name cached header mismatch: {err}"
    );
    assert_eq!(
        service.ledger.current_height(),
        0,
        "mismatched block must not be persisted"
    );
}

/// Public `BlockchainHandle::import_block` is the RPC/user-submitted block
/// path, so it must wait for the typed service verdict and verify the consensus
/// witness instead of reporting success after merely queueing the command.
#[tokio::test]
async fn handle_import_block_reports_rejection_and_verifies_witness() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");
    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_primary_index(0);
    header.set_next_consensus(*genesis.header.next_consensus());

    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&header.hash().to_bytes());
    let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let invocation = |sig: &[u8]| {
        let mut script = vec![0x0C, 64];
        script.extend_from_slice(sig);
        script
    };

    let mut tampered_signature = signature;
    tampered_signature[10] ^= 0xFF;
    let mut tampered_header = header.clone();
    tampered_header.witness = neo_payloads::Witness::new_with_scripts(
        invocation(&tampered_signature),
        verification.clone(),
    );

    header.witness = neo_payloads::Witness::new_with_scripts(invocation(&signature), verification);

    let runner = tokio::spawn(service.run());

    let tampered_block = Block::from_parts(tampered_header, vec![]);
    let tampered_tip = neo_runtime::ImportedTip::from_block(&tampered_block).expect("tampered tip");
    let rejected = handle
        .import_block(tampered_block)
        .await
        .expect("import command reply");
    assert_eq!(
        rejected,
        neo_runtime::BlockImportOutcome::NotImported {
            hash: tampered_tip.hash,
            height: tampered_tip.height,
        },
        "tampered witness must not be reported as imported",
    );
    assert_eq!(handle.get_height().await.expect("height reply"), 0);

    let valid_block = Block::from_parts(header, vec![]);
    let valid_tip = neo_runtime::ImportedTip::from_block(&valid_block).expect("valid tip");
    let imported = handle
        .import_block(valid_block)
        .await
        .expect("import command reply");
    assert_eq!(
        imported,
        neo_runtime::BlockImportOutcome::Imported(valid_tip),
        "validly signed block advances the tip",
    );
    assert_eq!(handle.get_height().await.expect("height reply"), 1);

    drop(handle);
    runner
        .await
        .expect("service exits after command channel closes");
}

#[tokio::test]
async fn handle_import_block_reports_durable_tip_after_finalized_delivery_failure() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;

    let fail_delivery = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let system = Arc::new(FailingFinalizedDeliveryContext {
        snapshot: Arc::new(neo_storage::DataCache::new(false)),
        settings: Arc::new(settings.clone()),
        fail_delivery: Arc::clone(&fail_delivery),
    });
    let ledger = Arc::new(LedgerContext::default());
    let (service, handle) = BlockchainService::with_defaults(
        system,
        Arc::clone(&ledger),
        Arc::new(HeaderCache::default()),
        Arc::new(TestMempool),
    );
    service.initialize().await.expect("initialize");

    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_primary_index(0);
    header.set_next_consensus(*genesis.header.next_consensus());
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    header.witness = sign_header_for_test(
        &header,
        settings.network,
        &private_key,
        verification.as_slice(),
    );
    let block = Block::from_parts(header, Vec::new());
    let expected_tip = neo_runtime::ImportedTip::from_block(&block).expect("tip");

    fail_delivery.store(true, Ordering::SeqCst);
    let runner = tokio::spawn(service.run());
    let outcome = handle
        .import_block(block)
        .await
        .expect("durable import outcome");

    assert_eq!(
        outcome,
        neo_runtime::BlockImportOutcome::Imported(expected_tip)
    );
    assert_eq!(ledger.current_height(), 1);
    drop(handle);
    runner.await.expect("fatal finalized failure stops service");
}

/// Sync and consensus should be able to use the shared runtime import
/// contract without depending on the concrete blockchain command enum.
#[tokio::test]
async fn handle_implements_runtime_block_import_contract() {
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key =
        neo_crypto::Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    let point = neo_crypto::ECPoint::from_bytes(&public_key).expect("point");
    let mut settings = neo_config::ProtocolSettings::default();
    settings.standby_committee = vec![point.clone()];
    settings.validators_count = 1;
    let network = settings.network;

    let (service, handle, _snapshot) = store_fixture_with(settings.clone());
    service.initialize().await.expect("initialize");
    let runner = tokio::spawn(service.run());

    let genesis = crate::native_persist::genesis_block(&settings).expect("genesis");
    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(genesis.hash());
    header.set_timestamp(genesis.header.timestamp() + 15_000);
    header.set_primary_index(0);
    header.set_next_consensus(*genesis.header.next_consensus());
    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&header.hash().to_bytes());
    let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, &private_key).expect("sign");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            1,
            &[point],
        )
        .expect("multisig script");
    let mut invocation = vec![0x0C, 64];
    invocation.extend_from_slice(&signature);
    header.witness = neo_payloads::Witness::new_with_scripts(invocation, verification);
    let block = Block::from_parts(header, vec![]);
    let expected_tip = neo_runtime::ImportedTip::from_block(&block).expect("block hash");

    neo_runtime::BlockImport::check(&handle, &block)
        .await
        .expect("check");
    let outcome = neo_runtime::BlockImport::import(&handle, block, neo_runtime::BlockOrigin::Sync)
        .await
        .expect("import through runtime contract");

    assert_eq!(
        outcome,
        neo_runtime::BlockImportOutcome::Imported(expected_tip)
    );

    drop(handle);
    runner
        .await
        .expect("service exits after command channel closes");
}
