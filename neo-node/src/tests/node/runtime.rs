use super::*;
use crate::node::context::CoordinatedNodeStoreWith;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::StoreDataCache;
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::persistence::{Store, StoreBackendKind, TransactionalStore};
use neo_system::NodeSystemContext;

struct TestDaemonContext<P, C = MemoryStore, S = MemoryStore, L = MemoryStore, T = MemoryStore>
where
    P: NativeContractProvider + 'static,
    C: TransactionalStore + CoordinatedNodeStoreWith<S> + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
{
    core: NodeSystemContext<P, C, DaemonCommitHooks<P, S, L, T, C>>,
    hooks: Arc<DaemonCommitHooks<P, S, L, T, C>>,
    finalized_stream: parking_lot::Mutex<
        Option<
            neo_system::FinalizedBlockStream<
                neo_storage::persistence::StoreCacheBacking<C>,
                crate::node::context::FinalizedProjectionConsumer<P, L, T>,
            >,
        >,
    >,
    shutdown: tokio_util::sync::CancellationToken,
}

impl<P, C, S, L, T> std::ops::Deref for TestDaemonContext<P, C, S, L, T>
where
    P: NativeContractProvider + 'static,
    C: TransactionalStore + CoordinatedNodeStoreWith<S> + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
{
    type Target = NodeSystemContext<P, C, DaemonCommitHooks<P, S, L, T, C>>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl<P, C, S, L, T> TestDaemonContext<P, C, S, L, T>
where
    P: NativeContractProvider + 'static,
    C: TransactionalStore + CoordinatedNodeStoreWith<S> + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
{
    fn block_committing_with_live_tip(
        &self,
        block: &neo_payloads::Block,
        snapshot: &StoreDataCache<C>,
        application_executed: &[neo_payloads::ApplicationExecuted],
        live_tip: u64,
    ) -> bool {
        self.hooks
            .block_committing_with_live_tip(block, snapshot, application_executed, live_tip)
    }

    fn spawn_finalized_stream(
        &self,
    ) -> tokio::task::JoinHandle<Result<(), neo_system::FinalizedBlockStreamError>> {
        let stream = self
            .finalized_stream
            .lock()
            .take()
            .expect("finalized stream has not already started");
        tokio::spawn(stream.run())
    }
}

fn daemon_context<P, C, S, L, T>(
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<StoreDataCache<C>>,
    store_cache: neo_storage::persistence::StoreCache<C>,
    state_service: Option<Arc<neo_state_service::commit_handlers::StateServiceCommitHandlers<S>>>,
    state_service_track_during_catchup: bool,
    indexer_service: Option<Arc<neo_indexer::IndexerService>>,
    native_contract_provider: Arc<P>,
    application_logs_service: Option<Arc<neo_rpc::application_logs::ApplicationLogsService<L>>>,
) -> TestDaemonContext<P, C, S, L, T>
where
    P: NativeContractProvider + 'static,
    C: TransactionalStore + CoordinatedNodeStoreWith<S> + 'static,
    S: Store + 'static,
    L: Store + 'static,
    T: Store + 'static,
{
    let shutdown = tokio_util::sync::CancellationToken::new();
    let (hooks, finalized_stream) = DaemonCommitHooks::<P, S, L, T, C>::compose(
        settings.network,
        state_service,
        state_service_track_during_catchup,
        indexer_service,
        application_logs_service,
        None,
        None,
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            shutdown.clone(),
        )),
    );
    let core = NodeSystemContext::new(
        settings,
        snapshot,
        store_cache,
        native_contract_provider,
        Arc::clone(&hooks),
    );
    TestDaemonContext {
        core,
        hooks,
        finalized_stream: parking_lot::Mutex::new(Some(finalized_stream)),
        shutdown,
    }
}

fn native_provider() -> Arc<neo_native_contracts::StandardNativeProvider> {
    Arc::new(neo_native_contracts::StandardNativeProvider::new())
}

fn memory_runtime_store() -> Arc<neo_storage::persistence::providers::RuntimeStore> {
    Arc::new(neo_storage::persistence::providers::RuntimeStore::Memory(
        MemoryStore::new(),
    ))
}

#[path = "runtime/indexer.rs"]
mod indexer;

