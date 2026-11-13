//! Core node system orchestration (actors, services, plugins, wallets, networking).
//!
//! This module contains the main `NeoSystem` and `NeoSystemContext` types that form
//! the heart of the Neo N3 blockchain node implementation.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                       NeoSystem                              │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                  NeoSystemContext                        ││
//! │  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌─────────────┐ ││
//! │  │  │ Ledger   │ │ MemPool  │ │ StateStore│ │ LocalNode   │ ││
//! │  │  │ Service  │ │ Service  │ │ Service   │ │ (P2P)       │ ││
//! │  │  └──────────┘ └──────────┘ └──────────┘ └─────────────┘ ││
//! │  │  ┌──────────┐ ┌──────────┐ ┌──────────────────────────┐ ││
//! │  │  │ Plugin   │ │ Service  │ │ Event Handlers           │ ││
//! │  │  │ Manager  │ │ Registry │ │ (Commit, Notify, Log)    │ ││
//! │  │  └──────────┘ └──────────┘ └──────────────────────────┘ ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                    Actor System                          ││
//! │  │  ┌──────────┐ ┌──────────┐ ┌──────────┐                 ││
//! │  │  │Blockchain│ │TaskManager│ │TxRouter  │                 ││
//! │  │  │  Actor   │ │  Actor   │ │  Actor   │                 ││
//! │  │  └──────────┘ └──────────┘ └──────────┘                 ││
//! │  └─────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Components
//!
//! - [`NeoSystem`]: Main entry point, owns the actor system and provides public API
//! - [`NeoSystemContext`]: Internal context holding services, event handlers, and state
//! - [`ServiceRegistry`]: Type-safe service discovery (see [`super::registry`])
//! # Thread Safety
//!
//! Both `NeoSystem` and `NeoSystemContext` are designed for concurrent access:
//! - Services are wrapped in `Arc` for shared ownership
//! - Event handlers use `RwLock` for safe concurrent registration
//! - Actor communication is message-based and inherently thread-safe
//!
//! # Lock Ordering
//!
//! To prevent deadlocks, locks must be acquired in this order:
//! 1. `store_cache` (persistence layer)
//! 2. `header_cache` (block headers)
//! 3. `mempool` (transaction pool)
//! 4. `relay_cache` (extensible payloads)
//! 5. Event handler locks (any order among themselves)

use parking_lot::{Mutex, RwLock};

#[derive(Clone, Copy, PartialEq, Eq)]
enum LedgerHydration {
    Complete,
    CheckpointTip,
}

use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

// Use extracted modules
use super::actors::TransactionRouterHandle;
use super::context::NeoSystemContext;
use super::helpers::{initialise_plugins, to_core_error};
use super::mempool::attach_mempool_callbacks;
use super::registry::ServiceRegistry;
use super::relay::{RELAY_CACHE_CAPACITY, RelayExtensibleCache};
use super::system::STATE_STORE_SERVICE;

use neo_actors::{ActorRef, ActorSystem, EventStreamHandle};

use crate::blockchain::{Blockchain, BlockchainCommand, BlockchainHandle, BlockchainSystemCommand};
use crate::events::{PluginEvent, broadcast_plugin_event};
use crate::mempool::MemoryPool;
use crate::network::p2p::{LocalNode, LocalNodeHandle, TaskManager, TaskManagerHandle, timeouts};
use crate::services::StateStoreService as StateStore;
use crate::services::{MempoolService, PeerManagerService, StateStoreService, SystemContext};
use neo_ledger::{HeaderCache, LedgerContext};
use neo_native_contracts::PolicyContract;
use neo_native_contracts::ledger_contract::LedgerContract;
use neo_primitives::UInt256;
use neo_primitives::{ExtensionsUtility, LogLevel};
use neo_protocol::hardfork::Hardfork;
pub use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_storage::persistence::{
    StoreCache, StoreFactory, store::Store, store_provider::StoreProvider,
};
#[cfg(test)]
use std::sync::LazyLock;

// initialise_plugins and to_core_error have been extracted to super::helpers module
#[cfg(test)]
static TEST_SYSTEM_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// RelayExtensibleEntry, RelayExtensibleCache, and constants have been extracted to super::relay module

