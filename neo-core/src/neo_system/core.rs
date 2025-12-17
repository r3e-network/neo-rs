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
use std::any::Any;
use std::sync::{Arc, Weak};
use std::time::Duration;

// Use extracted modules
use super::actors::TransactionRouterActor;
use super::context::NeoSystemContext;
use super::helpers::{initialise_plugins, to_core_error};
use super::mempool::attach_mempool_callbacks;
use super::registry::ServiceRegistry;
use super::relay::{RelayExtensibleCache, RELAY_CACHE_CAPACITY};
use super::system::STATE_STORE_SERVICE;

use crate::akka::{ActorRef, ActorSystem, EventStreamHandle};

use crate::error::{CoreError, CoreResult};
use crate::events::{broadcast_plugin_event, PluginEvent};
use crate::extensions::log_level::LogLevel;
use crate::extensions::utility::ExtensionsUtility;
#[cfg(test)]
use crate::extensions::LogLevel as ExternalLogLevel;
use crate::i_event_handlers::IServiceAddedHandler;
use crate::ledger::blockchain::{Blockchain, BlockchainCommand};
use crate::ledger::{HeaderCache, LedgerContext, MemoryPool};
use crate::network::p2p::{
    payloads::block::Block, timeouts, LocalNode, TaskManager, TaskManagerCommand,
};
use crate::persistence::{
    i_store::IStore, i_store_provider::IStoreProvider, StoreCache, StoreFactory,
};
pub use crate::protocol_settings::ProtocolSettings;
use crate::services::{LedgerService, MempoolService, PeerManagerService, StateStoreService};
use crate::smart_contract::native::helpers::NativeHelpers;
use crate::state_service::StateStore;
use neo_primitives::UInt256;
#[cfg(test)]
use once_cell::sync::Lazy;

// initialise_plugins and to_core_error have been extracted to super::helpers module
#[cfg(test)]
static TEST_SYSTEM_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

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
    blockchain: ActorRef,
    pub(crate) local_node: ActorRef,
    task_manager: ActorRef,
    tx_router: ActorRef,
    store_provider: Arc<dyn IStoreProvider>,
    store: Arc<dyn IStore>,
    ledger: Arc<LedgerContext>,
    genesis_block: Arc<Block>,
    context: Arc<NeoSystemContext>,
    pub(crate) self_ref: Mutex<Weak<NeoSystem>>,
}

impl NeoSystem {
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