#[test]
fn state_service_only_composition_skips_replay_artifact_copies() {
    use neo_blockchain::SystemContext;

    let settings = Arc::new(ProtocolSettings::default());
    let store = Arc::new(MemoryStore::new());
    let store_cache = neo_storage::persistence::StoreCache::new_from_store(store, false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let state_store = Arc::new(neo_state_service::StateStore::with_mpt(false));
    let state_service =
        Arc::new(neo_state_service::commit_handlers::StateServiceCommitHandlers::new(state_store));
    let context: TestDaemonContext<neo_native_contracts::StandardNativeProvider> = daemon_context(
        Arc::clone(&settings),
        snapshot,
        store_cache,
        Some(state_service),
        true,
        None,
        native_provider(),
        None,
    );
    let block = neo_blockchain::genesis_block(settings.as_ref()).expect("genesis block");

    assert!(
        !context.requires_replay_artifacts(&block, neo_blockchain::BlockPersistContext::live())
    );
}

#[test]
fn static_archive_fences_before_canonical_commit_and_recovers_an_ahead_failure() {
    use neo_blockchain::{
        BlockPersistContext, BlockProvider, NativePersistOptions, NativePersistResources,
        genesis_block, persist_block_natives_with_resources,
    };
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_system::BlockCommitHooks;

    type Hooks = DaemonCommitHooks<
        neo_native_contracts::StandardNativeProvider,
        MemoryStore,
        MemoryStore,
        MemoryStore,
    >;

    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(
        StoreCache::new_from_store(Arc::new(MemoryStore::new()), false)
            .data_cache()
            .clone(),
    );
    let provider = Arc::new(neo_native_contracts::StandardNativeProvider::new());
    let resources = NativePersistResources::from_provider(Arc::clone(&provider));
    let block = Arc::new(genesis_block(&settings).expect("genesis block"));
    let outcome = persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&block),
        Arc::new(settings.clone()),
        NativePersistOptions::default(),
        &resources,
    )
    .expect("stage finalized Ledger rows");
    let temp = tempfile::tempdir().expect("tempdir");
    let archive = neo_blockchain::StaticLedgerArchive::new(
        StaticFileArchiveFactory::default()
            .open(&temp.path().join("ledger.static"))
            .expect("archive"),
    );
    let shutdown = tokio_util::sync::CancellationToken::new();
    let hooks = Hooks::new(
        settings.network,
        None,
        false,
        None,
        None,
        Some(archive.clone()),
        Arc::new(crate::node::recovery::LocalReplayGuard::new(None, shutdown)),
    );

    assert!(<Hooks as BlockCommitHooks<MemoryStore>>::block_committing(
        &hooks,
        block.as_ref(),
        snapshot.as_ref(),
        &outcome.application_executed,
        0,
        BlockPersistContext::live(),
    ));
    assert_eq!(
        archive.tip(),
        None,
        "pre-commit capture must stay in memory"
    );
    <Hooks as BlockCommitHooks<MemoryStore>>::fence_precommit_durability(&hooks)
        .expect("durably stage archive before canonical commit");
    assert_eq!(
        archive.tip(),
        None,
        "the staged cold frame must remain hidden before canonical commit"
    );
    <Hooks as BlockCommitHooks<MemoryStore>>::canonical_commit_succeeded(&hooks);
    assert_eq!(archive.tip(), Some(0));
    assert_eq!(
        archive
            .provider()
            .block_by_index(0)
            .expect("archive read")
            .expect("genesis archived")
            .hash(),
        block.hash()
    );

    let second_temp = tempfile::tempdir().expect("tempdir");
    let discarded_archive = neo_blockchain::StaticLedgerArchive::new(
        StaticFileArchiveFactory::default()
            .open(&second_temp.path().join("ledger.static"))
            .expect("archive"),
    );
    let discarded = Hooks::new(
        settings.network,
        None,
        false,
        None,
        None,
        Some(discarded_archive.clone()),
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            tokio_util::sync::CancellationToken::new(),
        )),
    );
    assert!(<Hooks as BlockCommitHooks<MemoryStore>>::block_committing(
        &discarded,
        block.as_ref(),
        snapshot.as_ref(),
        &outcome.application_executed,
        0,
        BlockPersistContext::live(),
    ));
    <Hooks as BlockCommitHooks<MemoryStore>>::canonical_commit_failed(
        &discarded,
        "injected canonical failure",
    );
    <Hooks as BlockCommitHooks<MemoryStore>>::canonical_commit_succeeded(&discarded);
    assert_eq!(discarded_archive.tip(), None);

    let ahead_temp = tempfile::tempdir().expect("tempdir");
    let ahead_path = ahead_temp.path().join("ledger.static");
    let ahead_archive = neo_blockchain::StaticLedgerArchive::new(
        StaticFileArchiveFactory::default()
            .open(&ahead_path)
            .expect("archive"),
    );
    let ahead = Hooks::new(
        settings.network,
        None,
        false,
        None,
        None,
        Some(ahead_archive.clone()),
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            tokio_util::sync::CancellationToken::new(),
        )),
    );
    assert!(<Hooks as BlockCommitHooks<MemoryStore>>::block_committing(
        &ahead,
        block.as_ref(),
        snapshot.as_ref(),
        &outcome.application_executed,
        0,
        BlockPersistContext::live(),
    ));
    <Hooks as BlockCommitHooks<MemoryStore>>::fence_precommit_durability(&ahead)
        .expect("publish ahead archive frame");
    <Hooks as BlockCommitHooks<MemoryStore>>::canonical_commit_failed(
        &ahead,
        "injected failure after cold durability",
    );
    assert_eq!(
        ahead_archive.tip(),
        None,
        "failed canonical data must remain invisible until restart recovery"
    );
    drop(ahead);
    drop(ahead_archive);
    let recovered_archive = neo_blockchain::StaticLedgerArchiveFactory::default()
        .open(&ahead_path)
        .expect("recover staged archive suffix");
    assert_eq!(recovered_archive.tip(), Some(0));
    let empty_hot = neo_storage::DataCache::new(false);
    let recovery = recovered_archive
        .reconcile(&empty_hot, None, None, 64)
        .expect("truncate uncommitted ahead archive frame");
    assert_eq!(recovery.truncated_blocks, 1);
    assert_eq!(recovered_archive.tip(), None);
    assert!(!<Hooks as BlockCommitHooks<MemoryStore>>::allows_empty_block_fast_forward(&discarded));
    assert!(
        <Hooks as BlockCommitHooks<MemoryStore>>::allows_empty_block_committing_fast_forward(
            &discarded
        )
    );
}

#[test]
fn canonical_archive_publication_prunes_hot_ledger_rows_atomically() {
    use neo_blockchain::{
        BlockPersistContext, NativePersistOptions, NativePersistResources, genesis_block,
        persist_block_natives_with_resources,
    };
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};
    use neo_storage::StorageKey;
    use neo_storage::persistence::ReadOnlyStoreGeneric;
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::RuntimeStore;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::store::Store;
    use neo_system::BlockCommitHooks;

    type Hooks = DaemonCommitHooks<
        neo_native_contracts::StandardNativeProvider,
        MemoryStore,
        MemoryStore,
        MemoryStore,
    >;

    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(
        StoreCache::new_from_store(Arc::new(MemoryStore::new()), false)
            .data_cache()
            .clone(),
    );
    let provider = Arc::new(neo_native_contracts::StandardNativeProvider::new());
    let resources = NativePersistResources::from_provider(provider);
    let block = Arc::new(genesis_block(&settings).expect("genesis block"));
    let outcome = persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&block),
        Arc::new(settings.clone()),
        NativePersistOptions::default(),
        &resources,
    )
    .expect("stage genesis Ledger rows");

    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(RuntimeStore::Mdbx(
        neo_storage::mdbx::MdbxStoreProvider::new(
            neo_storage::persistence::storage::StorageConfig {
                path: temp.path().join("hot"),
                ..Default::default()
            },
        )
        .get_mdbx_store(std::path::Path::new(""))
        .expect("MDBX store"),
    ));
    assert!(
        store
            .try_commit_raw_overlay(&snapshot.extract_raw_changes())
            .expect("seed canonical store")
    );
    let archive = neo_blockchain::StaticLedgerArchive::new(
        StaticFileArchiveFactory::default()
            .open(&temp.path().join("ledger.static"))
            .expect("archive"),
    );
    let shutdown = tokio_util::sync::CancellationToken::new();
    let hooks = Hooks::new(
        settings.network,
        None,
        false,
        None,
        None,
        Some(archive.clone()),
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            shutdown.clone(),
        )),
    );
    hooks.configure_hot_ledger_pruning(Arc::clone(&store), 0);

    assert!(<Hooks as BlockCommitHooks<MemoryStore>>::block_committing(
        &hooks,
        block.as_ref(),
        snapshot.as_ref(),
        &outcome.application_executed,
        0,
        BlockPersistContext::live(),
    ));
    <Hooks as BlockCommitHooks<MemoryStore>>::fence_precommit_durability(&hooks)
        .expect("stage archive");
    <Hooks as BlockCommitHooks<MemoryStore>>::canonical_commit_succeeded(&hooks);

    let mut block_hash_key = vec![9];
    block_hash_key.extend_from_slice(&0u32.to_be_bytes());
    let block_hash_key = StorageKey::new(neo_native_contracts::LedgerContract::ID, block_hash_key);
    let current_block_key = StorageKey::new(
        neo_native_contracts::LedgerContract::ID,
        vec![neo_native_contracts::ledger_contract::storage::PREFIX_CURRENT_BLOCK],
    );
    assert!(store.try_get(&block_hash_key).is_none());
    assert!(
        store.try_get(&current_block_key).is_some(),
        "CurrentBlock must remain in the hot store"
    );
    assert_eq!(archive.hot_pruned_through(store.as_ref()).unwrap(), Some(0));
    assert_eq!(archive.tip(), Some(0));
    assert!(!shutdown.is_cancelled());
}