/// Central runtime coordinating all services for a Neo node.
///
/// # Lock Ordering
///
/// To prevent deadlocks, locks must be acquired in the following order:
///
/// 1. `context.service_registry.services_by_name` (RwLock)
/// 2. `context.service_registry.typed_services` (RwLock)
/// 3. `context.service_registry.services` (RwLock)
/// 4. `context.service_added_handlers` (RwLock)
/// 5. `self_ref` (Mutex)
///
/// For `NeoSystemContext`:
/// 1. `system` (RwLock)
/// 2. `current_wallet` (RwLock)
/// 3. `wallet_changed_handlers` (RwLock)
/// 4. `committing_handlers` (RwLock)
/// 5. `committed_handlers` (RwLock)
/// 6. `transaction_added_handlers` (RwLock)
/// 7. `transaction_removed_handlers` (RwLock)
/// 8. `log_handlers` (RwLock)
/// 9. `logging_handlers` (RwLock)
/// 10. `notify_handlers` (RwLock)
/// 11. `memory_pool` (Mutex)
///
/// **Important**: Never hold a lock across an `.await` point. If async operations
/// are needed, clone the data or drop the lock before awaiting.
pub struct NeoSystem {
    settings: ProtocolSettings,
    actor_system: ActorSystem,
    blockchain: BlockchainHandle,
    pub(crate) local_node: LocalNodeHandle,
    task_manager: TaskManagerHandle,
    tx_router: TransactionRouterHandle,
    store_provider: Arc<dyn StoreProvider>,
    store: Arc<dyn Store>,
    ledger: Arc<LedgerContext>,
    genesis_block: Arc<neo_protocol::execution::Block>,
    pub(crate) native_services: neo_smart_contract::application_engine::EngineNativeServices,
    state_commit_handlers: Option<Arc<crate::state_service::StateServiceCommitHandlers>>,
    context: Arc<NeoSystemContext>,
    execution_mode: AtomicU8,
    pub(crate) execution_stats: Arc<super::ExecutionStats>,
    native_method_cache: Arc<Mutex<neo_smart_contract::application_engine::NativeContractsCache>>,
    pub(crate) self_ref: Mutex<Weak<NeoSystem>>,
}

impl NeoSystem {
    pub(crate) fn native_method_cache_handle(
        &self,
    ) -> Arc<Mutex<neo_smart_contract::application_engine::NativeContractsCache>> {
        Arc::clone(&self.native_method_cache)
    }

    /// Returns the execution engine selected for normal block persistence.
    pub fn execution_mode(&self) -> super::ExecutionMode {
        match self.execution_mode.load(Ordering::Relaxed) {
            1 => super::ExecutionMode::Auto,
            2 => super::ExecutionMode::Verify,
            _ => super::ExecutionMode::Canonical,
        }
    }

    /// Selects the execution engine for subsequent normal block persistence.
    /// Explicit per-call mode APIs are unaffected.
    pub fn set_execution_mode(&self, mode: super::ExecutionMode) {
        let value = match mode {
            super::ExecutionMode::Canonical => 0,
            super::ExecutionMode::Auto => 1,
            super::ExecutionMode::Verify => 2,
        };
        self.execution_mode.store(value, Ordering::Relaxed);
    }

    /// Returns process-local execution counters for observability.
    pub fn execution_stats(&self) -> Arc<super::ExecutionStats> {
        Arc::clone(&self.execution_stats)
    }

    /// Creates a new `StoreCache` bound to the system's underlying store.
    pub fn store_cache(&self) -> StoreCache {
        self.context.store_cache()
    }

    /// Provides direct access to the shared memory pool instance.
    pub fn mempool(&self) -> Arc<Mutex<MemoryPool>> {
        self.context.memory_pool()
    }

    /// Returns the current best block index tracked by the system.
    pub fn current_block_index(&self) -> u32 {
        self.context.current_block_index()
    }
}

impl NeoSystem {
    /// Bootstraps the runtime and spawns the core actor hierarchy following the C# layout.
    ///
    /// Mirrors the C# constructor overload that accepts protocol settings,
    /// an optional store provider name and a storage path.
    ///
    /// # Arguments
    /// * `settings` - Protocol settings for the network
    /// * `storage_provider` - Optional storage provider (defaults to memory)
    /// * `storage_path` - Optional path for persistent storage
    /// * `state_service_settings` - Optional state service settings for state root calculation
    pub fn new(
        settings: ProtocolSettings,
        storage_provider: Option<Arc<dyn StoreProvider>>,
        storage_path: Option<String>,
    ) -> CoreResult<Arc<Self>> {
        Self::new_with_state_service(settings, storage_provider, storage_path, None)
    }

