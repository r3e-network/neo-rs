use super::*;
use crate::command::BlockchainCommand;
use crate::fill_memory_pool::FillMemoryPool;
use crate::handle::BlockchainHandle;
use crate::header_cache::HeaderCache;
use crate::ledger_context::LedgerContext;
use crate::pipeline::signature_verification::{
    SignatureVerificationPool, SignatureVerificationPoolConfig,
};
use crate::relay_result::RelayResult;
use crate::service::MempoolLike;
use crate::service_context::SystemContext;
use crate::{Import, ImportMode};
use neo_payloads::Block;
use neo_payloads::InventoryType;
use neo_payloads::Transaction;
use neo_payloads::header::Header;
use neo_primitives::UInt256;
use neo_primitives::verify_result::VerifyResult;
use neo_serialization::BinarySerializer;
use neo_storage::StorageKey;
use neo_vm::ExecutionEngineLimits;
use num_bigint::BigInt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::oneshot;

type NativeProviderArc = Arc<neo_native_contracts::StandardNativeProvider>;

fn standard_native_provider() -> NativeProviderArc {
    Arc::new(neo_native_contracts::StandardNativeProvider::new())
}

fn sign_header_for_test(
    header: &Header,
    network: u32,
    private_key: &[u8; 32],
    verification: &[u8],
) -> neo_payloads::Witness {
    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&header.hash().to_bytes());
    let signature = neo_crypto::Secp256r1Crypto::sign(&sign_data, private_key).expect("sign");
    let mut invocation = vec![0x0C, 64];
    invocation.extend_from_slice(&signature);
    neo_payloads::Witness::new_with_scripts(invocation, verification.to_vec())
}

#[derive(Debug)]
struct TestContext;
impl SystemContext for TestContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::new(neo_config::ProtocolSettings::default())
    }
    fn current_height(&self) -> u32 {
        0
    }
}

#[derive(Debug, Default)]
struct TestMempool;
impl MempoolLike for TestMempool {
    fn try_add<B: neo_storage::CacheRead>(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }

    fn try_add_cached<B: neo_storage::CacheRead>(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _settings: &neo_config::ProtocolSettings,
        _cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }
}

#[derive(Debug)]
struct FixedResultMempool {
    result: VerifyResult,
}
impl MempoolLike for FixedResultMempool {
    fn try_add<B: neo_storage::CacheRead>(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        self.result
    }

    fn try_add_cached<B: neo_storage::CacheRead>(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _settings: &neo_config::ProtocolSettings,
        _cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult {
        self.result
    }
}

#[derive(Debug)]
struct RecordingMempool {
    reverify_calls: Arc<AtomicUsize>,
    has_unverified_transactions: bool,
    block_persisted_calls: Option<Arc<AtomicUsize>>,
}
impl MempoolLike for RecordingMempool {
    fn try_add<B: neo_storage::CacheRead>(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _settings: &neo_config::ProtocolSettings,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }

    fn try_add_cached<B: neo_storage::CacheRead>(
        &self,
        _tx: &Transaction,
        _snapshot: &neo_storage::DataCache<B>,
        _settings: &neo_config::ProtocolSettings,
        _cached_state_independent: Option<VerifyResult>,
    ) -> VerifyResult {
        VerifyResult::Succeed
    }

    fn has_unverified_transactions(&self) -> bool {
        self.has_unverified_transactions
    }