#[test]
fn daemon_commit_hooks_do_not_own_core_system_resources() {
    let source = include_str!("../../node/context/mod.rs");
    let system_context_source =
        include_str!("../../../../neo-system/src/composition/system_context.rs");

    assert!(
        source.contains("pub(super) struct DaemonCommitHooks<"),
        "the application layer should expose only its typed commit-hook policy"
    );
    assert!(
        !source.contains("\n    store_cache:")
            && !source.contains("\n    snapshot:")
            && !source.contains("\n    native_contract_provider:"),
        "application hooks may name the finalized cache backing but must not own core storage or native-provider mechanics"
    );
    assert!(
        system_context_source.contains("pub struct NodeSystemContext<P, S, H>")
            && system_context_source.contains("H: BlockCommitHooks<S>"),
        "neo-system must own the generic core context and static hook boundary"
    );
    assert!(
        system_context_source.contains("SystemContext for NodeSystemContext<P, S, H>"),
        "the composition layer must implement the blockchain SystemContext contract"
    );
}

#[test]
fn canonical_store_failure_requests_shutdown_without_precommit_observers() {
    use neo_blockchain::SystemContext;
    use neo_storage::persistence::StoreCache;

    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(chain_store, true);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::new(ProtocolSettings::default()),
        snapshot,
        store_cache,
        None,
        false,
        None,
        native_provider(),
        None,
    );

    ctx.commit_to_store()
        .expect_err("read-only canonical store must reject commit");

    assert!(
        ctx.shutdown.is_cancelled(),
        "canonical durability loss must stop the node even without auxiliary observers"
    );
}

#[test]
fn daemon_context_skips_state_service_mpt_during_default_cold_catchup() {
    use neo_blockchain::{BlockPersistContext, SystemContext};
    use neo_payloads::{Block, Header};
    use neo_state_service::{StateStore, commit_handlers::StateServiceCommitHandlers};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let state_store = Arc::new(StateStore::with_mpt(true));
    let state_service = Arc::new(StateServiceCommitHandlers::new(Arc::clone(&state_store)));
    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Some(state_service),
        false,
        None,
        native_provider(),
        None,
    );

    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing_with_live_tip(&block, &snapshot, &[], 1_000_000));
    assert_eq!(
        state_store
            .mpt()
            .expect("state store exposes MPT")
            .current_local_root_index(),
        None,
        "default cold catch-up should preserve fast-sync behavior by deferring StateService MPT"
    );

    let mut header = Header::new();
    header.set_index(1);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing_with_context(
        &block,
        &snapshot,
        &[],
        BlockPersistContext::trusted_replay(),
    ));
    assert_eq!(
        state_store
            .mpt()
            .expect("state store exposes MPT")
            .current_local_root_index(),
        None,
        "explicit bulk import should stay on the cold-catchup fast path before peers report a live tip"
    );
}

#[test]
fn daemon_context_can_track_state_service_mpt_during_cold_catchup_for_validation() {
    use neo_blockchain::{BlockPersistContext, SystemContext};
    use neo_payloads::{Block, Header};
    use neo_state_service::{StateStore, commit_handlers::StateServiceCommitHandlers};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let state_store = Arc::new(StateStore::with_mpt(true));
    let state_service = Arc::new(StateServiceCommitHandlers::new(Arc::clone(&state_store)));
    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        Some(state_service),
        true,
        None,
        native_provider(),
        None,
    );

    let mut header = Header::new();
    header.set_index(0);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing_with_live_tip(&block, &snapshot, &[], 1_000_000));
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(mpt.current_local_root_index(), Some(0));
    assert!(
        mpt.get_state_root(0).is_some(),
        "validation profiles must keep local state roots advancing during cold catch-up"
    );

    let mut header = Header::new();
    header.set_index(1);
    let block = Block::from_parts(header, Vec::new());

    assert!(ctx.block_committing_with_context(
        &block,
        &snapshot,
        &[],
        BlockPersistContext::trusted_replay(),
    ));
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(
        mpt.current_local_root_index(),
        Some(1),
        "validation bulk import must still advance StateService MPT roots"
    );
}

#[test]
fn state_service_store_opens_mdbx_through_factory() {
    let temp = tempfile::tempdir().expect("temp StateService root");
    let path = temp.path().join("StateRoot_{0}");
    let store = services::open_service_store_with_storage_config(
        "StateService",
        "mdbx",
        &super::config::StorageSection::default(),
        &path,
        0x334F_454E,
    )
    .expect("open MDBX StateService store");

    assert_eq!(
        store.backend_kind(),
        neo_storage::persistence::StoreBackendKind::Mdbx
    );
}

#[test]
fn state_service_durable_fence_publishes_mdbx_backing() {
    use neo_state_service::{StateStore, commit_handlers::StateServiceCommitHandlers};
    use neo_storage::{DataCache, StorageItem, StorageKey};

    let temp = tempfile::tempdir().expect("temp StateService root");
    let path = temp.path().join("StateRoot_{0}");
    let backing = services::open_service_store_with_storage_config(
        "StateService",
        "mdbx",
        &super::config::StorageSection::default(),
        &path,
        0x334F_454E,
    )
    .expect("open MDBX StateService store");
    let state_store =
        Arc::new(StateStore::with_mpt_store(false, Arc::clone(&backing)).expect("open MPT store"));
    let handlers = StateServiceCommitHandlers::new(Arc::clone(&state_store));
    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xAB]),
        StorageItem::from_bytes(vec![0x01]),
    );

    assert!(handlers.on_committing_deferred(0, &snapshot));

    handlers
        .flush_durable_result()
        .expect("StateService backing durability fence");

    assert_eq!(
        state_store.mpt().expect("MPT").current_local_root_index(),
        Some(0)
    );
}

#[test]
fn db_probe_replay_uses_explicit_native_provider() {
    let source = include_str!("../../bin/neo-db-probe.rs");
    let start = source
        .find("fn execute_transaction_probe")
        .expect("probe replay function exists");
    let end = source[start..]
        .find("fn trace_engine_frames")
        .map(|offset| start + offset)
        .expect("frame tracing helper follows probe replay");
    let replay = &source[start..end];

    assert!(replay.contains("new_with_shared_block_and_native_contract_provider"));
    assert!(replay.contains("StandardNativeProvider::new()"));
    assert!(!replay.contains("ApplicationEngine::new_with_shared_block("));
}

#[test]
fn db_probe_replay_uses_hot_cold_ledger_provider_boundary() {
    let source = include_str!("../../bin/neo-db-probe.rs");
    let replay_start = source
        .find("fn replay_transaction(")
        .expect("replay transaction helper exists");
    let replay_end = source[replay_start..]
        .find("fn replay_raw_transaction(")
        .map(|offset| replay_start + offset)
        .expect("raw replay helper follows replay transaction");
    let replay = &source[replay_start..replay_end];

    assert!(source.contains("HotColdLedgerProviderFactory"));
    assert!(source.contains("OptionalStaticLedgerProvider"));
    assert!(source.contains("open_offline_ledger_factory"));
    assert!(replay.contains("let ledger_factory = open_offline_ledger_factory"));
    assert!(!source.contains("StorageLedgerProviderFactory"));
}