    /// Creates a new NeoSystem with custom state service settings.
    ///
    /// Use this method when you need to enable state root calculation and validation.
    /// Genesis is persisted before the function returns. Plugins that must observe
    /// genesis should use [`Self::new_with_state_service_deferring_genesis`].
    pub fn new_with_state_service(
        settings: ProtocolSettings,
        storage_provider: Option<Arc<dyn StoreProvider>>,
        storage_path: Option<String>,
        state_service_settings: Option<crate::state_service::StateServiceSettings>,
    ) -> CoreResult<Arc<Self>> {
        Self::open_with_genesis_policy(
            settings,
            storage_provider,
            storage_path,
            state_service_settings,
            true,
        )
    }

    /// Creates a NeoSystem without persisting genesis.
    ///
    /// Register committing handlers, then call [`Self::persist_genesis_if_absent`].
    /// Genesis is the first block those handlers must observe. Persisting it
    /// inside construction drops that block for plugins registered afterwards.
    pub fn new_with_state_service_deferring_genesis(
        settings: ProtocolSettings,
        storage_provider: Option<Arc<dyn StoreProvider>>,
        storage_path: Option<String>,
        state_service_settings: Option<crate::state_service::StateServiceSettings>,
    ) -> CoreResult<Arc<Self>> {
        Self::open_with_genesis_policy(
            settings,
            storage_provider,
            storage_path,
            state_service_settings,
            false,
        )
    }

    /// Opens a checkpoint-gated offline replay without re-reading every historical block.
    ///
    /// Online nodes must use the ordinary constructors, which fully hydrate and
    /// validate the persisted chain. The offline caller authenticates the audit
    /// checkpoint and package block before executing any successor.
    pub fn new_with_state_service_deferring_genesis_checkpoint_replay(
        settings: ProtocolSettings,
        storage_provider: Option<Arc<dyn StoreProvider>>,
        storage_path: Option<String>,
        state_service_settings: Option<crate::state_service::StateServiceSettings>,
    ) -> CoreResult<Arc<Self>> {
        Self::open_with_genesis_policy_and_hydration(
            settings,
            storage_provider,
            storage_path,
            state_service_settings,
            false,
            LedgerHydration::CheckpointTip,
        )
    }

    fn open_with_genesis_policy(
        settings: ProtocolSettings,
        storage_provider: Option<Arc<dyn StoreProvider>>,
        storage_path: Option<String>,
        state_service_settings: Option<crate::state_service::StateServiceSettings>,
        persist_genesis: bool,
    ) -> CoreResult<Arc<Self>> {
        Self::open_with_genesis_policy_and_hydration(
            settings,
            storage_provider,
            storage_path,
            state_service_settings,
            persist_genesis,
            LedgerHydration::Complete,
        )
    }