    fn block_persisted(&self, _block: &Block) {
        if let Some(calls) = &self.block_persisted_calls {
            calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn reverify_top_unverified<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _max_count: usize,
    ) -> bool {
        self.reverify_calls.fetch_add(1, Ordering::SeqCst);
        false
    }
}

#[test]
fn verified_import_pipeline_uses_explicit_native_providers() {
    let source = include_str!("../../handlers/verification.rs");
    let import_source = include_str!("../../handlers/import/verification.rs");
    let helper_start = source
        .find("fn verify_import_block_with_pipeline")
        .expect("verified import helper exists");
    let helper_end = source[helper_start..]
        .find("fn ensure_block_matches_cached_header")
        .map(|offset| helper_start + offset)
        .expect("next handler helper follows verified import helper");
    let helper = &source[helper_start..helper_end];
    assert!(
        helper
            .contains("VerifiedImportPipeline::<S::NativeProvider, S::CacheBacking>::verify_block"),
        "verified imports must route through the high-level verified import pipeline with the concrete provider type"
    );

    let import_start = import_source
        .find("pub(crate) fn verify_import_block_for_command")
        .expect("verified import command helper exists");
    let import_end = import_source[import_start..]
        .find("if let Err(error) = verify_result")
        .map(|offset| import_start + offset)
        .expect("verification result handling follows provider selection");
    let branch = &import_source[import_start..import_end];

    assert!(
        branch.contains("resources.native_persist.provider()"),
        "batch verified import must pass the provider captured in BatchPersistResources directly"
    );
    assert!(
        branch.contains("self.system.native_contract_provider()"),
        "store-backed verified import must pass the provider exposed by SystemContext"
    );
    assert!(
        branch.contains("native contract provider unavailable for block validation"),
        "store-backed verified import must fail clearly instead of passing an optional provider fallback"
    );
}

#[test]
fn store_fallback_reads_use_system_context_provider_boundary() {
    let source = include_str!("../../service/service/store_reads.rs");
    assert!(
        source.contains("self.system") && source.contains(".ledger_provider("),
        "store fallback reads should use the provider selected by SystemContext"
    );
    assert!(
        !source.contains("StorageLedgerProviderFactory"),
        "store fallback reads should not bypass the hot/cold provider boundary"
    );
    assert!(
        source.contains(".ok()") && source.contains(".flatten()"),
        "store fallback reads should preserve existing error-to-miss semantics"
    );
}

#[test]
fn store_header_verification_uses_system_native_provider() {
    let source = include_str!("../../handlers/verification.rs");
    let verifier_start = source
        .find("fn verify_consensus_witness_against_store")
        .expect("store-backed header verifier exists");
    let verifier_end = source[verifier_start..]
        .find("fn verify_consensus_witness_against_snapshot")
        .map(|offset| verifier_start + offset)
        .expect("snapshot verifier follows store verifier");
    let verifier = &source[verifier_start..verifier_end];

    assert!(
        verifier.contains("self.system.native_contract_provider()"),
        "store-backed header verification must use the provider exposed by SystemContext"
    );
    assert!(
        verifier.contains("verify_consensus_witness_against_snapshot_with_native_provider"),
        "store-backed header verification must route through the explicit-provider consensus-witness stage"
    );
}

#[test]
fn extensible_verification_uses_system_native_provider() {
    let source = include_str!("../../handlers/extensible.rs");
    let handler_start = source
        .find("pub(crate) async fn handle_extensible_inventory")
        .expect("extensible handler exists");
    let handler_end = source[handler_start..]
        .find("fn verify_extensible<")
        .map(|offset| handler_start + offset)
        .expect("extensible verifier follows handler");
    let handler = &source[handler_start..handler_end];
    assert!(
        handler.contains("self.system.native_contract_provider()"),
        "extensible payload verification must use the provider exposed by SystemContext"
    );
    assert!(
        !handler.contains("NativeExtensibleProviderFactory"),
        "extensible payload verification must not create a second production native provider factory"
    );

    let verifier_start = source
        .find("fn verify_extensible<")
        .expect("extensible verifier exists");
    let verifier = &source[verifier_start..];
    assert!(
        verifier.contains("verify_witness_with_native_provider"),
        "extensible payload verification must use the explicit-provider witness helper"
    );
    assert!(
        verifier.contains("ExtensibleNativeProvider"),
        "extensible payload whitelist reads must depend on a native read capability"
    );
    assert!(
        verifier.contains("native_contract_provider: Arc<S::NativeProvider>"),
        "extensible payload verification should preserve the SystemContext provider type"
    );
    assert!(
        handler.contains("self.system") && handler.contains(".ledger_provider("),
        "extensible payload height reads must use the provider selected by SystemContext"
    );
    assert!(
        !verifier.contains("StorageLedgerProviderFactory"),
        "extensible payload height reads should not bypass the hot/cold provider boundary"
    );
    assert!(
        !verifier.contains("LedgerContract::new()"),
        "extensible payload verification must not construct native LedgerContract directly"
    );
    assert!(
        !verifier.contains("NeoToken::new()"),
        "extensible payload verification must not construct native NEO directly"
    );
    assert!(
        !verifier.contains("RoleManagement::new()"),
        "extensible payload verification must not construct native RoleManagement directly"
    );

    let provider = include_str!("../../handlers/providers/extensible.rs");
    assert!(provider.contains("trait ExtensibleNativeProvider"));
    assert!(
        provider.contains("struct NativeExtensibleProvider<P>"),
        "extensible provider adapter should preserve the caller's concrete provider type"
    );
    assert!(
        provider.contains("native_contract_provider: Arc<P>"),
        "extensible provider adapter should own Arc<P>, not erase to dyn internally"
    );
    assert!(
        !provider.contains("trait ExtensibleNativeProviderFactory"),
        "extensible provider seam should adapt the node-composed NativeContractProvider instead of owning a private factory"
    );
    assert!(
        !provider.contains("NeoToken::new()"),
        "extensible provider seam must resolve NeoToken through the explicit native provider"
    );
    assert!(
        !provider.contains("RoleManagement::new()"),
        "extensible provider seam must resolve RoleManagement through the explicit native provider"
    );
    assert!(
        provider.contains(".committee_address(snapshot)"),
        "extensible provider seam should read the committee address from the explicit NativeContractProvider capability"
    );
    assert!(
        provider.contains(".next_block_validators(snapshot, settings)"),
        "extensible provider seam should read next-block validators from the explicit NativeContractProvider capability"
    );
    assert!(
        provider.contains(".state_validators(snapshot, height)"),
        "extensible provider seam should read StateValidator designations from the explicit NativeContractProvider capability"
    );
}

#[test]
fn transaction_admission_uses_system_native_provider() {
    let source = include_str!("../../handlers/transactions.rs");
    let conflict_start = source
        .find("fn persisted_conflict_exists")
        .expect("transaction conflict helper exists");
    let conflict_end = source[conflict_start..]
        .find("pub(crate) fn reverify_mempool_after_persist")
        .map(|offset| conflict_start + offset)
        .expect("mempool reverify helper follows conflict helper");
    let conflict_helper = &source[conflict_start..conflict_end];

    assert!(
        conflict_helper.contains("TransactionNativeProvider"),
        "persisted conflict checks should depend on a native read capability"
    );
    assert!(
        conflict_helper.contains("self.system.native_contract_provider()"),
        "persisted conflict checks should use the provider exposed by SystemContext"
    );
    assert!(
        !conflict_helper.contains("NativeTransactionProviderFactory"),
        "persisted conflict checks must not create a second production native provider factory"
    );
    assert!(
        conflict_helper.contains("NativeTransactionProvider::new(native_contract_provider)"),
        "persisted conflict checks should adapt the explicit native provider"
    );
    assert!(
        !conflict_helper.contains("PolicyContract::new()"),
        "transaction admission must not construct PolicyContract directly in the handler"
    );
    assert!(
        conflict_helper.contains("self.system.ledger_provider"),
        "persisted conflict checks should use the provider selected by SystemContext"
    );
    assert!(
        conflict_helper.contains("ledger_provider: &impl TransactionStateProvider"),
        "the conflict helper should depend on its narrow Ledger capability"
    );
    assert!(
        !source.contains("StorageLedgerProviderFactory"),
        "transaction admission ledger checks should not bypass the hot/cold provider boundary"
    );

    let provider_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/handlers/providers/transaction.rs");
    let provider = std::fs::read_to_string(&provider_path)
        .unwrap_or_else(|error| panic!("{}: {error}", provider_path.display()));
    assert!(provider.contains("trait TransactionNativeProvider"));
    assert!(
        provider.contains("struct NativeTransactionProvider<P>"),
        "transaction provider adapter should preserve the caller's concrete provider type"
    );
    assert!(
        provider.contains("native_contract_provider: Arc<P>"),
        "transaction provider adapter should own Arc<P>, not erase to dyn internally"
    );
    assert!(
        !provider.contains("trait TransactionNativeProviderFactory"),
        "transaction admission should adapt the node-composed NativeContractProvider instead of owning a private factory"
    );
    assert!(
        !provider.contains("PolicyContract::new()"),
        "transaction admission provider must resolve PolicyContract through the explicit native provider"
    );
    assert!(
        provider.contains(".max_traceable_blocks(snapshot, settings)"),
        "transaction admission provider should read MaxTraceableBlocks from the explicit NativeContractProvider capability"
    );
}

#[test]
fn header_inventory_verification_uses_system_native_provider() {
    let source = include_str!("../../handlers/headers.rs");
    let handler_start = source
        .find("pub(crate) fn handle_headers")
        .expect("header handler exists");
    let handler = &source[handler_start..];

    assert!(
        handler.contains("self.system.native_contract_provider()"),
        "header inventory verification must use the provider exposed by SystemContext"
    );
    assert!(
        handler.contains("verify_witness_with_native_provider"),
        "header inventory verification must use the explicit-provider witness helper"
    );
    assert!(
        handler.contains("self.system.ledger_provider"),
        "header inventory anchor reads should use the provider selected by SystemContext"
    );
    assert!(
        !handler.contains("StorageLedgerProviderFactory"),
        "header inventory anchor reads should not bypass the hot/cold provider boundary"
    );
    assert!(
        !handler.contains("LedgerContract::new()"),
        "header inventory handling must not construct native LedgerContract directly"
    );
}

#[test]
fn optimistic_header_verification_publishes_only_the_verified_prefix() {
    let (mut service, _handle, _snapshot) =
        store_fixture_with(neo_config::ProtocolSettings::default());
    let valid_witness =
        neo_payloads::Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH1.byte()]);
    let mut anchor = Header::new();
    anchor.set_index(0);
    anchor.set_timestamp(10);
    anchor.set_next_consensus(valid_witness.script_hash());
    assert!(service.header_cache.add(anchor.clone()));