#[test]
fn configured_mdbx_backend_is_used_for_service_stores() {
    let temp = tempfile::tempdir().expect("temp service store root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"

[state_service]
enabled = true
path = "{}"
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse mdbx service config");

    let canonical_path = temp.path().join("chain");
    let canonical_store = crate::node::config::open_store(&config, Some(&canonical_path))
        .expect("open canonical MDBX store");
    let services =
        services::build_operational_services(&config, 0x334F_454E, true, false, &canonical_store)
            .expect("build services");
    let state_store = services
        .durable_stores
        .first()
        .expect("state service durable store");

    assert_eq!(state_store.backend_kind(), StoreBackendKind::Mdbx);
    assert!(
        services
            .state_service
            .as_ref()
            .expect("state service")
            .is_coordinated(),
        "MDBX StateService must share the canonical transaction domain"
    );
}

#[test]
fn coordinated_mdbx_commit_publishes_ledger_and_state_root_atomically() {
    use neo_blockchain::{BlockPersistContext, SystemContext};
    use neo_storage::persistence::providers::RuntimeStore;
    use neo_storage::persistence::{RawReadOnlyStore, StoreCache};
    use neo_storage::{StorageItem, StorageKey};

    let temp = tempfile::tempdir().expect("temp MDBX root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"
data_dir = "{}"

[state_service]
enabled = true
track_during_catchup = true
"#,
        temp.path().join("chain").display(),
    ))
    .expect("parse coordinated MDBX config");
    let canonical_store =
        crate::node::config::open_store(&config, None).expect("open canonical MDBX store");
    let services =
        services::build_operational_services(&config, 0x334F_454E, true, true, &canonical_store)
            .expect("build coordinated services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service")
        .clone();
    assert!(state_service.is_coordinated());
    let state_backing = services
        .durable_stores
        .first()
        .expect("state backing")
        .clone();
    let store_cache = StoreCache::new_from_store(Arc::clone(&canonical_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let settings = Arc::new(ProtocolSettings::default());
    let genesis = Arc::new(
        neo_blockchain::genesis_block(settings.as_ref()).expect("construct coordinated genesis"),
    );
    let resources = neo_blockchain::NativePersistResources::from_provider(native_provider());
    neo_blockchain::persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&genesis),
        Arc::clone(&settings),
        neo_blockchain::NativePersistOptions::default(),
        &resources,
    )
    .expect("persist coordinated genesis transition");
    let contract_key = StorageKey::new(5, vec![0xAA]);
    snapshot.add(contract_key.clone(), StorageItem::from_bytes(vec![0x01]));
    let marker = temp.path().join(".neo-local-replay-poisoned");
    let shutdown = tokio_util::sync::CancellationToken::new();
    let (hooks, _stream) = DaemonCommitHooks::<
        neo_native_contracts::StandardNativeProvider,
        RuntimeStore,
        MemoryStore,
        MemoryStore,
        RuntimeStore,
    >::compose(
        0x334F_454E,
        Some(state_service),
        true,
        None,
        None,
        None,
        None,
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            Some(marker.clone()),
            shutdown,
        )),
    );
    let context = NodeSystemContext::new(
        settings,
        Arc::clone(&snapshot),
        store_cache,
        native_provider(),
        hooks,
    );

    assert!(context.block_committing_with_context(
        genesis.as_ref(),
        &snapshot,
        &[],
        BlockPersistContext::trusted_replay(),
    ));
    assert!(!marker.exists());
    assert_eq!(
        canonical_store.try_get_bytes(&contract_key.to_array()),
        None
    );
    assert_eq!(
        state_backing.try_get_bytes(neo_state_service::Keys::CURRENT_LOCAL_ROOT_INDEX),
        None
    );
    let transaction_before = canonical_store
        .as_mdbx()
        .expect("canonical MDBX")
        .info()
        .expect("MDBX info before")
        .last_txnid();

    context.commit_to_store().expect("coordinated commit");

    assert_eq!(
        canonical_store.try_get_bytes(&contract_key.to_array()),
        Some(vec![0x01])
    );
    assert_eq!(
        state_backing.try_get_bytes(neo_state_service::Keys::CURRENT_LOCAL_ROOT_INDEX),
        Some(0u32.to_le_bytes().to_vec())
    );
    assert_eq!(
        canonical_store
            .as_mdbx()
            .expect("canonical MDBX")
            .info()
            .expect("MDBX info after")
            .last_txnid(),
        transaction_before + 1
    );
    assert!(!marker.exists());

    drop(context);
    drop(snapshot);
    drop(state_backing);
    drop(services);
    drop(canonical_store);
    crate::node::config::validate_storage(&config, None, 0x334F_454E)
        .expect("coordinated genesis must pass startup validation after reopen");
}

#[test]
fn service_stores_inherit_primary_mdbx_storage_config() {
    let temp = tempfile::tempdir().expect("temp service store root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"
mdbx_geometry_upper_gb = 1
mdbx_geometry_growth_mb = 16
mdbx_max_readers = 128

[state_service]
enabled = true
path = "{}"
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse mdbx service config");

    let canonical_path = temp.path().join("chain");
    let canonical_store = crate::node::config::open_store(&config, Some(&canonical_path))
        .expect("open canonical MDBX store");
    let services =
        services::build_operational_services(&config, 0x334F_454E, true, false, &canonical_store)
            .expect("build services");
    let service_store = services
        .durable_stores
        .first()
        .expect("state service durable store");
    let mdbx = service_store
        .mdbx_environment_info()
        .expect("service store exposes MDBX info")
        .expect("MDBX info");

    assert_eq!(
        mdbx.map_size,
        1024 * 1024 * 1024,
        "service stores must inherit MDBX geometry from [storage]"
    );
}

#[test]
fn operational_state_service_store_uses_mdbx_for_normal_restart() {
    let temp = tempfile::tempdir().expect("temp StateService root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"

[state_service]
enabled = true
path = "{}"
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse state-service config");

    let services = services::build_operational_services(
        &config,
        0x334F_454E,
        true,
        false,
        &memory_runtime_store(),
    )
    .expect("build services");
    let state_store = services
        .durable_stores
        .first()
        .expect("state service durable store");
    assert_eq!(
        state_store.backend_kind(),
        neo_storage::persistence::StoreBackendKind::Mdbx
    );
    assert!(
        !services
            .state_service
            .as_ref()
            .expect("state service enabled")
            .is_async(),
        "normal replay should keep StateService MPT failures synchronous"
    );
}

#[test]
fn fast_sync_state_service_uses_ordered_async_mpt_worker() {
    let temp = tempfile::tempdir().expect("temp StateService root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"

[state_service]
enabled = true
path = "{}"
track_during_catchup = true
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse state-service config");

    let services = services::build_operational_services(
        &config,
        0x334F_454E,
        true,
        true,
        &memory_runtime_store(),
    )
    .expect("build fast-sync services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled");

    assert!(
        state_service.is_async(),
        "fast-sync validation should overlap native persistence with ordered StateService MPT writes"
    );
    assert_eq!(
        state_service.async_queue_capacity(),
        Some(4096),
        "fast-sync validation should absorb one burst window of ordered StateService MPT work"
    );
}

#[test]
fn daemon_context_bulk_sync_flush_reports_async_state_service_failure() {
    use neo_blockchain::{BlockPersistContext, SystemContext};
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::{StorageItem, StorageKey};

    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
full_state = true
track_during_catchup = true
"#,
    )
    .expect("parse validation state-service config");
    let services = services::build_operational_services(
        &config,
        0x334F_454E,
        true,
        true,
        &memory_runtime_store(),
    )
    .expect("build fast-sync validation services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled")
        .clone();
    assert!(
        state_service.is_async(),
        "fixture must exercise the async StateService worker"
    );

    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let ctx: TestDaemonContext<_, MemoryStore, neo_storage::persistence::providers::RuntimeStore> =
        daemon_context(
            Arc::new(ProtocolSettings::default()),
            Arc::clone(&snapshot),
            store_cache,
            Some(state_service),
            true,
            None,
            native_provider(),
            None,
        );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        ctx.block_committing_with_context(
            &block,
            &snapshot,
            &[],
            BlockPersistContext::trusted_replay(),
        ),
        "async StateService enqueue succeeds before the worker observes the non-contiguous root"
    );
    let err = ctx
        .flush_deferred_commit_handlers()
        .expect_err("deferred finalization must surface async StateService worker failure");
    assert!(
        err.contains("state-root worker"),
        "unexpected flush error: {err}"
    );
    assert!(
        ctx.shutdown.is_cancelled(),
        "a failed deferred StateService publication must stop local replay"
    );
}

#[test]
fn daemon_context_live_async_state_service_failure_is_immediate() {
    use neo_blockchain::{BlockPersistContext, SystemContext};
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::{StorageItem, StorageKey};

    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
full_state = true
track_during_catchup = true
"#,
    )
    .expect("parse validation state-service config");
    let services = services::build_operational_services(
        &config,
        0x334F_454E,
        true,
        true,
        &memory_runtime_store(),
    )
    .expect("build fast-sync validation services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled")
        .clone();
    assert!(
        state_service.is_async(),
        "fixture must exercise the async StateService worker"
    );

    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let ctx: TestDaemonContext<_, MemoryStore, neo_storage::persistence::providers::RuntimeStore> =
        daemon_context(
            Arc::new(ProtocolSettings::default()),
            Arc::clone(&snapshot),
            store_cache,
            Some(state_service),
            true,
            None,
            native_provider(),
            None,
        );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        !ctx.block_committing_with_context(&block, &snapshot, &[], BlockPersistContext::live()),
        "live async StateService must fail before chain commit when MPT roots are non-contiguous"
    );
    assert!(
        ctx.shutdown.is_cancelled(),
        "a failed live StateService publication must request node shutdown"
    );
}