    fn open_with_genesis_policy_and_hydration(
        settings: ProtocolSettings,
        storage_provider: Option<Arc<dyn StoreProvider>>,
        storage_path: Option<String>,
        state_service_settings: Option<crate::state_service::StateServiceSettings>,
        persist_genesis: bool,
        hydration: LedgerHydration,
    ) -> CoreResult<Arc<Self>> {
        #[cfg(test)]
        let _test_guard = TEST_SYSTEM_MUTEX.lock();

        let actor_system = ActorSystem::new("neo").map_err(to_core_error)?;
        let settings_arc = Arc::new(settings.clone());
        let genesis_payload = neo_ledger::create_genesis_block(&settings);
        let genesis_block = Arc::new(super::converters::convert_payload_block(&genesis_payload));

        let service_registry = Arc::new(ServiceRegistry::new());
        let wallet_changed_handlers = Arc::new(RwLock::new(Vec::new()));

        let state_service_enabled = state_service_settings.is_some();

        let store_provider = match storage_provider {
            Some(provider) => provider,
            None => StoreFactory::get_store_provider("Memory").ok_or_else(|| {
                CoreError::invalid_operation("default memory store provider is not registered")
            })?,
        };
        let (store, store_cache_for_hydration) =
            super::storage::init_store(&store_provider, storage_path.as_deref())?;

        let user_agent = format!("/neo-rs:{}/", env!("CARGO_PKG_VERSION"));
        let local_node_state = Arc::new(LocalNode::new(settings_arc.clone(), 10333, user_agent));
        local_node_state.set_seed_list(settings.seed_list.clone());
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        match hydration {
            LedgerHydration::Complete => {
                super::storage::hydrate_ledger(&store_cache_for_hydration, &ledger, &header_cache)?;
            }
            LedgerHydration::CheckpointTip => {
                super::storage::hydrate_ledger_tip_for_checkpoint_replay(
                    &store_cache_for_hydration,
                    &ledger,
                    &header_cache,
                )?;
            }
        }
        let native_services = neo_native_contracts::native::composition::default_native_services();
        super::storage::hydrate_native_contracts(
            &store_cache_for_hydration,
            &native_services,
            &settings,
        )?;
        let time_per_block = {
            let current_index = LedgerContract::new()
                .current_index(&store_cache_for_hydration)
                .unwrap_or(0);
            if settings.is_hardfork_enabled(Hardfork::HfEchidna, current_index) {
                PolicyContract::get_milliseconds_per_block_snapshot(&store_cache_for_hydration)
                    .map(|ms| Duration::from_millis(ms as u64))
                    .unwrap_or_else(|| settings.time_per_block())
            } else {
                settings.time_per_block()
            }
        };
        let memory_pool = Arc::new(Mutex::new(MemoryPool::new_with_time_per_block(
            &settings,
            time_per_block,
        )));
        let relay_cache = Arc::new(RelayExtensibleCache::new(RELAY_CACHE_CAPACITY));
        let state_store = if state_service_enabled {
            let state_store = Arc::new(
                crate::state_service::StateRootStore::new(
                    store.clone(),
                    store_cache_for_hydration.clone(),
                    settings_arc.clone(),
                )
                .map_err(|error| CoreError::invalid_operation(&error.message))?,
            );
            Some(state_store as Arc<dyn StateStoreService>)
        } else {
            None
        };

        if let Some(ref state_store_service) = state_store {
            register_builtin_services(
                &service_registry,
                &local_node_state,
                &ledger,
                &memory_pool,
                state_store_service,
            )?;
        } else {
            // Register services without state store
            register_builtin_services_without_state(
                &service_registry,
                &local_node_state,
                &ledger,
                &memory_pool,
            )?;
        }

        let (blockchain, local_node, task_manager, tx_router) = spawn_core_actors(
            &actor_system,
            ledger.clone(),
            local_node_state.clone(),
            settings_arc.clone(),
        )?;

        let context = Arc::new(NeoSystemContext {
            actor_system: actor_system.handle(),
            blockchain: blockchain.clone(),
            local_node: local_node.clone(),
            task_manager: task_manager.clone(),
            tx_router: tx_router.clone(),
            service_registry: service_registry.clone(),
            wallet_changed_handlers: wallet_changed_handlers.clone(),
            current_wallet: Arc::new(RwLock::new(None)),
            store_provider: store_provider.clone(),
            store: store.clone(),
            genesis_block: genesis_block.clone(),
            ledger: ledger.clone(),
            state_service_enabled,
            state_store: state_store.clone(),
            memory_pool: memory_pool.clone(),
            header_cache: header_cache.clone(),
            local_node_state: local_node_state.clone(),
            relay_cache: relay_cache.clone(),
            settings: settings_arc.clone(),
            system: RwLock::new(None),
            committing_handlers: Arc::new(RwLock::new(Vec::new())),
            committed_handlers: Arc::new(RwLock::new(Vec::new())),
            transaction_execution_sink: std::sync::OnceLock::new(),
            dropped_transaction_execution_observations: std::sync::atomic::AtomicU64::new(0),
            fast_sync_mode: Arc::new(AtomicBool::new(false)),
        });

        let state_commit_handlers = if state_service_enabled && state_store.is_some() {
            let handlers = Arc::new(
                crate::state_service::commit_handlers::StateServiceCommitHandlers::new(
                    store_cache_for_hydration.clone(),
                    true,
                )
                .map_err(|e| CoreError::invalid_operation(&e.message))?,
            );
            context.register_committing_handler(handlers.clone())?;
            context.register_committed_handler(handlers.clone())?;
            Some(handlers)
        } else {
            None
        };

        super::system_context::attach_system_context(context.clone());
        configure_logging_hook(&context);

        attach_mempool_callbacks(
            &context,
            &memory_pool,
            local_node.clone(),
            blockchain.clone(),
        );

        attach_system_to_actors(
            &blockchain,
            &local_node_state,
            &task_manager,
            context.clone(),
        )?;

        let system = Arc::new(Self {
            settings,
            actor_system,
            blockchain,
            local_node,
            task_manager,
            tx_router,
            store_provider,
            store,
            ledger,
            genesis_block: genesis_block.clone(),
            native_services,
            state_commit_handlers,
            context,
            execution_mode: AtomicU8::new(0),
            execution_stats: Arc::new(super::ExecutionStats::default()),
            native_method_cache: Arc::new(Mutex::new(Default::default())),
            self_ref: Mutex::new(Weak::new()),
        });

        system.context.set_system(Arc::downgrade(&system));

        {
            let mut weak_guard = system.self_ref.lock();
            *weak_guard = Arc::downgrade(&system);
        }

        if let Some(handlers) = &system.state_commit_handlers {
            handlers
                .recover_from_storage()
                .map_err(|e| CoreError::invalid_operation(&e.message))?;
        }

        initialise_plugins(&system)?;

        if persist_genesis {
            system.persist_genesis_if_absent()?;
        }

        Ok(system)
    }