    let mut valid = Header::new();
    valid.set_index(1);
    valid.set_prev_hash(anchor.hash());
    valid.set_timestamp(20);
    valid.witness = valid_witness.clone();

    let mut invalid = Header::new();
    invalid.set_index(2);
    invalid.set_prev_hash(valid.hash());
    invalid.set_timestamp(30);
    invalid.witness =
        neo_payloads::Witness::new_with_scripts(Vec::new(), vec![neo_vm::OpCode::PUSH0.byte()]);

    service.set_optimistic_signature_verification(Some(Arc::new(
        SignatureVerificationPool::new(SignatureVerificationPoolConfig {
            workers: 2,
            queue_capacity: 2,
        })
        .expect("pool"),
    )));

    let outcome = service.handle_headers(vec![valid.clone(), invalid]);
    assert_eq!(outcome.accepted, 1);
    assert_eq!(service.header_cache.hash_at(1), Some(valid.hash()));
    assert_eq!(service.header_cache.hash_at(2), None);
}

#[test]
fn initialize_uses_system_native_resources_for_genesis_persist() {
    let source = include_str!("../../handlers/initialize.rs");
    let start = source
        .find("pub(crate) async fn initialize")
        .expect("initialize handler exists");
    let initialize = &source[start..];

    assert!(
        initialize.contains("self.system.native_persist_resources()"),
        "genesis initialization must preserve resources composed by SystemContext"
    );
    assert!(
        initialize.contains("stage_block_natives_with_resources"),
        "genesis initialization must use the provider-aware native persistence entry point"
    );
}