    // persist_block moved to persistence module
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
        storage_provider: Option<Arc<dyn IStoreProvider>>,
        storage_path: Option<String>,
    ) -> CoreResult<Arc<Self>> {
        Self::new_with_state_service(settings, storage_provider, storage_path, None)
    }

    /// Creates a new NeoSystem with custom state service settings.
    ///
    /// Use this method when you need to enable state root calculation and validation.
    pub fn new_with_state_service(
        settings: ProtocolSettings,
        storage_provider: Option<Arc<dyn IStoreProvider>>,
        storage_path: Option<String>,
        state_service_settings: Option<crate::state_service::state_store::StateServiceSettings>,
    ) -> CoreResult<Arc<Self>> {
        #[cfg(test)]
        let _test_guard = TEST_SYSTEM_MUTEX.lock();

        let actor_system = ActorSystem::new("neo").map_err(to_core_error)?;
        let settings_arc = Arc::new(settings.clone());
        let genesis_block = Arc::new(crate::ledger::create_genesis_block(&settings));

        let service_registry = Arc::new(ServiceRegistry::new());
        let service_added_handlers: Arc<RwLock<Vec<Arc<dyn IServiceAddedHandler + Send + Sync>>>> =
            Arc::new(RwLock::new(Vec::new()));
        let wallet_changed_handlers = Arc::new(RwLock::new(Vec::new()));

        let state_service_enabled = state_service_settings.is_some();

        let store_provider = match storage_provider {
            Some(provider) => provider,
            None => StoreFactory::get_store_provider("Memory").ok_or_else(|| {
                CoreError::invalid_operation("default memory store provider is not registered")
            })?,
        };
        let (store, store_cache_for_hydration, state_store) = super::storage::init_store(
            store_provider.clone(),
            storage_path,
            settings_arc.clone(),
            state_service_settings,
        )?;

        let user_agent = format!("/neo-rs:{}/", env!("CARGO_PKG_VERSION"));
        let local_node_state = Arc::new(LocalNode::new(settings_arc.clone(), 10333, user_agent));
        local_node_state.set_seed_list(settings.seed_list.clone());
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        super::storage::hydrate_ledger(&store_cache_for_hydration, &ledger, &header_cache);
        let memory_pool = Arc::new(Mutex::new(MemoryPool::new(&settings)));
        let relay_cache = Arc::new(RelayExtensibleCache::new(RELAY_CACHE_CAPACITY));

        register_builtin_services(
            &service_registry,
            &local_node_state,
            &ledger,
            &memory_pool,
            &state_store,
        )?;

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
            service_added_handlers: service_added_handlers.clone(),
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
            transaction_added_handlers: Arc::new(RwLock::new(Vec::new())),
            transaction_removed_handlers: Arc::new(RwLock::new(Vec::new())),
            log_handlers: Arc::new(RwLock::new(Vec::new())),
            logging_handlers: Arc::new(RwLock::new(Vec::new())),
            notify_handlers: Arc::new(RwLock::new(Vec::new())),
        });

        if state_service_enabled {
            let handlers = Arc::new(
                crate::state_service::commit_handlers::StateServiceCommitHandlers::new(
                    state_store.clone(),
                ),
            );
            context.register_committing_handler(handlers.clone())?;
            context.register_committed_handler(handlers)?;
        }

        NativeHelpers::attach_system_context(context.clone());
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
            genesis_block,
            context,
            self_ref: Mutex::new(Weak::new()),
        });

        system.context.set_system(Arc::downgrade(&system));

        {
            let mut weak_guard = system.self_ref.lock();
            *weak_guard = Arc::downgrade(&system);
        }

        initialise_plugins(&system)?;

        Ok(system)
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

    /// Typed ledger view.
    pub fn ledger_typed(&self) -> CoreResult<Option<Arc<dyn LedgerService>>> {
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
        self.settings.time_per_block()
    }

    /// Reference to the blockchain actor.
    pub fn blockchain_actor(&self) -> ActorRef {
        self.blockchain.clone()
    }

    /// Reference to the local node actor.
    pub fn local_node_actor(&self) -> ActorRef {
        self.local_node.clone()
    }

    /// Reference to the task manager actor.
    pub fn task_manager_actor(&self) -> ActorRef {
        self.task_manager.clone()
    }

    /// Reference to the transaction router actor.
    pub fn tx_router_actor(&self) -> ActorRef {
        self.tx_router.clone()
    }

    /// Shared in-memory ledger facade used by networking components.
    pub fn ledger_context(&self) -> Arc<LedgerContext> {
        self.ledger.clone()
    }

    /// Returns the canonical genesis block for this system instance.
    pub fn genesis_block(&self) -> Arc<Block> {
        self.genesis_block.clone()
    }

    /// Returns the block hash at the given index if available.
    pub fn block_hash_at(&self, index: u32) -> Option<UInt256> {
        self.ledger.block_hash_at(index)
    }

    /// Returns the configured store provider.
    pub fn store_provider(&self) -> Arc<dyn IStoreProvider> {
        self.store_provider.clone()
    }

    /// Returns the primary store instance.
    pub fn store(&self) -> Arc<dyn IStore> {
        self.store.clone()
    }

    /// Returns the state store service when available (None if not registered).
    pub fn state_store(&self) -> CoreResult<Option<Arc<StateStore>>> {
        self.context.state_store()
    }

    /// Gracefully shuts down the actor hierarchy.
    pub async fn shutdown(self: Arc<Self>) -> CoreResult<()> {
        broadcast_plugin_event(&PluginEvent::NodeStopping);
        // Drop the global logging hook to avoid leaking callbacks across system lifetimes.
        ExtensionsUtility::set_logging(None);
        timeouts::log_stats();
        self.actor_system.shutdown().await.map_err(to_core_error)
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
    state_store: &Arc<StateStore>,
) -> CoreResult<()> {
    // Note: Cannot use Arc::clone() here because we need to coerce concrete Arc<T> to Arc<dyn Any>
    let local_node_any: Arc<dyn Any + Send + Sync> = local_node_state.clone();
    service_registry.register(local_node_any, Some("LocalNode".to_string()))?;

    let ledger_any: Arc<dyn Any + Send + Sync> = ledger.clone();
    service_registry.register(ledger_any, Some("Ledger".to_string()))?;

    let mem_pool_any: Arc<dyn Any + Send + Sync> = memory_pool.clone();
    service_registry.register(mem_pool_any, Some("MemoryPool".to_string()))?;

    let state_store_any: Arc<dyn Any + Send + Sync> = state_store.clone();
    service_registry.register(state_store_any, Some(STATE_STORE_SERVICE.to_string()))?;
    Ok(())
}

fn spawn_core_actors(
    actor_system: &ActorSystem,
    ledger: Arc<LedgerContext>,
    local_node_state: Arc<LocalNode>,
    settings: Arc<ProtocolSettings>,
) -> CoreResult<(ActorRef, ActorRef, ActorRef, ActorRef)> {
    let blockchain = actor_system
        .actor_of(Blockchain::props(ledger), "blockchain")
        .map_err(to_core_error)?;
    let local_node = actor_system
        .actor_of(LocalNode::props(local_node_state), "local_node")
        .map_err(to_core_error)?;
    let task_manager = actor_system
        .actor_of(TaskManager::props(), "task_manager")
        .map_err(to_core_error)?;
    let tx_router = actor_system
        .actor_of(
            TransactionRouterActor::props(settings, blockchain.clone()),
            "tx_router",
        )
        .map_err(to_core_error)?;
    Ok((blockchain, local_node, task_manager, tx_router))
}

fn attach_system_to_actors(
    blockchain: &ActorRef,
    local_node_state: &Arc<LocalNode>,
    task_manager: &ActorRef,
    context: Arc<NeoSystemContext>,
) -> CoreResult<()> {
    blockchain
        .tell(BlockchainCommand::AttachSystem(context.clone()))
        .map_err(to_core_error)?;
    local_node_state.set_system_context(context.clone());
    task_manager
        .tell(TaskManagerCommand::AttachSystem { context })
        .map_err(to_core_error)?;
    blockchain
        .tell(BlockchainCommand::Initialize)
        .map_err(to_core_error)?;
    Ok(())
}

// to_core_error has been extracted to super::helpers module
// TransactionRouterActor and TransactionRouterMessage have been extracted to super::actors module