    /// Persists the canonical genesis block when the ledger does not already
    /// contain it. Committing handlers registered before this call observe genesis.
    pub fn persist_genesis_if_absent(&self) -> CoreResult<()> {
        let mut genesis_payload = neo_ledger::create_genesis_block(self.settings());
        let genesis_hash = genesis_payload.hash();
        let store = self.store();
        let snapshot = store.snapshot();
        let store_cache = StoreCache::new_from_snapshot(snapshot);
        let ledger_contract = LedgerContract::new();
        if !ledger_contract.contains_block(&store_cache, &genesis_hash) {
            tracing::info!(target: "neo", "persisting genesis block to initialize state");
            self.persist_block((*self.genesis_block).clone())
                .map_err(CoreError::invalid_operation)?;
        }
        Ok(())
    }

    /// View of the underlying actor system.
    pub fn actor_system(&self) -> &ActorSystem {
        &self.actor_system
    }

    /// Accessor for broadcasting over the event stream.
    pub fn event_stream(&self) -> EventStreamHandle {
        self.actor_system.event_stream()
    }

    /// Returns a cloneable context handle with references to core actors and services.
    pub fn context(&self) -> Arc<NeoSystemContext> {
        self.context.clone()
    }

    /// Runtime configuration reference.
    pub fn settings(&self) -> &ProtocolSettings {
        &self.settings
    }

    /// Returns the state-root commit handler when state tracking is enabled.
    /// The same instance is registered in the block persistence pipeline and
    /// has recovered its persisted MPT baseline before this system is returned.
    pub fn state_commit_handlers(
        &self,
    ) -> Option<Arc<crate::state_service::StateServiceCommitHandlers>> {
        self.state_commit_handlers.clone()
    }

    /// Typed ledger view.
    pub fn ledger_typed(&self) -> CoreResult<Option<Arc<LedgerContext>>> {
        self.context.ledger_typed()
    }

    /// Typed mempool view.
    pub fn mempool_typed(&self) -> CoreResult<Option<Arc<dyn MempoolService + Send + Sync>>> {
        self.context.mempool_typed()
    }

    /// Typed peer manager view.
    pub fn peer_manager_typed(&self) -> CoreResult<Option<Arc<dyn PeerManagerService>>> {
        self.context.peer_manager_typed()
    }

    /// Typed state store view.
    pub fn state_store_typed(&self) -> CoreResult<Option<Arc<dyn StateStoreService>>> {
        self.context.state_store_typed()
    }

    /// Convenience wrapper returning `Duration` between blocks.
    pub fn time_per_block(&self) -> Duration {
        let settings = &self.settings;
        let store_cache = StoreCache::new_from_snapshot(self.store().snapshot());
        let index = LedgerContract::new()
            .current_index(&store_cache)
            .unwrap_or(0);

        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, index) {
            return settings.time_per_block();
        }