fn fixture() -> (
    BlockchainService<TestContext, TestMempool>,
    BlockchainHandle,
) {
    let system = Arc::new(TestContext);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    BlockchainService::with_defaults(system, ledger, header_cache, mempool)
}

fn fixture_with_mempool_result(
    result: VerifyResult,
) -> (
    BlockchainService<TestContext, FixedResultMempool>,
    BlockchainHandle,
) {
    let system = Arc::new(TestContext);
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(FixedResultMempool { result });
    BlockchainService::with_defaults(system, ledger, header_cache, mempool)
}

/// [`SystemContext`] over a shared in-memory store snapshot, so the
/// native persistence pipeline actually runs.
struct StoreContext {
    snapshot: Arc<neo_storage::DataCache>,
    settings: Arc<neo_config::ProtocolSettings>,
    requires_replay_artifacts: bool,
    state_service: Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers>>,
    committing_application_executed_lengths: Option<Arc<parking_lot::Mutex<Vec<usize>>>>,
    committed_heights: Option<Arc<parking_lot::Mutex<Vec<u32>>>>,
    store_snapshot_calls: Option<Arc<AtomicUsize>>,
    commit_to_store_calls: Option<Arc<AtomicUsize>>,
}
impl std::fmt::Debug for StoreContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreContext").finish_non_exhaustive()
    }
}
impl SystemContext for StoreContext {
    type NativeProvider = neo_native_contracts::StandardNativeProvider;
    type CacheBacking = neo_storage::EmptyCacheBacking;