#[test]
fn fast_sync_state_service_without_catchup_tracking_stays_synchronous() {
    let temp = tempfile::tempdir().expect("temp StateService root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"

[state_service]
enabled = true
path = "{}"
track_during_catchup = false
"#,
        temp.path().join("StateRoot_{{0}}").display(),
    ))
    .expect("parse state-service config");

    let services = services::build_operational_services(
        &config,
        0x334F_454E,
        true,
        true,
        &memory_runtime_store(),
    )
    .expect("build fast-sync services");
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled");

    assert!(
        !state_service.is_async(),
        "bulk import alone should not pay async StateService overhead when catch-up MPT tracking is disabled"
    );

    let state_store = services
        .durable_stores
        .first()
        .expect("state service durable store");
    assert_eq!(
        state_store.backend_kind(),
        neo_storage::persistence::StoreBackendKind::Mdbx
    );
}

#[test]
fn validation_state_service_reports_non_contiguous_root_before_chain_commit() {
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::{StorageItem, StorageKey};

    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
full_state = true
track_during_catchup = true
"#,
    )
    .expect("parse validation state-service config");
    let services = services::build_operational_services(
        &config,
        0x334F_454E,
        true,
        false,
        &memory_runtime_store(),
    )
    .expect("build validation services");
    let state_store = services
        .state_store
        .as_ref()
        .expect("state store enabled")
        .clone();
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled")
        .clone();

    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let ctx: TestDaemonContext<_, MemoryStore, neo_storage::persistence::providers::RuntimeStore> =
        daemon_context(
            Arc::new(ProtocolSettings::default()),
            Arc::clone(&snapshot),
            store_cache,
            Some(state_service),
            true,
            None,
            native_provider(),
            None,
        );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        !ctx.block_committing_with_live_tip(&block, &snapshot, &[], 1_000_000),
        "validation StateService must fail before chain commit when MPT roots would become non-contiguous"
    );
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(mpt.current_local_root_index(), None);
    assert!(mpt.get_state_root(5).is_none());
}

#[test]
fn default_state_service_reports_near_tip_root_failure_before_chain_commit() {
    use neo_payloads::{Block, Header};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::{StorageItem, StorageKey};

    let config: NodeConfig = toml::from_str(
        r#"
[state_service]
enabled = true
full_state = true
"#,
    )
    .expect("parse default state-service config");
    let services = services::build_operational_services(
        &config,
        0x334F_454E,
        true,
        false,
        &memory_runtime_store(),
    )
    .expect("build services");
    let state_store = services
        .state_store
        .as_ref()
        .expect("state store enabled")
        .clone();
    let state_service = services
        .state_service
        .as_ref()
        .expect("state service enabled")
        .clone();

    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    snapshot.add(
        StorageKey::new(5, vec![0xBB]),
        StorageItem::from_bytes(vec![0x02]),
    );
    let ctx: TestDaemonContext<_, MemoryStore, neo_storage::persistence::providers::RuntimeStore> =
        daemon_context(
            Arc::new(ProtocolSettings::default()),
            Arc::clone(&snapshot),
            store_cache,
            Some(state_service),
            false,
            None,
            native_provider(),
            None,
        );

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    assert!(
        !ctx.block_committing_with_live_tip(&block, &snapshot, &[], 0),
        "when default StateService tracking is active, MPT failures must stop chain commit"
    );
    let mpt = state_store.mpt().expect("state store exposes MPT");
    assert_eq!(mpt.current_local_root_index(), None);
    assert!(mpt.get_state_root(5).is_none());
}

#[test]
fn flush_durable_stores_accepts_chain_and_service_mdbx() {
    use neo_storage::mdbx::MdbxStoreProvider;
    use neo_storage::persistence::storage::StorageConfig;

    let temp = tempfile::tempdir().expect("temp store root");
    let chain_cfg = StorageConfig {
        path: temp.path().join("chain"),
        ..Default::default()
    };
    let chain_store = Arc::new(
        MdbxStoreProvider::new(chain_cfg)
            .get_mdbx_store("")
            .expect("chain MDBX store"),
    );

    let state_path = temp.path().join("StateRoot_{0}");
    let service_store = services::open_service_store_with_storage_config(
        "StateService",
        "mdbx",
        &super::config::StorageSection::default(),
        &state_path,
        0x334F_454E,
    )
    .expect("open StateService MDBX store");

    flush_durable_stores(chain_store.as_ref(), &[Arc::clone(&service_store)])
        .expect("flush durable stores");

    assert_eq!(
        chain_store.backend_kind(),
        neo_storage::persistence::StoreBackendKind::Mdbx
    );
    assert_eq!(
        service_store.backend_kind(),
        neo_storage::persistence::StoreBackendKind::Mdbx
    );
}