        PolicyContract::get_milliseconds_per_block_snapshot(&store_cache)
            .map(|ms| Duration::from_millis(ms as u64))
            .unwrap_or_else(|| settings.time_per_block())
    }

    /// Returns the max valid-until-block increment with Policy overrides after HF_Echidna.
    pub fn max_valid_until_block_increment(&self) -> u32 {
        let settings = &self.settings;
        let store_cache = StoreCache::new_from_snapshot(self.store().snapshot());
        let index = LedgerContract::new()
            .current_index(&store_cache)
            .unwrap_or(0);

        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, index) {
            return settings.max_valid_until_block_increment;
        }

        PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(&store_cache, settings)
            .unwrap_or(settings.max_valid_until_block_increment)
    }

    /// Returns the max traceable blocks with Policy overrides after HF_Echidna.
    pub fn max_traceable_blocks(&self) -> u32 {
        let settings = &self.settings;
        let store_cache = StoreCache::new_from_snapshot(self.store().snapshot());
        let index = LedgerContract::new()
            .current_index(&store_cache)
            .unwrap_or(0);

        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, index) {
            return settings.max_traceable_blocks;
        }

        PolicyContract::get_max_traceable_blocks_snapshot(&store_cache)
            .unwrap_or(settings.max_traceable_blocks)
    }

    /// Reference to the blockchain actor.
    pub fn blockchain_actor(&self) -> ActorRef {
        self.blockchain.raw_ref().clone()
    }

    /// Typed command handle for the blockchain actor.
    pub fn blockchain_handle(&self) -> BlockchainHandle {
        self.blockchain.clone()
    }

    /// Reference to the local node actor.
    pub fn local_node_actor(&self) -> ActorRef {
        self.local_node.raw_ref().clone()
    }

    /// Typed command handle for the local node actor.
    pub fn local_node_handle(&self) -> LocalNodeHandle {
        self.local_node.clone()
    }

    /// Reference to the task manager actor.
    pub fn task_manager_actor(&self) -> ActorRef {
        self.task_manager.raw_ref().clone()
    }

    /// Typed command handle for the task manager actor.
    pub fn task_manager_handle(&self) -> TaskManagerHandle {
        self.task_manager.clone()
    }

    /// Handle to the transaction router worker.
    pub fn tx_router_actor(&self) -> TransactionRouterHandle {
        self.tx_router.clone()
    }

    /// Shared in-memory ledger facade used by networking components.
    pub fn ledger_context(&self) -> Arc<LedgerContext> {
        self.ledger.clone()
    }

    /// Returns the canonical genesis block for this system instance.
    pub fn genesis_block(&self) -> Arc<neo_protocol::execution::Block> {
        self.genesis_block.clone()
    }

    /// Returns the block hash at the given index if available.
    pub fn block_hash_at(&self, index: u32) -> Option<UInt256> {
        self.ledger.block_hash_at(index)
    }

    /// Returns the configured store provider.
    pub fn store_provider(&self) -> Arc<dyn StoreProvider> {
        self.store_provider.clone()
    }

    /// Returns the primary store instance.
    pub fn store(&self) -> Arc<dyn Store> {
        self.store.clone()
    }

    /// Returns the state store service when available (None if not registered).
    pub fn state_store(&self) -> CoreResult<Option<Arc<dyn StateStore>>> {
        self.context.state_store()
    }

    /// Gracefully shuts down the actor hierarchy.
    pub async fn shutdown(self: Arc<Self>) -> CoreResult<()> {
        broadcast_plugin_event(&PluginEvent::NodeStopping);
        // Drop the global logging hook to avoid leaking callbacks across system lifetimes.
        ExtensionsUtility::set_logging(None);
        timeouts::log_stats();
        self.tx_router.abort();
        let result = self.actor_system.shutdown().await.map_err(to_core_error);
        self.context.local_node_state.clear_system_context();
        let context: Arc<dyn SystemContext> = self.context.clone();
        super::system_context::clear_system_context(&context);
        result
    }
}

#[cfg(test)]
mod tests;

fn configure_logging_hook(context: &Arc<NeoSystemContext>) {
    let logging_context = Arc::downgrade(context);
    ExtensionsUtility::set_logging(Some(Box::new(move |source, level, message| {
        if let Some(ctx) = logging_context.upgrade() {
            let local_level: LogLevel = level;
            ctx.notify_logging_handlers(&source, local_level, &message);
        }
    })));
}