    fn settings(&self) -> Arc<neo_config::ProtocolSettings> {
        Arc::clone(&self.settings)
    }
    fn current_height(&self) -> u32 {
        0
    }
    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache>> {
        if let Some(calls) = &self.store_snapshot_calls {
            calls.fetch_add(1, Ordering::SeqCst);
        }
        Some(Arc::clone(&self.snapshot))
    }
    fn native_contract_provider(&self) -> Option<NativeProviderArc> {
        Some(standard_native_provider())
    }
    fn requires_replay_artifacts(
        &self,
        _block: &Block,
        _context: crate::BlockPersistContext,
    ) -> bool {
        self.requires_replay_artifacts
    }
    fn block_committing(
        &self,
        block: &Block,
        snapshot: &neo_storage::DataCache,
        _application_executed_list: &[neo_payloads::ApplicationExecuted],
    ) -> bool {
        if let Some(lengths) = &self.committing_application_executed_lengths {
            lengths.lock().push(_application_executed_list.len());
        }
        match &self.state_service {
            Some(handler) => handler.on_committing(block.index(), snapshot),
            None => true,
        }
    }
    fn commit_to_store(&self) -> Result<(), String> {
        if let Some(calls) = &self.commit_to_store_calls {
            calls.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
    ) -> crate::SyncBatchCommitPolicy {
        if self.state_service.is_none() && self.committing_application_executed_lengths.is_none() {
            crate::SyncBatchCommitPolicy::DeferredLive
        } else {
            crate::SyncBatchCommitPolicy::PerBlock
        }
    }
    async fn block_finalized(
        &self,
        finalized: crate::FinalizedBlock<Self::CacheBacking>,
    ) -> Result<(), String> {
        if let Some(heights) = &self.committed_heights {
            heights.lock().push(finalized.block().index());
        }
        Ok(())
    }
    fn allows_empty_block_fast_forward(&self) -> bool {
        self.state_service.is_none() && self.committing_application_executed_lengths.is_none()
    }
}

fn store_fixture() -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
) {
    store_fixture_with(neo_config::ProtocolSettings::default())
}

fn store_fixture_with(
    settings: neo_config::ProtocolSettings,
) -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
) {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(settings),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: None,
        committed_heights: None,
        store_snapshot_calls: None,
        commit_to_store_calls: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, handle, snapshot)
}

fn store_fixture_with_state_service() -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<neo_state_service::StateStore>,
) {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let state_store = Arc::new(neo_state_service::StateStore::with_mpt(true));
    let state_service = Arc::new(
        neo_state_service::commit_handlers::StateServiceCommitHandlers::new(Arc::clone(
            &state_store,
        )),
    );
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        requires_replay_artifacts: false,
        state_service: Some(state_service),
        committing_application_executed_lengths: None,
        committed_heights: None,
        store_snapshot_calls: None,
        commit_to_store_calls: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, handle, snapshot, state_store)
}

fn store_fixture_recording_application_executed_lengths() -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<parking_lot::Mutex<Vec<usize>>>,
) {
    store_fixture_recording_application_executed_lengths_with_policy(true)
}

fn store_fixture_recording_application_executed_lengths_with_policy(
    requires_replay_artifacts: bool,
) -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<parking_lot::Mutex<Vec<usize>>>,
) {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let lengths = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        requires_replay_artifacts,
        state_service: None,
        committing_application_executed_lengths: Some(Arc::clone(&lengths)),
        committed_heights: None,
        store_snapshot_calls: None,
        commit_to_store_calls: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, handle, snapshot, lengths)
}

fn store_fixture_counting_commits() -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<AtomicUsize>,
) {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let commit_calls = Arc::new(AtomicUsize::new(0));
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: None,
        committed_heights: None,
        store_snapshot_calls: None,
        commit_to_store_calls: Some(Arc::clone(&commit_calls)),
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, handle, snapshot, commit_calls)
}

fn store_fixture_recording_committed_heights() -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<parking_lot::Mutex<Vec<u32>>>,
) {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let committed_heights = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: None,
        committed_heights: Some(Arc::clone(&committed_heights)),
        store_snapshot_calls: None,
        commit_to_store_calls: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, handle, snapshot, committed_heights)
}

fn store_fixture_counting_snapshot_and_commits() -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    store_fixture_counting_snapshot_and_commits_with(neo_config::ProtocolSettings::default())
}

fn store_fixture_counting_snapshot_commits_and_committed_heights() -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    Arc<parking_lot::Mutex<Vec<u32>>>,
) {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let snapshot_calls = Arc::new(AtomicUsize::new(0));
    let commit_calls = Arc::new(AtomicUsize::new(0));
    let committed_heights = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: None,
        committed_heights: Some(Arc::clone(&committed_heights)),
        store_snapshot_calls: Some(Arc::clone(&snapshot_calls)),
        commit_to_store_calls: Some(Arc::clone(&commit_calls)),
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (
        service,
        handle,
        snapshot,
        snapshot_calls,
        commit_calls,
        committed_heights,
    )
}