#[tokio::test]
async fn build_node_rejects_missing_state_root_store_for_populated_chain() {
    const DURABLE_TIP: u32 = 1;

    let temp = tempfile::tempdir().expect("temp MDBX root");
    let storage_path = temp.path().join("chain");
    let settings = Arc::new(ProtocolSettings::default());
    seed_mdbx_tip(&storage_path, settings.as_ref(), DURABLE_TIP).expect("seed durable MDBX tip");

    let mut config: NodeConfig = toml::from_str(
        r#"
[storage]
backend = "mdbx"
"#,
    )
    .expect("parse MDBX storage config");
    config.state_service.enabled = true;

    let err = match build_node(
        Arc::clone(&settings),
        &config,
        Some(&storage_path),
        None,
        LedgerMode::Local,
        false,
        None,
    )
    .await
    {
        Ok(running) => {
            running.abort_for_test().await;
            panic!("missing StateRoot store must abort local node startup");
        }
        Err(err) => err,
    };

    assert!(
        err.to_string().contains("no current local root"),
        "startup error should name StateRoot parity failure: {err:#}"
    );
}

#[tokio::test]
async fn remote_ledger_node_build_does_not_register_local_replay_services() {
    let temp = tempfile::tempdir().expect("temp stores");
    let settings = Arc::new(ProtocolSettings::default());
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[state_service]
enabled = true
path = "{}"

[indexer]
enabled = true
store_path = "{}"

[application_logs]
enabled = true
path = "{}"

[tokens_tracker]
enabled = true
db_path = "{}"
"#,
        temp.path().join("StateRoot_{{0}}").display(),
        temp.path().join("NeoIndexer_{{0}}").display(),
        temp.path().join("ApplicationLogs_{{0}}").display(),
        temp.path().join("TokensTracker_{{0}}").display(),
    ))
    .expect("parse service config");

    let running = build_node(
        Arc::clone(&settings),
        &config,
        Some(&temp.path().join("chain")),
        None,
        LedgerMode::RemoteRpc {
            endpoint: "https://rpc.example.invalid",
        },
        false,
        None,
    )
    .await
    .expect("build remote-ledger node");

    assert!(
        running.services().state_store().is_none(),
        "remote-ledger mode must not build a local StateService store"
    );
    assert!(
        running.services().state_commit_handlers().is_none(),
        "remote-ledger mode must not start local StateService replay"
    );
    assert!(
        running.services().indexer().is_none(),
        "remote-ledger mode must not start local indexer replay"
    );
    assert!(
        running.services().application_logs().is_none(),
        "remote-ledger mode must not start local application-log replay"
    );
    assert!(
        running.services().tokens_tracker().is_none(),
        "remote-ledger mode must not start local token tracker replay"
    );
    assert!(
        running.services().remote_ledger().is_some(),
        "remote-ledger mode must expose the remote-ledger status in explicit runtime composition"
    );
    assert!(
        running.node().storage().backend_kind() == StoreBackendKind::Memory,
        "remote-ledger mode should use an ephemeral chain context instead of opening the configured local ledger"
    );
    assert!(
        !temp.path().join("chain").exists(),
        "remote-ledger mode must not create a local chain MDBX directory"
    );
    assert!(
        neo_native_contracts::LedgerContract::new()
            .current_index(running.node().store_cache().data_cache())
            .is_err(),
        "remote-ledger mode must not initialize even an ephemeral canonical ledger"
    );

    running.abort_for_test().await;
}

#[tokio::test]
async fn remote_ledger_node_advertises_upstream_height_when_available() {
    const REMOTE_BLOCK_COUNT: u32 = 42;

    let temp = tempfile::tempdir().expect("temp stores");
    let settings = Arc::new(ProtocolSettings::default());
    let endpoint = serve_rpc_once("getblockcount", serde_json::json!(REMOTE_BLOCK_COUNT));
    let running = build_node(
        Arc::clone(&settings),
        &NodeConfig::default(),
        Some(&temp.path().join("chain")),
        None,
        LedgerMode::RemoteRpc {
            endpoint: endpoint.as_str(),
        },
        false,
        None,
    )
    .await
    .expect("build remote-ledger node");

    running
        .network()
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start P2P listener");
    let port = running.network().local_node_info().port();
    let mut fake = fake_dial(port).await;
    let node_version = recv_frame(&mut fake).await;
    assert_eq!(node_version.command, MessageCommand::Version);
    let node_version: VersionPayload = decode_payload(&node_version);
    assert!(
        node_version.capabilities.iter().any(|capability| matches!(
            capability,
            NodeCapability::FullNode {
                start_height: height
            } if *height == REMOTE_BLOCK_COUNT - 1
        )),
        "remote-ledger mode should advertise the upstream RPC tip height"
    );

    running.abort_for_test().await;
}

/// Reproduces the v3.10.1 consistency testnet failure
/// (`Policy_getExecFeeFactor`: Python 30 vs local node 0).
///
/// The live RPC `invokefunction(Policy.getExecFeeFactor)` path builds its
/// engine over a FRESH `system.store_cache()` (see Session::new), NOT over
/// the genesis-persist shared snapshot. So this test runs the genesis
/// native-persist pipeline into the shared snapshot, commits via
/// `commit_to_store`, then reads Policy storage through a brand-new
/// `store_cache` view — exactly the live read path. If the genesis
/// `ExecFeeFactor=30` write is visible there, the live node must return 30.
#[test]
fn genesis_policy_init_visible_through_fresh_store_cache_after_commit() {
    use neo_blockchain::SystemContext;
    use neo_blockchain::native_persist::{
        NativePersistOptions, NativePersistResources, chain_state_initialized, genesis_block,
        persist_block_natives_with_resources,
    };
    use neo_native_contracts::PolicyContract;
    use neo_storage::StorageKey;
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use num_bigint::BigInt;

    let resources = NativePersistResources::from_provider(Arc::new(
        neo_native_contracts::StandardNativeProvider::new(),
    ));
    let store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::new(ProtocolSettings::default()),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        None,
        native_provider(),
        None,
    );

    // Sanity: empty store is not yet initialized.
    assert!(!chain_state_initialized(&snapshot));

    // Run the genesis native-persist pipeline into the shared snapshot, then
    // flush to the durable store — exactly handlers.rs::initialize().
    let settings = ProtocolSettings::default();
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_block_natives_with_resources(
        Arc::clone(&snapshot),
        Arc::clone(&genesis),
        Arc::new(settings.clone()),
        NativePersistOptions::default(),
        &resources,
    )
    .expect("genesis persist");
    ctx.commit_to_store().expect("commit store");

    // The live RPC read path: a FRESH store_cache over the same backing store.
    let read_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    // Policy ExecFeeFactor prefix (PolicyContract.cs:93 Prefix_ExecFeeFactor = 18).
    let key = StorageKey::new(PolicyContract::ID, vec![18u8]);
    let value = read_cache
        .data_cache()
        .get(&key)
        .map(|item| item.value_bytes().into_owned());
    assert_eq!(
        value,
        Some(BigInt::from(30i64).to_signed_bytes_le()),
        "Policy ExecFeeFactor=30 must be visible through a fresh store_cache \
         after genesis persist + commit_to_store (the live RPC read path). \
         This is the v3.10.1 Policy_getExecFeeFactor parity requirement."
    );
}