fn register_builtin_services(
    service_registry: &Arc<ServiceRegistry>,
    local_node_state: &Arc<LocalNode>,
    ledger: &Arc<LedgerContext>,
    memory_pool: &Arc<Mutex<MemoryPool>>,
    state_store: &Arc<dyn StateStoreService>,
) -> CoreResult<()> {
    // Note: Cannot use Arc::clone() here because we need to coerce concrete Arc<T> to Arc<dyn Any>
    let local_node_any: Arc<dyn Any + Send + Sync> = local_node_state.clone();
    service_registry.register(local_node_any, Some("LocalNode".to_string()))?;

    let ledger_any: Arc<dyn Any + Send + Sync> = ledger.clone();
    service_registry.register(ledger_any, Some("Ledger".to_string()))?;

    let mem_pool_any: Arc<dyn Any + Send + Sync> = memory_pool.clone();
    service_registry.register(mem_pool_any, Some("MemoryPool".to_string()))?;

    // Register state_store as its concrete type via Any
    // Since state_store is Arc<dyn StateStoreService>, we need to box it
    let state_store_boxed: Arc<dyn Any + Send + Sync> = Arc::new(state_store.clone());
    service_registry.register(state_store_boxed, Some(STATE_STORE_SERVICE.to_string()))?;
    Ok(())
}

fn register_builtin_services_without_state(
    service_registry: &Arc<ServiceRegistry>,
    local_node_state: &Arc<LocalNode>,
    ledger: &Arc<LedgerContext>,
    memory_pool: &Arc<Mutex<MemoryPool>>,
) -> CoreResult<()> {
    let local_node_any: Arc<dyn Any + Send + Sync> = local_node_state.clone();
    service_registry.register(local_node_any, Some("LocalNode".to_string()))?;

    let ledger_any: Arc<dyn Any + Send + Sync> = ledger.clone();
    service_registry.register(ledger_any, Some("Ledger".to_string()))?;

    let mem_pool_any: Arc<dyn Any + Send + Sync> = memory_pool.clone();
    service_registry.register(mem_pool_any, Some("MemoryPool".to_string()))?;
    Ok(())
}

fn spawn_core_actors(
    actor_system: &ActorSystem,
    ledger: Arc<LedgerContext>,
    local_node_state: Arc<LocalNode>,
    settings: Arc<ProtocolSettings>,
) -> CoreResult<(
    BlockchainHandle,
    LocalNodeHandle,
    TaskManagerHandle,
    TransactionRouterHandle,
)> {
    let blockchain = actor_system
        .actor_of(Blockchain::props(ledger), "blockchain")
        .map_err(to_core_error)?;
    let blockchain = BlockchainHandle::new(blockchain);
    let local_node = actor_system
        .actor_of(LocalNode::props(local_node_state), "local_node")
        .map_err(to_core_error)?;
    let local_node = LocalNodeHandle::new(local_node);
    let task_manager = actor_system
        .actor_of(TaskManager::props(), "task_manager")
        .map_err(to_core_error)?;
    let task_manager = TaskManagerHandle::new(task_manager);
    let tx_router = TransactionRouterHandle::spawn(settings, blockchain.clone());
    Ok((blockchain, local_node, task_manager, tx_router))
}

fn attach_system_to_actors(
    blockchain: &BlockchainHandle,
    local_node_state: &Arc<LocalNode>,
    task_manager: &TaskManagerHandle,
    context: Arc<NeoSystemContext>,
) -> CoreResult<()> {
    // System attachment travels as this crate's `BlockchainSystemCommand`
    // envelope (split from the neo-core-resident `BlockchainCommand`).
    blockchain
        .raw_ref()
        .tell(BlockchainSystemCommand::AttachSystem(context.clone()))
        .map_err(|e| CoreError::system(e.to_string()))?;
    local_node_state.set_system_context(context.clone());
    task_manager.attach_system(context).map_err(to_core_error)?;
    blockchain
        .tell(BlockchainCommand::Initialize)
        .map_err(|e| CoreError::system(e.to_string()))?;
    Ok(())
}

// to_core_error has been extracted to super::helpers module
// TransactionRouterHandle has been extracted to super::actors module