fn store_fixture_counting_snapshot_and_commits_with(
    settings: neo_config::ProtocolSettings,
) -> (
    BlockchainService<StoreContext, TestMempool>,
    BlockchainHandle,
    Arc<neo_storage::DataCache>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let snapshot_calls = Arc::new(AtomicUsize::new(0));
    let commit_calls = Arc::new(AtomicUsize::new(0));
    let system = Arc::new(StoreContext {
        snapshot: Arc::clone(&snapshot),
        settings: Arc::new(settings),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: None,
        committed_heights: None,
        store_snapshot_calls: Some(Arc::clone(&snapshot_calls)),
        commit_to_store_calls: Some(Arc::clone(&commit_calls)),
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(TestMempool);
    let (service, handle) = BlockchainService::with_defaults(system, ledger, header_cache, mempool);
    (service, handle, snapshot, snapshot_calls, commit_calls)
}

/// NEO total supply read (NEP-17 `Prefix_TotalSupply` = 11).
fn neo_total_supply(snapshot: &neo_storage::DataCache) -> Option<num_bigint::BigInt> {
    snapshot
        .get(&neo_storage::StorageKey::new(
            neo_native_contracts::NeoToken::ID,
            vec![11],
        ))
        .map(|item| num_bigint::BigInt::from_signed_bytes_le(&item.value_bytes()))
}

fn transaction_with_nonce(nonce: u32) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![neo_vm::OpCode::PUSH1.byte()]);
    tx.set_system_fee(1_0000_0000);
    tx.set_signers(vec![neo_payloads::Signer::new(
        neo_primitives::UInt160::from_bytes(&[0x33; 20]).expect("test signer"),
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    tx
}

fn fund_test_signer_gas<B: neo_storage::CacheRead>(
    snapshot: &neo_storage::DataCache<B>,
    amount: i64,
) {
    let signer = neo_primitives::UInt160::from_bytes(&[0x33; 20]).expect("test signer");
    let mut gas_key = vec![20];
    gas_key.extend_from_slice(&signer.to_bytes());
    let account_state =
        neo_vm::StackItem::from_struct(vec![neo_vm::StackItem::from_int(BigInt::from(amount))]);
    let account_bytes =
        BinarySerializer::serialize(&account_state, &ExecutionEngineLimits::default())
            .expect("serialize GAS account");
    snapshot.add(
        StorageKey::new(neo_native_contracts::GasToken::ID, gas_key),
        neo_storage::StorageItem::from_bytes(account_bytes),
    );
    let supply_key = StorageKey::new(neo_native_contracts::GasToken::ID, vec![11]);
    let supply = snapshot
        .get(&supply_key)
        .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
        .unwrap_or_default()
        + BigInt::from(amount);
    snapshot.update(
        supply_key,
        neo_storage::StorageItem::from_bytes(supply.to_signed_bytes_le()),
    );
}

fn seed_current_ledger<B: neo_storage::CacheRead>(
    snapshot: &neo_storage::DataCache<B>,
    index: u32,
) {
    let hash = UInt256::from_bytes(&[0u8; 32]).expect("zero hash");
    let bytes = neo_native_contracts::LedgerContract::new()
        .serialize_hash_index_state(&hash, index)
        .expect("hash index state");
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
        neo_storage::StorageItem::from_bytes(bytes),
    );
}

fn seed_conflict_record<B: neo_storage::CacheRead>(
    snapshot: &neo_storage::DataCache<B>,
    hash: &UInt256,
    signer: &neo_primitives::UInt160,
    index: u32,
) {
    let stub = neo_native_contracts::LedgerContract::new()
        .serialize_conflict_stub(index)
        .expect("conflict stub");
    let mut bare_key = Vec::with_capacity(33);
    bare_key.push(11);
    bare_key.extend_from_slice(&hash.to_bytes());
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, bare_key),
        neo_storage::StorageItem::from_bytes(stub.clone()),
    );

    let mut signer_key = Vec::with_capacity(53);
    signer_key.push(11);
    signer_key.extend_from_slice(&hash.to_bytes());
    signer_key.extend_from_slice(&signer.to_bytes());
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, signer_key),
        neo_storage::StorageItem::from_bytes(stub),
    );
}

#[path = "handlers/block_flow.rs"]
mod block_flow;
#[path = "handlers/extensible_headers.rs"]
mod extensible_headers;
#[path = "handlers/sync_batch.rs"]
mod sync_batch;
#[path = "handlers/transactions.rs"]
mod transactions;