#[tokio::test]
async fn daemon_context_dispatches_application_logs_handlers() {
    use neo_blockchain::SystemContext;
    use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
    use neo_primitives::{TriggerType, UInt160, WitnessScope};
    use neo_rpc::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_vm_rs::VmState as VMState;

    let settings = Arc::new(ProtocolSettings::default());
    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());

    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = settings.network;
    let logs_service = Arc::new(ApplicationLogsService::new(
        logs_settings,
        Arc::new(MemoryStore::new()),
    ));

    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::clone(&settings),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        None,
        native_provider(),
        Some(Arc::clone(&logs_service)),
    );
    let _finalized_stream = ctx.spawn_finalized_stream();
    let signer = UInt160::from_bytes(&[5; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[6; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(117);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(7);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    assert!(ctx.requires_replay_artifacts(&block, neo_blockchain::BlockPersistContext::live()));
    assert!(
        !ctx.requires_replay_artifacts(&block, neo_blockchain::BlockPersistContext::catch_up())
    );

    let mut executed = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        10,
        Vec::new(),
    );
    executed
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            Vec::new(),
        ));

    assert!(ctx.block_committing(&block, &snapshot, &[]));
    ctx.block_finalized(neo_blockchain::FinalizedBlock::new(
        Arc::new(block),
        Some(Arc::clone(&snapshot)),
        vec![executed],
        neo_blockchain::BlockPersistContext::live(),
    ))
    .await
    .expect("acknowledged finalized projection");

    let tx_log = logs_service
        .get_transaction_log(&tx_hash)
        .expect("transaction application log");
    assert_eq!(tx_log["txid"], tx_hash.to_string());
    assert_eq!(tx_log["executions"][0]["trigger"], "Application");
    assert_eq!(
        tx_log["executions"][0]["notifications"][0]["eventname"],
        "Transfer"
    );
}

#[test]
fn post_canonical_application_logs_do_not_arm_replay_marker() {
    use neo_blockchain::BlockPersistContext;
    use neo_payloads::{Block, Header};
    use neo_rpc::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_system::BlockCommitHooks;

    let temp = tempfile::tempdir().expect("temp dir");
    let marker = temp.path().join(".neo-local-replay-poisoned");
    let shutdown = tokio_util::sync::CancellationToken::new();
    let settings = ProtocolSettings::default();
    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = settings.network;
    let logs = Arc::new(ApplicationLogsService::new(
        logs_settings,
        Arc::new(MemoryStore::new()),
    ));
    let hooks: DaemonCommitHooks<
        neo_native_contracts::StandardNativeProvider,
        MemoryStore,
        MemoryStore,
        MemoryStore,
    > = DaemonCommitHooks::new(
        settings.network,
        None,
        false,
        None,
        Some(logs),
        None,
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            Some(marker.clone()),
            shutdown,
        )),
    );
    let chain_store = Arc::new(MemoryStore::new());
    let snapshot = StoreCache::new_from_store(chain_store, false)
        .data_cache()
        .clone();
    let mut header = Header::new();
    header.set_index(1);
    let block = Block::from_parts(header, Vec::new());

    assert!(BlockCommitHooks::block_committing(
        &hooks,
        &block,
        &snapshot,
        &[],
        1,
        BlockPersistContext::live(),
    ));
    assert!(
        !marker.exists(),
        "post-canonical ApplicationLogs persistence must not fsync the recovery marker"
    );
}

#[test]
fn sync_batch_policy_never_spans_live_post_canonical_plugin_staging() {
    use neo_rpc::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
    use neo_system::BlockCommitHooks;

    type Hooks = DaemonCommitHooks<
        neo_native_contracts::StandardNativeProvider,
        MemoryStore,
        MemoryStore,
        MemoryStore,
    >;

    let settings = ProtocolSettings::default();
    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = settings.network;
    let logs = Arc::new(ApplicationLogsService::new(
        logs_settings,
        Arc::new(MemoryStore::new()),
    ));
    let guarded = Hooks::new(
        settings.network,
        None,
        false,
        None,
        Some(logs),
        None,
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            tokio_util::sync::CancellationToken::new(),
        )),
    );

    assert_eq!(
        <Hooks as BlockCommitHooks<MemoryStore>>::sync_batch_commit_policy(
            &guarded, 1, 100, 10_100,
        ),
        neo_blockchain::SyncBatchCommitPolicy::PerBlock,
        "the exact catch-up boundary still runs per-block plugin staging",
    );
    assert_eq!(
        <Hooks as BlockCommitHooks<MemoryStore>>::sync_batch_commit_policy(
            &guarded, 1, 100, 10_101,
        ),
        neo_blockchain::SyncBatchCommitPolicy::DeferredCatchUp,
        "batching freezes catch-up observer semantics for the whole range",
    );

    let observer_free = Hooks::new(
        settings.network,
        None,
        false,
        None,
        None,
        None,
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            tokio_util::sync::CancellationToken::new(),
        )),
    );
    assert_eq!(
        <Hooks as BlockCommitHooks<MemoryStore>>::sync_batch_commit_policy(
            &observer_free,
            1,
            100,
            0,
        ),
        neo_blockchain::SyncBatchCommitPolicy::DeferredLive,
        "observer-free compositions can batch before a peer tip is known",
    );
}

#[test]
fn static_archive_bounds_deferred_commit_staging() {
    use neo_static_files::{StaticFileArchiveFactory, StaticFileProviderFactory};
    use neo_system::BlockCommitHooks;

    type Hooks = DaemonCommitHooks<
        neo_native_contracts::StandardNativeProvider,
        MemoryStore,
        MemoryStore,
        MemoryStore,
    >;

    let temp = tempfile::tempdir().expect("temp dir");
    let files = StaticFileArchiveFactory::default()
        .open(&temp.path().join("ledger.static"))
        .expect("static archive");
    let hooks = Hooks::new(
        ProtocolSettings::default().network,
        None,
        false,
        None,
        None,
        Some(neo_blockchain::StaticLedgerArchive::new(files)),
        Arc::new(crate::node::recovery::LocalReplayGuard::new(
            None,
            tokio_util::sync::CancellationToken::new(),
        )),
    );

    assert_eq!(
        <Hooks as BlockCommitHooks<MemoryStore>>::sync_batch_commit_policy(&hooks, 1, 64, 0),
        neo_blockchain::SyncBatchCommitPolicy::DeferredLive,
        "a bounded archive batch should retain one canonical commit",
    );
    assert_eq!(
        <Hooks as BlockCommitHooks<MemoryStore>>::sync_batch_commit_policy(&hooks, 1, 65, 0),
        neo_blockchain::SyncBatchCommitPolicy::PerBlock,
        "oversized archive staging must fall back to bounded per-block commits",
    );
}