#[test]
fn dispatch_command_variants_is_exhaustive() {
    // The exhaustive match in `BlockchainService::dispatch` (in
    // `service/service/dispatch.rs`) is the real compile-time exhaustiveness
    // check. Any new variant added to `BlockchainCommand` will
    // fail to compile there until the dispatch arm is added. This
    // test documents that invariant and additionally verifies the
    // number of variants stays in sync with the dispatch arm
    // count, so accidental drift between documentation and
    // reality is caught by the test suite rather than discovered
    // by a panicked `unreachable!()` at runtime.
    use std::mem;

    // Helper that mirrors the dispatch arm order. It is
    // `unreachable!()`d because the test does not actually
    // invoke it; the function's job is to fail to compile when
    // the variant list drifts. The match has the same arm count
    // as the real dispatch in `service/service/dispatch.rs`.
    #[allow(dead_code, unreachable_code)]
    fn exhaustive_dispatch(_cmd: BlockchainCommand) -> std::convert::Infallible {
        match _cmd {
            BlockchainCommand::Import(_) => unreachable!(),
            BlockchainCommand::ImportBlocks { .. } => unreachable!(),
            BlockchainCommand::FillMemoryPool(_) => unreachable!(),
            BlockchainCommand::FillCompleted => unreachable!(),
            BlockchainCommand::Reverify(_) => unreachable!(),
            BlockchainCommand::ConsensusBlock { .. } => unreachable!(),
            BlockchainCommand::CheckedInventoryBlocks { .. } => unreachable!(),
            BlockchainCommand::ImportBlock { .. } => unreachable!(),
            BlockchainCommand::InventoryExtensible { .. } => unreachable!(),
            BlockchainCommand::PreverifyCompleted(_) => unreachable!(),
            BlockchainCommand::ValidateHeaders { .. } => unreachable!(),
            BlockchainCommand::Idle => unreachable!(),
            BlockchainCommand::DrainUnverified => unreachable!(),
            BlockchainCommand::RelayResult(_) => unreachable!(),
            BlockchainCommand::Initialize { .. } => unreachable!(),
            BlockchainCommand::Shutdown => unreachable!(),
            BlockchainCommand::AddTransaction { .. } => unreachable!(),
            BlockchainCommand::GetHeight { .. } => unreachable!(),
            BlockchainCommand::GetBlock { .. } => unreachable!(),
            BlockchainCommand::GetBlockByHeight { .. } => unreachable!(),
        }
    }

    // Build one of every reply-bearing variant so we can inspect
    // their discriminants. The four variants that need a
    // `Block`/`ExtensiblePayload`/`Transaction` field are not
    // constructed here; their discriminants are covered by the
    // static count assertion below.
    let (tx, _rx) = oneshot::channel();
    let _add_tx = BlockchainCommand::AddTransaction {
        transaction: neo_payloads::Transaction::new(),
        reply: tx,
    };
    let (ibtx, _ibrx) = oneshot::channel();
    let _import_block = BlockchainCommand::ImportBlock {
        block: Arc::new(Block::from_parts(Header::new(), vec![])),
        reply: ibtx,
    };
    let (batch_tx, _batch_rx) = oneshot::channel();
    let _import_blocks = BlockchainCommand::ImportBlocks {
        import: Import::default(),
        reply: batch_tx,
    };
    let (htx, _hrx) = oneshot::channel();
    let _get_height = BlockchainCommand::GetHeight { reply: htx };
    let (bhx, _bhx_rx) = oneshot::channel();
    let _get_block = BlockchainCommand::GetBlock {
        hash: UInt256::zero(),
        reply: bhx,
    };
    let (bhx2, _bhx2_rx) = oneshot::channel();
    let _get_block_h = BlockchainCommand::GetBlockByHeight {
        height: 0,
        reply: bhx2,
    };
    let (vtx, _vrx) = oneshot::channel();
    let _validate_headers = BlockchainCommand::ValidateHeaders {
        headers: vec![Header::new()],
        reply: vtx,
    };

    // Confirm each of the constructed variants has a unique
    // discriminant — a regression test against accidental
    // discriminator reuse.
    let mut seen = std::collections::HashSet::new();
    for cmd in [
        &_add_tx,
        &_import_block,
        &_import_blocks,
        &_get_height,
        &_get_block,
        &_get_block_h,
        &_validate_headers,
    ] {
        assert!(seen.insert(mem::discriminant(cmd)));
    }

    // The expected variant count must match the dispatch arm
    // count above. Bump this when adding a new variant and
    // add a corresponding arm in both `exhaustive_dispatch` and
    // `BlockchainService::dispatch` in `service.rs`.
    const EXPECTED_VARIANTS: usize = 21;
    assert!(seen.len() <= EXPECTED_VARIANTS);

    // Keep the helper symbol alive so the dispatch table is not
    // dead-code-eliminated by the compiler when running tests
    // with `cfg(test)`.
    let _ = exhaustive_dispatch as fn(BlockchainCommand) -> std::convert::Infallible;
}

#[test]
fn reverify_mempool_after_persist_skips_snapshot_when_no_unverified_transactions() {
    let snapshot = Arc::new(neo_storage::DataCache::new(false));
    let store_snapshot_calls = Arc::new(AtomicUsize::new(0));
    let reverify_calls = Arc::new(AtomicUsize::new(0));
    let system = Arc::new(StoreContext {
        snapshot,
        settings: Arc::new(neo_config::ProtocolSettings::default()),
        requires_replay_artifacts: true,
        state_service: None,
        committing_application_executed_lengths: None,
        committed_heights: None,
        store_snapshot_calls: Some(Arc::clone(&store_snapshot_calls)),
        commit_to_store_calls: None,
    });
    let ledger = Arc::new(LedgerContext::default());
    let header_cache = Arc::new(HeaderCache::default());
    let mempool = Arc::new(RecordingMempool {
        reverify_calls: Arc::clone(&reverify_calls),
        has_unverified_transactions: false,
        block_persisted_calls: None,
    });
    let (service, _handle) =
        BlockchainService::with_defaults(system, ledger, header_cache, mempool);

    assert!(!service.reverify_mempool_after_persist(0, 10));
    assert_eq!(store_snapshot_calls.load(Ordering::SeqCst), 0);
    assert_eq!(reverify_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn relay_result_broadcasts_non_extensible_failures() {
    let (service, handle) = fixture();
    let mut events = handle.subscribe();

    let result = RelayResult {
        hash: UInt256::from_bytes(&[0x41; 32]).expect("hash"),
        inventory_type: InventoryType::Transaction,
        block_index: None,
        result: VerifyResult::Invalid,
    };
    service.handle_relay_result(result).await;

    assert_eq!(
        events.try_recv().expect("relay result event"),
        crate::RuntimeEvent::RelayResult {
            hash: UInt256::from_bytes(&[0x41; 32]).expect("hash"),
            inventory_type: InventoryType::Transaction,
            block_index: None,
            result: VerifyResult::Invalid,
        },
        "ordinary failed relay results remain observable"
    );
}

#[tokio::test]
async fn relay_result_suppresses_failed_extensible_payloads() {
    let (service, handle) = fixture();
    let mut events = handle.subscribe();

    let result = RelayResult {
        hash: UInt256::from_bytes(&[0x42; 32]).expect("hash"),
        inventory_type: InventoryType::Extensible,
        block_index: None,
        result: VerifyResult::Invalid,
    };
    service.handle_relay_result(result).await;

    assert!(
        events.try_recv().is_err(),
        "C# v3.10.1 does not publish failed ExtensiblePayload relay results"
    );
}

#[test]
fn empty_fast_forward_run_collection_borrows_import_batch_blocks() {
    let source = include_str!("../../handlers/empty_fast_forward.rs");
    let collector = source
        .split("fn collect_empty_fast_forward_run")
        .nth(1)
        .and_then(|tail| tail.split("pub(super) fn persist_empty_block").next())
        .expect("collect_empty_fast_forward_run source");

    assert!(
        !collector.contains("Arc::new(block.clone())"),
        "empty-block fast-forward run collection must not clone full blocks from the import batch"
    );
}

#[test]
fn bulk_import_clones_blocks_only_after_empty_fast_forward_attempt() {
    let source = include_str!("../../handlers/import.rs");
    let empty_fast_forward_source = include_str!("../../handlers/import/empty_fast_forward.rs");
    let persist_source = include_str!("../../handlers/import/persist.rs");
    let handle_import = source
        .split("pub(crate) async fn handle_import")
        .nth(1)
        .and_then(|tail| tail.split("impl<S, M> BlockchainService").next())
        .expect("handle_import source");
    let fast_forward_delegate = handle_import
        .find("try_bulk_empty_fast_forward")
        .expect("bulk import attempts empty-block fast-forward");
    let persistence_delegate = handle_import
        .find("persist_import_block_for_command")
        .expect("normal persistence delegates to accepted-block helper");

    assert!(
        fast_forward_delegate < persistence_delegate,
        "bulk import should not clone a block before the empty-block fast-forward path can consume it by borrow"
    );
    assert!(
        !handle_import.contains("blocks[position].clone()"),
        "handle_import should not clone batch blocks directly before fast-forward decisions"
    );
    assert!(
        empty_fast_forward_source.contains("stage_empty_block_fast_forward"),
        "bulk empty-block helper must own the state-equivalent fast-forward attempt"
    );
    assert!(
        !empty_fast_forward_source.contains("Arc::new(block.clone())"),
        "bulk empty-block helper must borrow batch blocks instead of cloning them"
    );
    assert!(
        persist_source.contains("let block = Arc::new(block.clone());"),
        "normal persistence still needs an owned block inside the accepted-block helper"
    );
}