#[tokio::test]
async fn daemon_context_skips_application_logs_finalized_projection_during_bulk_sync() {
    use neo_blockchain::{BlockPersistContext, SystemContext};
    use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
    use neo_primitives::{TriggerType, UInt160, WitnessScope};
    use neo_rpc::application_logs::{ApplicationLogsService, ApplicationLogsSettings};
    use neo_storage::persistence::StoreCache;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_vm_rs::VmState as VMState;

    let settings = Arc::new(ProtocolSettings::default());
    let chain_store = Arc::new(MemoryStore::new());
    let store_cache = StoreCache::new_from_store(Arc::clone(&chain_store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());

    let mut logs_settings = ApplicationLogsSettings::default();
    logs_settings.enabled = true;
    logs_settings.network = settings.network;
    let logs_service = Arc::new(ApplicationLogsService::new(
        logs_settings,
        Arc::new(MemoryStore::new()),
    ));

    let ctx: TestDaemonContext<_, MemoryStore> = daemon_context(
        Arc::clone(&settings),
        Arc::clone(&snapshot),
        store_cache,
        None,
        false,
        None,
        native_provider(),
        Some(Arc::clone(&logs_service)),
    );
    let signer = UInt160::from_bytes(&[7; UInt160::LENGTH]).expect("signer");
    let contract = UInt160::from_bytes(&[8; UInt160::LENGTH]).expect("contract");
    let mut tx = Transaction::new();
    tx.set_nonce(223);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(signer, WitnessScope::CALLED_BY_ENTRY)]);
    let tx_hash = tx.try_hash().expect("tx hash");

    let mut header = Header::new();
    header.set_index(11);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let mut executed = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        10,
        Vec::new(),
    );
    executed
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            Vec::new(),
        ));

    assert!(ctx.block_committing_with_context(
        &block,
        &snapshot,
        std::slice::from_ref(&executed),
        BlockPersistContext::trusted_replay(),
    ));
    ctx.block_finalized(neo_blockchain::FinalizedBlock::new(
        Arc::new(block),
        Some(Arc::clone(&snapshot)),
        vec![executed],
        BlockPersistContext::trusted_replay(),
    ))
    .await
    .expect("trusted replay finality is intentionally skipped");

    assert!(
        logs_service.get_transaction_log(&tx_hash).is_none(),
        "bulk sync must not commit local ApplicationLogs replay data"
    );
}

/// Full daemon restart smoke test: when the durable MDBX store already
/// contains a ledger tip, `build_node` must read it before P2P starts,
/// advertise it in `version`, request verified headers from `tip + 1`, and
/// request bodies only after that header range passes validation.
#[tokio::test]
async fn build_node_restarts_from_durable_mdbx_tip_and_resumes_sync_cursor() {
    const DURABLE_TIP: u32 = 1;
    const PEER_HEIGHT: u32 = 3;

    let temp = tempfile::tempdir().expect("temp MDBX root");
    let storage_path = temp.path().join("chain");
    let private_key = neo_crypto::Secp256r1Crypto::generate_private_key();
    let public_key_bytes = neo_crypto::Secp256r1Crypto::derive_public_key(&private_key)
        .expect("derive validator public key");
    let public_key = neo_crypto::ECPoint::from_bytes(&public_key_bytes).expect("validator point");
    let mut protocol = ProtocolSettings::default();
    protocol.standby_committee = vec![public_key.clone()];
    protocol.validators_count = 1;
    let settings = Arc::new(protocol);
    seed_mdbx_tip(&storage_path, settings.as_ref(), DURABLE_TIP).expect("seed durable MDBX tip");

    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    let running = build_node(
        Arc::clone(&settings),
        &config,
        Some(&storage_path),
        None,
        LedgerMode::Local,
        false,
        None,
    )
    .await
    .expect("build node over durable store");

    running
        .network()
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start P2P listener");
    let port = running.network().local_node_info().port();
    assert_ne!(port, 0);

    let mut fake = fake_dial(port).await;
    let node_version = recv_frame(&mut fake).await;
    assert_eq!(node_version.command, MessageCommand::Version);
    let node_version: VersionPayload = decode_payload(&node_version);
    assert!(
        node_version.capabilities.iter().any(|capability| matches!(
            capability,
            NodeCapability::FullNode {
                start_height: DURABLE_TIP
            }
        )),
        "restarted daemon must advertise the durable ledger tip"
    );

    fake.send(fake_peer_version_message(
        settings.network,
        0xfa4e_00d0,
        PEER_HEIGHT,
    ))
    .await
    .expect("send peer version");
    let verack = recv_frame(&mut fake).await;
    assert_eq!(verack.command, MessageCommand::Verack);
    fake.send(verack_message()).await.expect("send verack");

    let header_request = recv_getheaders(&mut fake).await;
    assert_eq!(header_request.index_start, DURABLE_TIP + 1);
    assert_eq!(header_request.count, (PEER_HEIGHT - DURABLE_TIP) as i16);

    let genesis = neo_blockchain::genesis_block(settings.as_ref()).expect("genesis");
    let durable_parent = empty_child_block(&genesis, DURABLE_TIP);
    let block_two = signed_empty_child_block(
        &durable_parent,
        DURABLE_TIP + 1,
        settings.network,
        &private_key,
        &public_key,
    );
    let block_three = signed_empty_child_block(
        &block_two,
        PEER_HEIGHT,
        settings.network,
        &private_key,
        &public_key,
    );
    let headers = neo_payloads::HeadersPayload::create(vec![
        block_two.header.clone(),
        block_three.header.clone(),
    ]);
    fake.send(
        Message::create(MessageCommand::Headers, Some(&headers), false)
            .expect("encode verified headers"),
    )
    .await
    .expect("send verified headers");

    let request = recv_getblockbyindex(&mut fake).await;
    assert_eq!(
        request.index_start,
        DURABLE_TIP + 1,
        "coordinator sync cursor resumes just after the durable tip"
    );
    assert_eq!(request.count, (PEER_HEIGHT - DURABLE_TIP) as i16);

    running.abort_for_test().await;
}

/// Operator-facing RPC smoke test: a daemon rebuilt over a durable MDBX
/// ledger must expose the recovered chain height through JSON-RPC.
///
/// Runs on a multi-thread runtime to match the production daemon
/// (`#[tokio::main]`): the JSON-RPC relay path (`sendrawtransaction` /
/// `submitblock`) uses `block_in_place`, which requires a multi-thread
/// runtime. `getblockcount` itself does not, but the multi-thread flavor
/// keeps this end-to-end smoke test representative of the real daemon.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rpc_getblockcount_reads_restarted_durable_mdbx_tip() {
    const DURABLE_TIP: u32 = 1;

    let temp = tempfile::tempdir().expect("temp MDBX root");
    let storage_path = temp.path().join("chain");
    let settings = Arc::new(ProtocolSettings::default());
    seed_mdbx_tip(&storage_path, settings.as_ref(), DURABLE_TIP).expect("seed durable MDBX tip");

    let rpc_port = unused_local_rpc_port();
    let mut config = NodeConfig::default();
    config.storage.backend = Some("mdbx".to_string());
    config.rpc.enabled = true;
    config.rpc.port = Some(rpc_port);
    config.rpc.bind_address = Some("127.0.0.1".to_string());

    let running = build_node(
        Arc::clone(&settings),
        &config,
        Some(&storage_path),
        None,
        LedgerMode::Local,
        false,
        None,
    )
    .await
    .expect("build node over durable store");
    let server = start_rpc_server(
        running.node(),
        running.services().as_ref(),
        &config,
        settings.network,
        None,
    )
    .expect("start JSON-RPC server");
    assert!(server.read().is_started(), "JSON-RPC server must bind");

    let response = rpc_post_json(
        rpc_port,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 1
        }),
    )
    .await;
    assert_eq!(response.get("error"), None);
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"], serde_json::json!(DURABLE_TIP + 1));

    server.write().stop_rpc_server();
    drop(server);
    running.abort_for_test().await;
}
