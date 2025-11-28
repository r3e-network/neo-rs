//! Core node system orchestration (actors, services, plugins, wallets, networking).
use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use akka::{
    Actor, ActorContext, ActorRef, ActorResult, ActorSystem, ActorSystemHandle, EventStreamHandle,
    Props,
};
use async_trait::async_trait;
use tokio::runtime::{Builder as RuntimeBuilder, Handle as RuntimeHandle, RuntimeFlavor};
use tokio::sync::oneshot;
use tokio::task::block_in_place;
use tracing::{debug, warn};

use crate::constants::GENESIS_TIMESTAMP_MS;
use crate::contains_transaction_type::ContainsTransactionType;
use crate::error::{CoreError, CoreResult};
use crate::extensions::log_level::LogLevel;
use crate::i_event_handlers::{
    ICommittedHandler, ICommittingHandler, ILogHandler, ILoggingHandler, INotifyHandler,
    IServiceAddedHandler, ITransactionAddedHandler, ITransactionRemovedHandler,
    IWalletChangedHandler,
};
use crate::ledger::blockchain::{Blockchain, BlockchainCommand, PreverifyCompleted};
use crate::ledger::{
    block::Block as LedgerBlock, block_header::BlockHeader as LedgerBlockHeader,
    blockchain_application_executed::ApplicationExecuted, HeaderCache, LedgerContext, MemoryPool,
};
use crate::network::p2p::{
    local_node::RelayInventory,
    payloads::{
        block::Block, extensible_payload::ExtensiblePayload, header::Header,
        transaction::Transaction, witness::Witness as PayloadWitness,
    },
    timeouts, ChannelsConfig, LocalNode, LocalNodeCommand, PeerCommand, RemoteNodeSnapshot,
    TaskManager, TaskManagerCommand,
};
use crate::persistence::{
    data_cache::DataCache, i_store::IStore, i_store_provider::IStoreProvider,
    track_state::TrackState, StoreCache, StoreFactory,
};
pub use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::application_engine::TEST_MODE_GAS;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::log_event_args::LogEventArgs;
use crate::smart_contract::native::helpers::NativeHelpers;
use crate::smart_contract::native::ledger_contract::{
    HashOrIndex, LedgerContract, LedgerTransactionStates, PersistedTransactionState,
};
use crate::smart_contract::notify_event_args::NotifyEventArgs;
use crate::smart_contract::trigger_type::TriggerType;
use crate::state_service::{state_store::StateServiceSettings, StateStore};
use crate::uint160::UInt160;
use crate::uint256::UInt256;
use crate::wallets::{IWalletProvider, Wallet};
use neo_extensions::error::ExtensionError;
use neo_extensions::plugin::{
    broadcast_global_event, initialise_global_runtime, shutdown_global_runtime, PluginContext,
    PluginEvent,
};
use neo_extensions::utility::Utility;
#[cfg(test)]
use neo_extensions::LogLevel as ExternalLogLevel;
use neo_io_crate::{InventoryHash, RelayCache};
use neo_vm::{vm_state::VMState, OpCode};
#[cfg(test)]
use once_cell::sync::Lazy;
use std::thread;

fn block_on_extension<F, T>(future: F) -> Result<T, ExtensionError>
where
    F: Future<Output = Result<T, ExtensionError>> + Send,
    T: Send,
{
    if let Ok(handle) = RuntimeHandle::try_current() {
        match handle.runtime_flavor() {
            RuntimeFlavor::MultiThread => block_in_place(|| handle.block_on(future)),
            RuntimeFlavor::CurrentThread => RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| ExtensionError::operation_failed(err.to_string()))?
                .block_on(future),
            _ => RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| ExtensionError::operation_failed(err.to_string()))?
                .block_on(future),
        }
    } else {
        RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| ExtensionError::operation_failed(err.to_string()))?
            .block_on(future)
    }
}

fn initialise_plugins(system: &Arc<NeoSystem>) -> CoreResult<()> {
    let context = PluginContext::from_environment();
    block_on_extension(initialise_global_runtime(Some(context))).map_err(|err| {
        CoreError::system(format!("failed to initialize plugin runtime: {}", err))
    })?;

    // Auto-register core plugins based on compiled feature set to mirror C# defaults.
    let system_any: Arc<dyn Any + Send + Sync> = system.clone();
    let event = PluginEvent::NodeStarted { system: system_any };
    block_on_extension(broadcast_global_event(&event))
        .map_err(|err| CoreError::system(format!("failed to broadcast NodeStarted: {}", err)))?;
    Ok(())
}

/// Trait implemented by all pluggable Neo plugins.
pub trait Plugin: Send + Sync {
    /// Human readable plugin name.
    fn name(&self) -> &str;

    /// Plugin version string.
    fn version(&self) -> &str;

    /// Called during system start up.
    fn initialize(&mut self, system: &NeoSystem) -> CoreResult<()>;

    /// Called during system shutdown.
    fn shutdown(&mut self) -> CoreResult<()>;
}

/// Container responsible for keeping plugin state aligned with the running system.
#[derive(Default)]
pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
    metadata: HashMap<String, String>,
}

#[cfg(test)]
static TEST_SYSTEM_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

impl PluginManager {
    /// Creates an empty plugin manager instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a plugin and immediately initializes it against the provided system.
    pub fn register_plugin(
        &mut self,
        mut plugin: Box<dyn Plugin>,
        system: &NeoSystem,
    ) -> CoreResult<()> {
        plugin.initialize(system)?;
        self.plugins.push(plugin);
        Ok(())
    }

    /// Invokes shutdown for all registered plugins.
    pub fn shutdown_all(&mut self) {
        for plugin in &mut self.plugins {
            if let Err(err) = plugin.shutdown() {
                warn!(target: "neo", plugin = plugin.name(), error = %err, "failed to shutdown plugin");
            }
        }
        self.plugins.clear();
    }

    /// Provides an arbitrary metadata view for plugins (parity with C# PluginManager).
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    /// Mutable access to plugin metadata allowing callers to keep custom configuration.
    pub fn metadata_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.metadata
    }

    /// Returns a snapshot of the registered plugins (name, version).
    pub fn plugin_infos(&self) -> Vec<(String, String)> {
        self.plugins
            .iter()
            .map(|plugin| (plugin.name().to_string(), plugin.version().to_string()))
            .collect()
    }
}

const RELAY_CACHE_CAPACITY: usize = 100;

#[derive(Clone)]
struct RelayExtensibleEntry {
    hash: UInt256,
    payload: ExtensiblePayload,
}

impl RelayExtensibleEntry {
    fn new(mut payload: ExtensiblePayload) -> Self {
        let hash = payload.hash();
        Self { hash, payload }
    }

    fn payload(&self) -> ExtensiblePayload {
        self.payload.clone()
    }

    fn hash(&self) -> UInt256 {
        self.hash
    }
}

impl InventoryHash<UInt256> for RelayExtensibleEntry {
    fn inventory_hash(&self) -> &UInt256 {
        &self.hash
    }
}

type RelayExtensibleCache = RelayCache<UInt256, RelayExtensibleEntry>;

/// Central runtime coordinating all services for a Neo node.
pub struct NeoSystem {
    settings: ProtocolSettings,
    actor_system: ActorSystem,
    blockchain: ActorRef,
    local_node: ActorRef,
    task_manager: ActorRef,
    tx_router: ActorRef,
    services_by_name: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
    services: Arc<RwLock<Vec<Arc<dyn Any + Send + Sync>>>>,
    service_added_handlers: Arc<RwLock<Vec<Arc<dyn IServiceAddedHandler + Send + Sync>>>>,
    plugin_manager: Arc<RwLock<PluginManager>>,
    store_provider: Arc<dyn IStoreProvider>,
    store: Arc<dyn IStore>,
    ledger: Arc<LedgerContext>,
    genesis_block: Arc<Block>,
    context: Arc<NeoSystemContext>,
    self_ref: Mutex<Weak<NeoSystem>>,
}

/// Named service keys registered into the NeoSystem service registry.
pub const STATE_STORE_SERVICE: &str = "StateStore";

/// Snapshot of basic liveness/sync state for readiness checks.
#[derive(Debug, Clone, Copy)]
pub struct ReadinessStatus {
    pub block_height: u32,
    pub header_height: u32,
    pub header_lag: u32,
    pub healthy: bool,
    pub rpc_ready: bool,
    pub storage_ready: bool,
}

impl ReadinessStatus {
    /// Annotates the readiness snapshot with service readiness and updates the overall health flag.
    pub fn with_services(mut self, rpc_ready: bool, storage_ready: bool) -> Self {
        self.rpc_ready = rpc_ready;
        self.storage_ready = storage_ready;
        self.healthy = self.healthy && rpc_ready && storage_ready;
        self
    }
}

/// Lightweight handle exposing shared system facilities to actors outside the core module.
pub struct NeoSystemContext {
    /// Handle to the underlying Akka system for scheduling and event stream access.
    pub actor_system: ActorSystemHandle,
    /// Reference to the blockchain actor hierarchy root.
    pub blockchain: ActorRef,
    /// Reference to the local node actor (peer supervisor).
    pub local_node: ActorRef,
    /// Reference to the task manager actor coordinating inventory download.
    pub task_manager: ActorRef,
    /// Reference to the transaction router actor.
    pub tx_router: ActorRef,
    /// Global service registry mirrored from the C# implementation.
    pub services_by_name: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
    /// Ordered service list used for type-based lookups.
    pub services: Arc<RwLock<Vec<Arc<dyn Any + Send + Sync>>>>,
    /// Registered callbacks for service additions.
    pub service_added_handlers: Arc<RwLock<Vec<Arc<dyn IServiceAddedHandler + Send + Sync>>>>,
    /// Plugin manager shared state for hot-pluggable extensions.
    pub plugin_manager: Arc<RwLock<PluginManager>>,
    pub wallet_changed_handlers: Arc<RwLock<Vec<Arc<dyn IWalletChangedHandler + Send + Sync>>>>,
    pub current_wallet: Arc<RwLock<Option<Arc<dyn Wallet>>>>,
    /// Store provider used to instantiate persistence backends.
    pub store_provider: Arc<dyn IStoreProvider>,
    /// Active persistence store.
    pub store: Arc<dyn IStore>,
    /// Cached genesis block shared with the blockchain actor.
    genesis_block: Arc<Block>,
    ledger: Arc<LedgerContext>,
    memory_pool: Arc<Mutex<MemoryPool>>,
    header_cache: Arc<HeaderCache>,
    settings: Arc<ProtocolSettings>,
    relay_cache: Arc<RelayExtensibleCache>,
    system: RwLock<Option<Weak<NeoSystem>>>,
    committing_handlers: Arc<RwLock<Vec<Arc<dyn ICommittingHandler + Send + Sync>>>>,
    committed_handlers: Arc<RwLock<Vec<Arc<dyn ICommittedHandler + Send + Sync>>>>,
    transaction_added_handlers: Arc<RwLock<Vec<Arc<dyn ITransactionAddedHandler + Send + Sync>>>>,
    transaction_removed_handlers:
        Arc<RwLock<Vec<Arc<dyn ITransactionRemovedHandler + Send + Sync>>>>,
    log_handlers: Arc<RwLock<Vec<Arc<dyn ILogHandler + Send + Sync>>>>,
    logging_handlers: Arc<RwLock<Vec<Arc<dyn ILoggingHandler + Send + Sync>>>>,
    notify_handlers: Arc<RwLock<Vec<Arc<dyn INotifyHandler + Send + Sync>>>>,
}

impl fmt::Debug for NeoSystemContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NeoSystemContext")
            .field("actor_system", &"ActorSystemHandle")
            .field("blockchain", &self.blockchain)
            .field("local_node", &self.local_node)
            .field("task_manager", &self.task_manager)
            .field("tx_router", &self.tx_router)
            .field("services", &"ServiceRegistry")
            .field("plugin_manager", &"PluginManager")
            .field("store_provider", &"StoreProvider")
            .field("store", &"Store")
            .field("ledger_height", &self.ledger.current_height())
            .field(
                "memory_pool_size",
                &self
                    .memory_pool
                    .lock()
                    .map(|pool| pool.count())
                    .unwrap_or(0usize),
            )
            .field("cached_headers", &self.header_cache.count())
            .finish()
    }
}

impl NeoSystemContext {
    pub fn store_cache(&self) -> StoreCache {
        StoreCache::new_from_store(self.store.clone(), true)
    }

    pub fn store_snapshot_cache(&self) -> StoreCache {
        let snapshot = self.store.get_snapshot();
        StoreCache::new_from_snapshot(snapshot)
    }

    /// Returns the canonical genesis block.
    pub fn genesis_block(&self) -> Arc<Block> {
        self.genesis_block.clone()
    }

    /// Returns the RPC service name for the current network (if configured).
    pub fn rpc_service_name(&self) -> String {
        format!("RpcServer:{}", self.settings.network)
    }

    /// Shared access to the ledger context.
    pub fn ledger(&self) -> Arc<LedgerContext> {
        self.ledger.clone()
    }

    /// Access the registered state store service if present.
    pub fn state_store(&self) -> CoreResult<Option<Arc<StateStore>>> {
        self.get_named_service::<StateStore>(STATE_STORE_SERVICE)
    }

    /// Snapshot of basic readiness (ledger sync) using the provided lag threshold.
    pub fn readiness(&self, max_header_lag: Option<u32>) -> ReadinessStatus {
        self.neo_system()
            .map(|sys| sys.readiness(max_header_lag))
            .unwrap_or(ReadinessStatus {
                block_height: 0,
                header_height: 0,
                header_lag: u32::MAX,
                healthy: false,
                rpc_ready: false,
                storage_ready: false,
            })
    }

    /// Snapshot of readiness annotated with optional service checks (by name) and storage state.
    pub fn readiness_with_services(
        &self,
        max_header_lag: Option<u32>,
        rpc_service_name: Option<&str>,
        storage_ready: Option<bool>,
    ) -> ReadinessStatus {
        let status = self.readiness(max_header_lag);
        let rpc_ready = rpc_service_name
            .map(|name| self.has_named_service(name))
            .unwrap_or(true);
        let storage_ready = storage_ready.unwrap_or(true);
        status.with_services(rpc_ready, storage_ready)
    }

    /// Convenience wrapper that uses the configured RPC service name for this network.
    pub fn readiness_with_defaults(
        &self,
        max_header_lag: Option<u32>,
        storage_ready: Option<bool>,
    ) -> ReadinessStatus {
        self.readiness_with_services(
            max_header_lag,
            Some(&self.rpc_service_name()),
            storage_ready,
        )
    }

    /// Returns `true` when the node is considered ready (sync within the given lag).
    pub fn is_ready(&self, max_header_lag: Option<u32>) -> bool {
        self.readiness(max_header_lag).healthy
    }

    /// Provides access to the shared memory pool handle.
    pub fn memory_pool_handle(&self) -> Arc<Mutex<MemoryPool>> {
        self.memory_pool.clone()
    }

    /// Provides access to the shared header cache.
    pub fn header_cache(&self) -> Arc<HeaderCache> {
        self.header_cache.clone()
    }

    /// Returns a clone of the protocol settings.
    pub fn protocol_settings(&self) -> Arc<ProtocolSettings> {
        self.settings.clone()
    }

    /// Sets a weak reference to the owning NeoSystem.
    pub fn set_system(&self, system: Weak<NeoSystem>) {
        if let Ok(mut guard) = self.system.write() {
            *guard = Some(system);
        }
    }

    /// Attempts to upgrade to a strong reference to the NeoSystem.
    pub fn neo_system(&self) -> Option<Arc<NeoSystem>> {
        self.system
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(|weak| weak.upgrade()))
    }

    pub fn broadcast_plugin_event(&self, event: PluginEvent) {
        if let Err(err) = block_on_extension(broadcast_global_event(&event)) {
            debug!(target: "neo", %err, "failed to broadcast plugin event");
        }
    }

    pub fn register_committing_handler(
        &self,
        handler: Arc<dyn ICommittingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let mut guard = self
            .committing_handlers
            .write()
            .map_err(|_| CoreError::system("committing handler registry poisoned"))?;
        guard.push(handler);
        Ok(())
    }

    pub fn register_committed_handler(
        &self,
        handler: Arc<dyn ICommittedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let mut guard = self
            .committed_handlers
            .write()
            .map_err(|_| CoreError::system("committed handler registry poisoned"))?;
        guard.push(handler);
        Ok(())
    }

    pub fn register_transaction_added_handler(
        &self,
        handler: Arc<dyn ITransactionAddedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let mut guard = self
            .transaction_added_handlers
            .write()
            .map_err(|_| CoreError::system("transaction added handler registry poisoned"))?;
        guard.push(handler);
        Ok(())
    }

    pub fn register_transaction_removed_handler(
        &self,
        handler: Arc<dyn ITransactionRemovedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let mut guard = self
            .transaction_removed_handlers
            .write()
            .map_err(|_| CoreError::system("transaction removed handler registry poisoned"))?;
        guard.push(handler);
        Ok(())
    }

    pub fn register_log_handler(
        &self,
        handler: Arc<dyn ILogHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let mut guard = self
            .log_handlers
            .write()
            .map_err(|_| CoreError::system("log handler registry poisoned"))?;
        guard.push(handler);
        Ok(())
    }

    pub fn register_logging_handler(
        &self,
        handler: Arc<dyn ILoggingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let mut guard = self
            .logging_handlers
            .write()
            .map_err(|_| CoreError::system("logging handler registry poisoned"))?;
        guard.push(handler);
        Ok(())
    }

    pub fn register_notify_handler(
        &self,
        handler: Arc<dyn INotifyHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.notify_handlers
            .write()
            .map_err(|_| CoreError::system("notify handler registry poisoned"))?
            .push(handler);
        Ok(())
    }

    pub fn register_wallet_changed_handler(
        &self,
        handler: Arc<dyn IWalletChangedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let handler_clone = handler.clone();
        self.wallet_changed_handlers
            .write()
            .map_err(|_| CoreError::system("wallet changed handler registry poisoned"))?
            .push(handler);
        let current = self
            .current_wallet
            .read()
            .map_err(|_| CoreError::system("current wallet lock poisoned"))?
            .clone();
        handler_clone.i_wallet_provider_wallet_changed_handler(self, current);
        Ok(())
    }

    pub fn notify_application_log(&self, engine: &ApplicationEngine, args: &LogEventArgs) {
        if let Ok(handlers) = self.log_handlers.read() {
            for handler in handlers.iter() {
                handler.application_engine_log_handler(engine, args);
            }
        }
    }

    pub fn notify_logging_handlers(&self, source: &str, level: LogLevel, message: &str) {
        if let Ok(handlers) = self.logging_handlers.read() {
            for handler in handlers.iter() {
                handler.utility_logging_handler(source, level, message);
            }
        }
    }

    pub fn notify_application_notify(&self, engine: &ApplicationEngine, args: &NotifyEventArgs) {
        if let Ok(handlers) = self.notify_handlers.read() {
            for handler in handlers.iter() {
                handler.application_engine_notify_handler(engine, args);
            }
        }
    }

    pub fn notify_wallet_changed(&self, sender: &dyn Any, wallet: Option<Arc<dyn Wallet>>) {
        if let Ok(mut current) = self.current_wallet.write() {
            *current = wallet.clone();
        }
        if let Ok(handlers) = self.wallet_changed_handlers.read() {
            for handler in handlers.iter() {
                handler.i_wallet_provider_wallet_changed_handler(sender, wallet.clone());
            }
        }
        let wallet_name = wallet
            .as_ref()
            .map(|w| w.name().to_string())
            .unwrap_or_default();
        self.broadcast_plugin_event(PluginEvent::WalletChanged { wallet_name });
    }

    pub fn attach_wallet_provider(
        context: &Arc<Self>,
        provider: Arc<dyn IWalletProvider + Send + Sync>,
    ) -> CoreResult<()> {
        let receiver = provider.wallet_changed();
        let provider_thread = Arc::clone(&provider);
        let weak_context = Arc::downgrade(context);

        thread::Builder::new()
            .name("wallet-provider-listener".to_string())
            .spawn(move || {
                for wallet in receiver {
                    if let Some(ctx) = weak_context.upgrade() {
                        let sender = provider_thread.as_any();
                        ctx.notify_wallet_changed(sender, wallet.clone());
                    } else {
                        break;
                    }
                }
            })
            .map_err(|err| {
                CoreError::system(format!("failed to spawn wallet provider listener: {err}"))
            })?;
        context.notify_wallet_changed(provider.as_any(), provider.get_wallet());
        Ok(())
    }

    pub fn committing_handlers(
        &self,
    ) -> Arc<RwLock<Vec<Arc<dyn ICommittingHandler + Send + Sync>>>> {
        Arc::clone(&self.committing_handlers)
    }

    pub fn committed_handlers(&self) -> Arc<RwLock<Vec<Arc<dyn ICommittedHandler + Send + Sync>>>> {
        Arc::clone(&self.committed_handlers)
    }

    /// Access to the actor system event stream.
    pub fn event_stream(&self) -> EventStreamHandle {
        self.actor_system.event_stream()
    }

    /// Retrieves the first registered service assignable to `T`.
    pub fn get_service<T>(&self) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        let guard = self
            .services
            .read()
            .map_err(|_| CoreError::system("service registry poisoned"))?;

        for service in guard.iter() {
            if let Some(typed) = downcast_service::<T>(service) {
                return Ok(Some(typed));
            }
        }

        Ok(None)
    }

    /// Retrieves a named service assignable to `T`.
    pub fn get_named_service<T>(&self, name: &str) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        let guard = self
            .services_by_name
            .read()
            .map_err(|_| CoreError::system("service registry poisoned"))?;

        match guard.get(name) {
            Some(service) => Ok(downcast_service::<T>(service)),
            None => Ok(None),
        }
    }

    /// Returns `true` if a named service is registered.
    pub fn has_named_service(&self, name: &str) -> bool {
        self.services_by_name
            .read()
            .map(|guard| guard.contains_key(name))
            .unwrap_or(false)
    }

    fn convert_payload_witness(witness: &PayloadWitness) -> crate::Witness {
        crate::Witness::new_with_scripts(
            witness.invocation_script().to_vec(),
            witness.verification_script().to_vec(),
        )
    }

    fn convert_payload_header(header: &Header) -> LedgerBlockHeader {
        LedgerBlockHeader::new(
            header.version(),
            *header.prev_hash(),
            *header.merkle_root(),
            header.timestamp(),
            header.nonce(),
            header.index(),
            header.primary_index(),
            *header.next_consensus(),
            vec![Self::convert_payload_witness(&header.witness)],
        )
    }

    fn convert_payload_block(block: &Block) -> LedgerBlock {
        LedgerBlock::new(
            Self::convert_payload_header(&block.header),
            block.transactions.clone(),
        )
    }

    fn convert_witness(witness: crate::Witness) -> PayloadWitness {
        PayloadWitness::new_with_scripts(
            witness.invocation_script.clone(),
            witness.verification_script.clone(),
        )
    }

    fn convert_ledger_header(header: LedgerBlockHeader) -> Header {
        let LedgerBlockHeader {
            version,
            previous_hash,
            merkle_root,
            timestamp,
            nonce,
            index,
            primary_index,
            next_consensus,
            witnesses,
        } = header;

        let mut converted = Header::new();
        converted.set_version(version);
        converted.set_prev_hash(previous_hash);
        converted.set_merkle_root(merkle_root);
        converted.set_timestamp(timestamp);
        converted.set_nonce(nonce);
        converted.set_index(index);
        converted.set_primary_index(primary_index);
        converted.set_next_consensus(next_consensus);

        let witness = witnesses
            .into_iter()
            .next()
            .unwrap_or_else(crate::Witness::new);
        converted.witness = Self::convert_witness(witness);

        converted
    }

    pub(crate) fn convert_ledger_block(block: LedgerBlock) -> Block {
        Block {
            header: Self::convert_ledger_header(block.header),
            transactions: block.transactions,
        }
    }

    pub(crate) fn hydrate_ledger_from_store(
        store_cache: &StoreCache,
        ledger: &Arc<LedgerContext>,
        header_cache: &HeaderCache,
    ) {
        let ledger_contract = LedgerContract::new();
        let Ok(height) = ledger_contract.current_index(store_cache) else {
            return;
        };

        for index in 0..=height {
            match ledger_contract.get_block(store_cache, HashOrIndex::Index(index)) {
                Ok(Some(block)) => {
                    let payload_block = Self::convert_ledger_block(block);
                    let header_clone = payload_block.header.clone();
                    // insert_block will populate hash/header caches and advance height markers.
                    ledger.insert_block(payload_block);
                    // keep header cache warm for network queries
                    header_cache.add(header_clone);
                }
                Ok(None) => break,
                Err(err) => {
                    warn!(target: "neo", index, error = %err, "failed to hydrate block from store");
                    break;
                }
            }
        }
    }

    /// Returns the current best block index known to the system. This will be wired
    /// to the ledger subsystem once the persistence layer is fully ported.
    pub fn current_block_index(&self) -> u32 {
        self.ledger.current_height()
    }

    /// Attempts to retrieve a transaction from the local mempool or persisted store.
    pub fn try_get_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        if let Some(tx) = self.ledger.get_transaction(hash) {
            return Some(tx);
        }

        let ledger_contract = LedgerContract::new();
        let store_cache = self.store_cache();
        ledger_contract
            .get_transaction_state(&store_cache, hash)
            .ok()
            .flatten()
            .map(|state| state.transaction().clone())
    }

    /// Attempts to retrieve a block by its hash from the local store.
    pub fn try_get_block(&self, hash: &UInt256) -> Option<Block> {
        if let Some(block) = self.ledger.get_block(hash) {
            return Some(block);
        }

        let ledger_contract = LedgerContract::new();
        let store_cache = self.store_cache();
        ledger_contract
            .get_block(&store_cache, HashOrIndex::Hash(*hash))
            .ok()
            .flatten()
            .map(Self::convert_ledger_block)
    }

    /// Attempts to retrieve an extensible payload by hash.
    pub fn try_get_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload> {
        self.ledger.get_extensible(hash)
    }

    /// Returns block hashes starting at the provided hash, limited by `count`.
    pub fn block_hashes_from(&self, hash_start: &UInt256, count: usize) -> Vec<UInt256> {
        let mut hashes = self.ledger.block_hashes_from(hash_start, count);
        if count == 0 {
            return hashes;
        }

        if hashes.len() >= count {
            return hashes;
        }

        let ledger_contract = LedgerContract::new();
        let store_cache = self.store_cache();
        if let Ok(Some(block)) =
            ledger_contract.get_block(&store_cache, HashOrIndex::Hash(*hash_start))
        {
            let mut next_index = block.index().saturating_add(1 + hashes.len() as u32);
            while hashes.len() < count {
                match ledger_contract.get_block_hash_by_index(&store_cache, next_index) {
                    Ok(Some(hash)) => hashes.push(hash),
                    _ => break,
                }
                next_index = next_index.saturating_add(1);
            }
        }

        hashes
    }

    /// Exposes the shared memory pool instance.
    pub fn memory_pool(&self) -> Arc<Mutex<MemoryPool>> {
        self.memory_pool.clone()
    }

    /// Attempts to retrieve a transaction from the in-memory pool without touching persistence.
    pub fn try_get_transaction_from_mempool(&self, hash: &UInt256) -> Option<Transaction> {
        let guard = self.memory_pool.lock().ok()?;
        guard.try_get(hash)
    }

    /// Determines whether a transaction exists in the mempool or persisted store.
    pub fn contains_transaction(&self, hash: &UInt256) -> ContainsTransactionType {
        if let Ok(pool) = self.memory_pool.lock() {
            if pool.contains_key(hash) {
                return ContainsTransactionType::ExistsInPool;
            }
        }

        let ledger_contract = LedgerContract::new();
        let store_cache = self.store_cache();
        if ledger_contract
            .contains_transaction(&store_cache, hash)
            .unwrap_or(false)
        {
            return ContainsTransactionType::ExistsInLedger;
        }

        ContainsTransactionType::NotExist
    }

    /// Determines whether the supplied transaction conflicts with on-chain entries.
    pub fn contains_conflict_hash(&self, hash: &UInt256, signers: &[UInt160]) -> bool {
        if signers.is_empty() {
            return false;
        }

        let ledger_contract = LedgerContract::new();
        let store_cache = self.store_cache();
        let settings = self.settings();
        let max_traceable = ledger_contract
            .max_traceable_blocks_snapshot(&store_cache, &settings)
            .unwrap_or(settings.max_traceable_blocks);

        ledger_contract
            .contains_conflict_hash(&store_cache, hash, signers, max_traceable)
            .unwrap_or(false)
    }

    /// Protocol settings shared with network components.
    pub fn settings(&self) -> Arc<ProtocolSettings> {
        self.settings.clone()
    }

    /// Returns the block hash at the specified index if known.
    pub fn block_hash_at(&self, index: u32) -> Option<UInt256> {
        if let Some(hash) = self.ledger.block_hash_at(index) {
            return Some(hash);
        }

        let ledger_contract = LedgerContract::new();
        let store_cache = self.store_cache();
        ledger_contract
            .get_block_hash_by_index(&store_cache, index)
            .ok()
            .flatten()
    }

    /// Returns headers starting from the supplied index.
    pub fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header> {
        let mut headers = self.ledger.headers_from_index(index_start, count);
        if count == 0 || headers.len() >= count {
            return headers;
        }

        let ledger_contract = LedgerContract::new();
        let store_cache = self.store_cache();
        let mut next_index = index_start.saturating_add(headers.len() as u32);

        while headers.len() < count {
            match ledger_contract.get_block(&store_cache, HashOrIndex::Index(next_index)) {
                Ok(Some(block)) => headers.push(Self::convert_ledger_header(block.header)),
                _ => break,
            }
            next_index = next_index.saturating_add(1);
        }

        headers
    }

    /// Returns transaction hashes currently present in the mempool.
    pub fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        self.ledger.mempool_transaction_hashes()
    }

    /// Adds a transaction to the in-memory pool and returns its hash.
    pub fn record_transaction(&self, transaction: Transaction) -> UInt256 {
        self.ledger.insert_transaction(transaction)
    }

    /// Removes a transaction from the mempool if present.
    pub fn remove_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.ledger.remove_transaction(hash)
    }

    /// Registers a block with the in-memory ledger cache and returns its hash.
    pub fn record_block(&self, block: Block) -> UInt256 {
        self.ledger.insert_block(block)
    }

    /// Registers an extensible payload with the in-memory ledger cache.
    pub fn record_extensible(&self, payload: ExtensiblePayload) -> UInt256 {
        let entry = RelayExtensibleEntry::new(payload.clone());
        let hash = entry.hash();
        self.relay_cache.add(entry);
        self.ledger.insert_extensible(payload);
        hash
    }

    /// Attempts to retrieve a recently relayed extensible payload from the cache.
    pub fn try_get_relay_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload> {
        self.relay_cache.try_get(hash).map(|entry| entry.payload())
    }

    /// Provides shared access to the underlying ledger context for advanced consumers.
    pub fn ledger_handle(&self) -> Arc<LedgerContext> {
        Arc::clone(&self.ledger)
    }
}

fn downcast_service<T>(service: &Arc<dyn Any + Send + Sync>) -> Option<Arc<T>>
where
    T: Any + Send + Sync + 'static,
{
    Arc::downcast::<T>(Arc::clone(service)).ok()
}

impl NeoSystem {
    fn create_genesis_block(settings: &ProtocolSettings) -> Block {
        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(UInt256::zero());
        header.set_merkle_root(UInt256::zero());
        header.set_timestamp(GENESIS_TIMESTAMP_MS);
        header.set_nonce(2_083_236_893u64);
        header.set_index(0);
        header.set_primary_index(0);
        let validators = settings.standby_validators();
        let next_consensus = if validators.is_empty() {
            UInt160::zero()
        } else {
            NativeHelpers::get_bft_address(&validators)
        };
        header.set_next_consensus(next_consensus);
        header.witness = PayloadWitness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1 as u8]);

        Block {
            header,
            transactions: Vec::new(),
        }
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

    /// Persists a block through the minimal smart-contract pipeline, returning
    /// the list of execution summaries produced during processing.
    pub fn persist_block(&self, block: Block) -> CoreResult<Vec<ApplicationExecuted>> {
        let ledger_block = NeoSystemContext::convert_payload_block(&block);
        let mut store_cache = self.context.store_snapshot_cache();
        let base_snapshot = Arc::new(store_cache.data_cache().clone());

        let mut on_persist_engine = ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            Arc::clone(&base_snapshot),
            Some(ledger_block.clone()),
            self.settings.clone(),
            TEST_MODE_GAS,
            None,
        )?;

        on_persist_engine.native_on_persist()?;
        let on_persist_exec = ApplicationExecuted::new(&mut on_persist_engine);
        self.actor_system
            .event_stream()
            .publish(on_persist_exec.clone());

        let mut executed = Vec::with_capacity(ledger_block.transactions.len() + 2);
        executed.push(on_persist_exec);

        let mut tx_states = on_persist_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| {
                LedgerTransactionStates::new(Vec::<PersistedTransactionState>::new())
            });

        for tx in &ledger_block.transactions {
            let tx_snapshot = Arc::new(base_snapshot.as_ref().clone_cache());
            let container: Arc<dyn crate::IVerifiable> = Arc::new(tx.clone());
            let mut tx_engine = ApplicationEngine::new(
                TriggerType::Application,
                Some(container),
                Arc::clone(&tx_snapshot),
                Some(ledger_block.clone()),
                self.settings.clone(),
                tx.system_fee(),
                None,
            )?;

            tx_engine.set_state(tx_states);
            tx_engine.load_script(tx.script().to_vec(), CallFlags::ALL, None)?;
            tx_engine.execute()?;

            let vm_state = tx_engine.state();
            let tx_hash = tx.hash();

            let executed_tx = ApplicationExecuted::new(&mut tx_engine);
            self.actor_system
                .event_stream()
                .publish(executed_tx.clone());
            tx_states = tx_engine
                .take_state::<LedgerTransactionStates>()
                .unwrap_or_else(|| {
                    LedgerTransactionStates::new(Vec::<PersistedTransactionState>::new())
                });
            executed.push(executed_tx);

            if vm_state != VMState::HALT {
                return Err(CoreError::system(format!(
                    "transaction execution halted in state {:?} for hash {}",
                    vm_state, tx_hash
                )));
            }

            let tracked = tx_snapshot.tracked_items();
            base_snapshot.merge_tracked_items(&tracked);
        }

        let mut post_persist_engine = ApplicationEngine::new(
            TriggerType::PostPersist,
            None,
            Arc::clone(&base_snapshot),
            Some(ledger_block.clone()),
            self.settings.clone(),
            TEST_MODE_GAS,
            None,
        )?;
        post_persist_engine.set_state(tx_states);
        post_persist_engine.native_post_persist()?;
        let post_persist_exec = ApplicationExecuted::new(&mut post_persist_engine);
        self.actor_system
            .event_stream()
            .publish(post_persist_exec.clone());
        executed.push(post_persist_exec);

        self.invoke_committing(&ledger_block, base_snapshot.as_ref(), &executed);

        for (key, trackable) in base_snapshot.tracked_items() {
            match trackable.state {
                TrackState::Added => {
                    store_cache.add(key, trackable.item);
                }
                TrackState::Changed => {
                    store_cache.update(key, trackable.item);
                }
                TrackState::Deleted => {
                    store_cache.delete(key);
                }
                TrackState::None | TrackState::NotFound => {}
            }
        }

        store_cache.commit();

        // Update in-memory caches with the payload block so networking queries can respond immediately.
        self.context.record_block(block.clone());

        // Notify plugins that a block has been persisted, matching the C# event ordering.
        let block_hash = ledger_block.hash().to_string();
        let block_height = ledger_block.index();
        self.context
            .broadcast_plugin_event(PluginEvent::BlockReceived {
                block_hash,
                block_height,
            });

        self.invoke_committed(&ledger_block);

        Ok(executed)
    }
}

impl NeoSystem {
    /// Bootstraps the runtime and spawns the core actor hierarchy following the C# layout.
    ///
    /// Mirrors the C# constructor overload that accepts protocol settings,
    /// an optional store provider name and a storage path.
    pub fn new(
        settings: ProtocolSettings,
        storage_provider: Option<Arc<dyn IStoreProvider>>,
        storage_path: Option<String>,
    ) -> CoreResult<Arc<Self>> {
        #[cfg(test)]
        let _test_guard = TEST_SYSTEM_MUTEX.lock().unwrap();

        let actor_system = ActorSystem::new("neo").map_err(to_core_error)?;
        let settings_arc = Arc::new(settings.clone());
        let genesis_block = Arc::new(Self::create_genesis_block(&settings));

        let services_by_name: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let services: Arc<RwLock<Vec<Arc<dyn Any + Send + Sync>>>> =
            Arc::new(RwLock::new(Vec::new()));
        let service_added_handlers: Arc<RwLock<Vec<Arc<dyn IServiceAddedHandler + Send + Sync>>>> =
            Arc::new(RwLock::new(Vec::new()));
        let plugin_manager = Arc::new(RwLock::new(PluginManager::new()));
        let wallet_changed_handlers = Arc::new(RwLock::new(Vec::new()));

        let store_provider = storage_provider.unwrap_or_else(|| {
            StoreFactory::get_store_provider("Memory")
                .expect("default memory store provider must be registered")
        });
        let store = store_provider.get_store(storage_path.as_deref().unwrap_or(""))?;
        let store_cache_for_hydration = StoreCache::new_from_store(store.clone(), true);
        let state_store = Arc::new(StateStore::new_from_store(
            store.clone(),
            StateServiceSettings::default(),
            settings_arc.clone(),
        ));

        let user_agent = format!("/neo-rs:{}/", env!("CARGO_PKG_VERSION"));
        let local_node_state = Arc::new(LocalNode::new(settings_arc.clone(), 10333, user_agent));
        local_node_state.set_seed_list(settings.seed_list.clone());
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        NeoSystemContext::hydrate_ledger_from_store(
            &store_cache_for_hydration,
            &ledger,
            &header_cache,
        );
        let memory_pool = Arc::new(Mutex::new(MemoryPool::new(&settings)));
        let relay_cache = Arc::new(RelayExtensibleCache::new(RELAY_CACHE_CAPACITY));

        {
            let mut by_name_guard = services_by_name
                .write()
                .map_err(|_| CoreError::system("service registry poisoned"))?;
            let mut list_guard = services
                .write()
                .map_err(|_| CoreError::system("service registry poisoned"))?;

            let local_node_any: Arc<dyn Any + Send + Sync> = local_node_state.clone();
            by_name_guard.insert("LocalNode".to_string(), local_node_any.clone());
            list_guard.push(local_node_any);

            let ledger_any: Arc<dyn Any + Send + Sync> = ledger.clone();
            by_name_guard.insert("Ledger".to_string(), ledger_any.clone());
            list_guard.push(ledger_any);

            let mem_pool_any: Arc<dyn Any + Send + Sync> = memory_pool.clone();
            by_name_guard.insert("MemoryPool".to_string(), mem_pool_any.clone());
            list_guard.push(mem_pool_any);

            let state_store_any: Arc<dyn Any + Send + Sync> = state_store.clone();
            by_name_guard.insert(STATE_STORE_SERVICE.to_string(), state_store_any.clone());
            list_guard.push(state_store_any);
        }

        let blockchain = actor_system
            .actor_of(Blockchain::props(ledger.clone()), "blockchain")
            .map_err(to_core_error)?;
        let local_node = actor_system
            .actor_of(LocalNode::props(local_node_state.clone()), "local_node")
            .map_err(to_core_error)?;
        let task_manager = actor_system
            .actor_of(TaskManager::props(), "task_manager")
            .map_err(to_core_error)?;
        let tx_router = actor_system
            .actor_of(
                TransactionRouterActor::props(settings_arc.clone(), blockchain.clone()),
                "tx_router",
            )
            .map_err(to_core_error)?;

        let context = Arc::new(NeoSystemContext {
            actor_system: actor_system.handle(),
            blockchain: blockchain.clone(),
            local_node: local_node.clone(),
            task_manager: task_manager.clone(),
            tx_router: tx_router.clone(),
            services_by_name: services_by_name.clone(),
            services: services.clone(),
            service_added_handlers: service_added_handlers.clone(),
            plugin_manager: plugin_manager.clone(),
            wallet_changed_handlers: wallet_changed_handlers.clone(),
            current_wallet: Arc::new(RwLock::new(None)),
            store_provider: store_provider.clone(),
            store: store.clone(),
            genesis_block: genesis_block.clone(),
            ledger: ledger.clone(),
            memory_pool: memory_pool.clone(),
            header_cache: header_cache.clone(),
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

        NativeHelpers::attach_system_context(context.clone());
        let logging_context = Arc::downgrade(&context);
        Utility::set_logging(Some(Box::new(move |source, level, message| {
            if let Some(ctx) = logging_context.upgrade() {
                let local_level: LogLevel = level.into();
                ctx.notify_logging_handlers(&source, local_level, &message);
            }
        })));

        if let Ok(mut pool) = memory_pool.lock() {
            let context_added = context.clone();
            pool.transaction_added = Some(Box::new(move |sender, tx| {
                if let Ok(handlers) = context_added.transaction_added_handlers.read() {
                    for handler in handlers.iter() {
                        handler.memory_pool_transaction_added_handler(sender, tx);
                    }
                }
                context_added.broadcast_plugin_event(PluginEvent::MempoolTransactionAdded {
                    tx_hash: tx.hash().to_string(),
                });
            }));

            let context_removed = context.clone();
            pool.transaction_removed = Some(Box::new(move |sender, args| {
                if let Ok(handlers) = context_removed.transaction_removed_handlers.read() {
                    for handler in handlers.iter() {
                        handler.memory_pool_transaction_removed_handler(sender, args);
                    }
                }
                let hashes = args
                    .transactions
                    .iter()
                    .map(|tx| tx.hash().to_string())
                    .collect::<Vec<_>>();
                context_removed.broadcast_plugin_event(PluginEvent::MempoolTransactionRemoved {
                    tx_hashes: hashes,
                    reason: format!("{:?}", args.reason),
                });
            }));

            let local_node_ref = local_node.clone();
            let blockchain_ref = blockchain.clone();
            pool.transaction_relay = Some(Box::new(move |tx: &Transaction| {
                if let Err(error) = local_node_ref.tell_from(
                    LocalNodeCommand::RelayDirectly {
                        inventory: RelayInventory::Transaction(tx.clone()),
                        block_index: None,
                    },
                    Some(blockchain_ref.clone()),
                ) {
                    debug!(
                        target: "neo",
                        %error,
                        "failed to enqueue relayed transaction from memory pool"
                    );
                }
            }));
        }

        blockchain
            .tell(BlockchainCommand::AttachSystem(context.clone()))
            .map_err(to_core_error)?;
        local_node_state.set_system_context(context.clone());
        task_manager
            .tell(TaskManagerCommand::AttachSystem {
                context: context.clone(),
            })
            .map_err(to_core_error)?;
        blockchain
            .tell(BlockchainCommand::Initialize)
            .map_err(to_core_error)?;

        let system = Arc::new(Self {
            settings,
            actor_system,
            blockchain,
            local_node,
            task_manager,
            tx_router,
            services,
            services_by_name,
            service_added_handlers,
            plugin_manager,
            store_provider,
            store,
            ledger,
            genesis_block,
            context,
            self_ref: Mutex::new(Weak::new()),
        });

        system.context.set_system(Arc::downgrade(&system));

        {
            let mut weak_guard = system
                .self_ref
                .lock()
                .map_err(|_| CoreError::system("failed to initialise self reference"))?;
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

    /// Starts the local node actor with the supplied networking configuration.
    pub fn start_node(&self, config: ChannelsConfig) -> CoreResult<()> {
        self.local_node
            .tell(PeerCommand::Configure { config })
            .map_err(to_core_error)
    }

    /// Records a new peer within the local node actor.
    pub fn add_peer(
        &self,
        remote_address: SocketAddr,
        listener_tcp_port: Option<u16>,
        version: u32,
        services: u64,
        last_block_index: u32,
    ) -> CoreResult<()> {
        self.local_node
            .tell(LocalNodeCommand::AddPeer {
                remote_address,
                listener_tcp_port,
                version,
                services,
                last_block_index,
            })
            .map_err(to_core_error)
    }

    /// Adds endpoints to the unconnected peer queue (parity with C# `LocalNode.AddPeers`).
    pub fn add_unconnected_peers(&self, endpoints: Vec<SocketAddr>) -> CoreResult<()> {
        self.local_node
            .tell(LocalNodeCommand::AddUnconnectedPeers { endpoints })
            .map_err(to_core_error)
    }

    /// Updates the last reported block height for the specified peer.
    pub fn update_peer_height(
        &self,
        remote_address: SocketAddr,
        last_block_index: u32,
    ) -> CoreResult<()> {
        self.local_node
            .tell(LocalNodeCommand::UpdatePeerHeight {
                remote_address,
                last_block_index,
            })
            .map_err(to_core_error)
    }

    /// Removes the peer and returns whether a record existed.
    pub async fn remove_peer(&self, remote_address: SocketAddr) -> CoreResult<bool> {
        self.ask_local_node(|reply| LocalNodeCommand::RemovePeer {
            remote_address,
            reply,
        })
        .await
    }

    async fn ask_local_node<T>(
        &self,
        builder: impl FnOnce(oneshot::Sender<T>) -> LocalNodeCommand,
    ) -> CoreResult<T>
    where
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let message = builder(tx);
        self.local_node.tell(message).map_err(to_core_error)?;
        rx.await
            .map_err(|_| CoreError::system("local node actor dropped response"))
    }

    /// Returns the number of peers currently tracked by the local node actor.
    pub async fn peer_count(&self) -> CoreResult<usize> {
        self.ask_local_node(|reply| LocalNodeCommand::PeerCount { reply })
            .await
    }

    /// Returns the number of queued unconnected peers.
    pub async fn unconnected_count(&self) -> CoreResult<usize> {
        self.ask_local_node(|reply| LocalNodeCommand::UnconnectedCount { reply })
            .await
    }

    /// Returns the queued unconnected peers.
    pub async fn unconnected_peers(&self) -> CoreResult<Vec<SocketAddr>> {
        self.ask_local_node(|reply| LocalNodeCommand::GetUnconnectedPeers { reply })
            .await
    }

    /// Returns the socket addresses for each connected peer.
    pub async fn peers(&self) -> CoreResult<Vec<SocketAddr>> {
        self.ask_local_node(|reply| LocalNodeCommand::GetPeers { reply })
            .await
    }

    /// Returns detailed snapshots for the connected peers.
    pub async fn remote_node_snapshots(&self) -> CoreResult<Vec<RemoteNodeSnapshot>> {
        self.ask_local_node(|reply| LocalNodeCommand::GetRemoteNodes { reply })
            .await
    }

    /// Returns the maximum reported block height among connected peers.
    pub async fn max_peer_block_height(&self) -> CoreResult<u32> {
        let snapshots = self.remote_node_snapshots().await?;
        Ok(snapshots
            .into_iter()
            .map(|snap| snap.last_block_index)
            .max()
            .unwrap_or(0))
    }

    /// Fetches the shared local node snapshot for advanced operations.
    pub async fn local_node_state(&self) -> CoreResult<Arc<LocalNode>> {
        self.ask_local_node(|reply| LocalNodeCommand::GetInstance { reply })
            .await
    }

    /// Records a relay broadcast via the local node actor.
    pub fn relay_directly(
        &self,
        inventory: RelayInventory,
        block_index: Option<u32>,
    ) -> CoreResult<()> {
        self.local_node
            .tell(LocalNodeCommand::RelayDirectly {
                inventory,
                block_index,
            })
            .map_err(to_core_error)
    }

    /// Records a direct send broadcast via the local node actor.
    pub fn send_directly(
        &self,
        inventory: RelayInventory,
        block_index: Option<u32>,
    ) -> CoreResult<()> {
        self.local_node
            .tell(LocalNodeCommand::SendDirectly {
                inventory,
                block_index,
            })
            .map_err(to_core_error)
    }

    /// Registers an arbitrary service instance for later retrieval (parity with C# `NeoSystem.AddService`).
    pub fn add_service<T, S>(&self, service: S) -> CoreResult<()>
    where
        T: Any + Send + Sync + 'static,
        S: Into<Arc<T>>,
    {
        self.add_service_internal::<T>(None, service.into())
    }

    /// Registers an arbitrary service instance with an explicit name (compatibility helper).
    pub fn add_named_service<T, S>(&self, name: impl Into<String>, service: S) -> CoreResult<()>
    where
        T: Any + Send + Sync + 'static,
        S: Into<Arc<T>>,
    {
        self.add_service_internal::<T>(Some(name.into()), service.into())
    }

    /// Registers a handler invoked before block commit.
    pub fn register_committing_handler(
        &self,
        handler: Arc<dyn ICommittingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context.register_committing_handler(handler)
    }

    /// Registers a handler invoked after block commit completes.
    pub fn register_committed_handler(
        &self,
        handler: Arc<dyn ICommittedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context.register_committed_handler(handler)
    }

    /// Registers a handler invoked when transactions enter the memory pool.
    pub fn register_transaction_added_handler(
        &self,
        handler: Arc<dyn ITransactionAddedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context.register_transaction_added_handler(handler)
    }

    /// Registers a handler invoked when transactions leave the memory pool.
    pub fn register_transaction_removed_handler(
        &self,
        handler: Arc<dyn ITransactionRemovedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context.register_transaction_removed_handler(handler)
    }

    /// Registers a handler for `ApplicationEngine.Log` events.
    pub fn register_log_handler(
        &self,
        handler: Arc<dyn ILogHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context.register_log_handler(handler)
    }

    /// Registers a handler for `Utility.Logging` events.
    pub fn register_logging_handler(
        &self,
        handler: Arc<dyn ILoggingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context.register_logging_handler(handler)
    }

    /// Registers a handler for `ApplicationEngine.Notify` events.
    pub fn register_notify_handler(
        &self,
        handler: Arc<dyn INotifyHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context.register_notify_handler(handler)
    }

    /// Registers a handler for wallet provider changes.
    pub fn register_wallet_changed_handler(
        &self,
        handler: Arc<dyn IWalletChangedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context.register_wallet_changed_handler(handler)
    }

    /// Attaches a wallet provider so wallet-change notifications propagate to handlers.
    pub fn attach_wallet_provider(
        &self,
        provider: Arc<dyn IWalletProvider + Send + Sync>,
    ) -> CoreResult<()> {
        NeoSystemContext::attach_wallet_provider(&self.context, provider)
    }

    fn add_service_internal<T>(&self, name: Option<String>, service: Arc<T>) -> CoreResult<()>
    where
        T: Any + Send + Sync + 'static,
    {
        let service_any: Arc<dyn Any + Send + Sync> = service.clone();

        {
            let mut list = self
                .services
                .write()
                .map_err(|_| CoreError::system("service registry poisoned"))?;
            list.push(service_any.clone());
        }

        if let Some(ref name_ref) = name {
            let mut map = self
                .services_by_name
                .write()
                .map_err(|_| CoreError::system("service registry poisoned"))?;
            map.insert(name_ref.clone(), service_any.clone());
        }

        self.notify_service_added(service_any, name);
        Ok(())
    }

    fn notify_service_added(&self, service: Arc<dyn Any + Send + Sync>, name: Option<String>) {
        let sender: &dyn Any = self;
        if let Ok(handlers) = self.service_added_handlers.read() {
            for handler in handlers.iter() {
                handler.neo_system_service_added_handler(sender, service.as_ref());
            }
        }

        if let Ok(guard) = self.self_ref.lock() {
            if let Some(system) = guard.clone().upgrade() {
                let system_any: Arc<dyn Any + Send + Sync> = system.clone();
                let event = PluginEvent::ServiceAdded {
                    system: system_any,
                    name,
                    service,
                };
                if let Err(err) = block_on_extension(broadcast_global_event(&event)) {
                    warn!("failed to broadcast ServiceAdded event: {}", err);
                }
            }
        }
    }

    fn invoke_committing(
        &self,
        block: &LedgerBlock,
        snapshot: &DataCache,
        application_executed: &[ApplicationExecuted],
    ) {
        if let Ok(handlers) = self.context.committing_handlers().read() {
            for handler in handlers.iter() {
                handler.blockchain_committing_handler(self, block, snapshot, application_executed);
            }
        }
    }

    fn invoke_committed(&self, block: &LedgerBlock) {
        if let Ok(handlers) = self.context.committed_handlers().read() {
            for handler in handlers.iter() {
                handler.blockchain_committed_handler(self, block);
            }
        }
    }

    /// Registers a handler to be notified when services are added.
    pub fn register_service_added_handler(
        &self,
        handler: Arc<dyn IServiceAddedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let mut guard = self
            .service_added_handlers
            .write()
            .map_err(|_| CoreError::system("service handler registry poisoned"))?;
        guard.push(handler);
        Ok(())
    }

    /// Returns the RPC service name for the current network (if configured).
    pub fn rpc_service_name(&self) -> String {
        format!("RpcServer:{}", self.settings.network)
    }

    /// Retrieves the first registered service assignable to `T`.
    pub fn get_service<T>(&self) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        let guard = self
            .services
            .read()
            .map_err(|_| CoreError::system("service registry poisoned"))?;

        for service in guard.iter() {
            if let Some(typed) = downcast_service::<T>(service) {
                return Ok(Some(typed));
            }
        }

        Ok(None)
    }

    /// Retrieves a previously registered service by name.
    pub fn get_named_service<T>(&self, name: &str) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        let guard = self
            .services_by_name
            .read()
            .map_err(|_| CoreError::system("service registry poisoned"))?;

        match guard.get(name) {
            Some(service) => Ok(downcast_service::<T>(service)),
            None => Ok(None),
        }
    }

    /// Returns `true` if a named service is registered.
    pub fn has_named_service(&self, name: &str) -> bool {
        self.services_by_name
            .read()
            .map(|guard| guard.contains_key(name))
            .unwrap_or(false)
    }

    /// Basic readiness snapshot (ledger sync only). Consumers can layer on service checks (RPC, storage, etc.).
    pub fn readiness(&self, max_header_lag: Option<u32>) -> ReadinessStatus {
        let ledger = self.ledger_context();
        let block_height = ledger.current_height();
        let header_height = ledger.highest_header_index();
        let header_lag = header_height.saturating_sub(block_height);
        let healthy = max_header_lag
            .map(|threshold| header_lag <= threshold)
            .unwrap_or(true);

        ReadinessStatus {
            block_height,
            header_height,
            header_lag,
            healthy,
            rpc_ready: true,
            storage_ready: true,
        }
    }

    /// Readiness snapshot annotated with optional service and storage readiness flags.
    pub fn readiness_with_services(
        &self,
        max_header_lag: Option<u32>,
        rpc_service_name: Option<&str>,
        storage_ready: Option<bool>,
    ) -> ReadinessStatus {
        let status = self.readiness(max_header_lag);
        let rpc_ready = rpc_service_name
            .map(|name| self.has_named_service(name))
            .unwrap_or(true);
        let storage_ready = storage_ready.unwrap_or(true);
        status.with_services(rpc_ready, storage_ready)
    }

    /// Convenience wrapper that uses the configured RPC service name for this network.
    pub fn readiness_with_defaults(
        &self,
        max_header_lag: Option<u32>,
        storage_ready: Option<bool>,
    ) -> ReadinessStatus {
        self.readiness_with_services(
            max_header_lag,
            Some(&self.rpc_service_name()),
            storage_ready,
        )
    }

    /// Returns `true` when the node is considered ready (sync within the given lag).
    pub fn is_ready(&self, max_header_lag: Option<u32>) -> bool {
        self.readiness(max_header_lag).healthy
    }

    /// Access to the plugin manager for registering plugins.
    pub fn plugin_manager(&self) -> Arc<RwLock<PluginManager>> {
        self.plugin_manager.clone()
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
        let event = PluginEvent::NodeStopping;
        if let Err(err) = broadcast_global_event(&event).await {
            warn!("failed to broadcast NodeStopping event: {}", err);
        }
        if let Ok(mut manager) = self.context.plugin_manager.write() {
            manager.shutdown_all();
        }
        if let Err(err) = shutdown_global_runtime().await {
            warn!("failed to shutdown plugin runtime: {}", err);
        }
        // Drop the global logging hook to avoid leaking callbacks across system lifetimes.
        Utility::set_logging(None);
        timeouts::log_stats();
        self.actor_system.shutdown().await.map_err(to_core_error)
    }
}

fn to_core_error(err: akka::AkkaError) -> CoreError {
    CoreError::system(err.to_string())
}

// === Actor definitions ====================================================

struct TransactionRouterActor {
    settings: Arc<ProtocolSettings>,
    blockchain: ActorRef,
}

impl TransactionRouterActor {
    fn new(settings: Arc<ProtocolSettings>, blockchain: ActorRef) -> Self {
        Self {
            settings,
            blockchain,
        }
    }

    fn props(settings: Arc<ProtocolSettings>, blockchain: ActorRef) -> Props {
        Props::new(move || Self::new(settings.clone(), blockchain.clone()))
    }
}

#[async_trait]
impl Actor for TransactionRouterActor {
    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match envelope.downcast::<TransactionRouterMessage>() {
            Ok(message) => {
                match *message {
                    TransactionRouterMessage::Preverify { transaction, relay } => {
                        let result = transaction.verify_state_independent(&self.settings);
                        let completed = PreverifyCompleted {
                            transaction,
                            relay,
                            result,
                        };
                        if let Err(error) = self.blockchain.tell_from(
                            BlockchainCommand::PreverifyCompleted(completed),
                            ctx.sender(),
                        ) {
                            warn!(target: "neo", %error, "failed to deliver preverify result to blockchain actor");
                        }
                    }
                }
                Ok(())
            }
            Err(payload) => {
                warn!(
                    target: "neo",
                    message_type = ?(*payload).type_id(),
                    "unknown message routed to transaction router actor"
                );
                Ok(())
            }
        }
    }
}

// === Actor Messages ======================================================

#[derive(Debug)]
pub enum TransactionRouterMessage {
    Preverify {
        transaction: Transaction,
        relay: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{
        transaction_removal_reason::TransactionRemovalReason,
        transaction_removed_event_args::TransactionRemovedEventArgs, Block,
    };
    use crate::neo_io::Serializable;
    use crate::network::p2p::payloads::witness::Witness as PayloadWitness;
    use crate::network::p2p::payloads::Transaction;
    use crate::persistence::i_store::IStore;
    use crate::persistence::providers::memory_store::MemoryStore;
    use crate::persistence::StoreCache;
    use crate::smart_contract::contract::Contract;
    use crate::smart_contract::native::trimmed_block::TrimmedBlock;
    use crate::smart_contract::notify_event_args::NotifyEventArgs;
    use crate::wallets::key_pair::KeyPair;
    use crate::wallets::{Version, Wallet, WalletAccount, WalletError, WalletResult};
    use crate::Witness;
    use crate::{IVerifiable, UInt160, UInt256};
    use async_trait::async_trait;
    use lazy_static::lazy_static;
    use neo_vm::StackItem;
    use std::any::Any;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use tokio::time::{sleep, timeout, Duration};

    lazy_static! {
        static ref LOG_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    #[test]
    fn hydrate_ledger_from_empty_store_is_noop() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let store_cache = StoreCache::new_from_store(store, true);
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = HeaderCache::new();

        NeoSystemContext::hydrate_ledger_from_store(&store_cache, &ledger, &header_cache);

        assert_eq!(ledger.current_height(), 0);
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn hydrate_ledger_restores_height_and_headers() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let mut snapshot = store.get_snapshot();
        let snapshot = Arc::get_mut(&mut snapshot).expect("mutable snapshot");

        // Persist two blocks (genesis index 0 and block index 1).
        let mut persist_block = |index: u32, nonce: u64| {
            let header = crate::ledger::block_header::BlockHeader {
                index,
                timestamp: index as u64,
                nonce,
                witnesses: vec![Witness::new()],
                ..Default::default()
            };
            let block = Block {
                header: header.clone(),
                transactions: Vec::new(),
            };
            let hash = block.hash();

            let key = crate::smart_contract::native::ledger_contract::keys::block_hash_storage_key(
                -4, index,
            )
            .to_array();
            snapshot.put(key, hash.to_bytes().to_vec());

            let block_key =
                crate::smart_contract::native::ledger_contract::keys::block_storage_key(-4, &hash)
                    .to_array();
            let mut writer = crate::neo_io::BinaryWriter::new();
            let trimmed = TrimmedBlock::from_block(&block);
            trimmed
                .serialize(&mut writer)
                .expect("trimmed block serialize");
            snapshot.put(block_key, writer.to_bytes());
            hash
        };

        let _genesis_hash = persist_block(0, 1);
        let hash = persist_block(1, 42);

        // Persist current block pointer.
        let current_key =
            crate::smart_contract::native::ledger_contract::keys::current_block_storage_key(-4)
                .to_array();
        let mut current_state = Vec::with_capacity(36);
        current_state.extend_from_slice(&hash.to_bytes());
        current_state.extend_from_slice(&1u32.to_le_bytes());
        snapshot.put(current_key, current_state);
        snapshot.commit();

        let store_cache = StoreCache::new_from_store(store, true);
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = HeaderCache::new();

        NeoSystemContext::hydrate_ledger_from_store(&store_cache, &ledger, &header_cache);
        assert_eq!(ledger.current_height(), 1);
        assert_eq!(header_cache.count(), 2);
        assert_eq!(ledger.block_hash_at(1), Some(hash));
    }

    fn sample_u256(byte: u8) -> UInt256 {
        UInt256::from_bytes(&[byte; 32]).expect("uint256 from bytes")
    }

    fn sample_u160(byte: u8) -> UInt160 {
        UInt160::from_bytes(&[byte; 20]).expect("uint160 from bytes")
    }

    fn sample_ledger_header() -> LedgerBlockHeader {
        let witness = crate::Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        LedgerBlockHeader {
            version: 1,
            previous_hash: sample_u256(1),
            merkle_root: sample_u256(2),
            timestamp: 42,
            nonce: 7,
            index: 10,
            primary_index: 3,
            next_consensus: sample_u160(5),
            witnesses: vec![witness],
        }
    }

    #[test]
    fn convert_ledger_header_preserves_fields() {
        let header = sample_ledger_header();
        let converted = NeoSystemContext::convert_ledger_header(header.clone());

        assert_eq!(converted.version(), 1);
        assert_eq!(converted.prev_hash(), &sample_u256(1));
        assert_eq!(converted.merkle_root(), &sample_u256(2));
        assert_eq!(converted.timestamp(), 42);
        assert_eq!(converted.nonce(), 7);
        assert_eq!(converted.index(), 10);
        assert_eq!(converted.primary_index(), 3);
        assert_eq!(converted.next_consensus(), &sample_u160(5));

        let expected_witness = PayloadWitness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        assert_eq!(
            converted.witness.invocation_script,
            expected_witness.invocation_script
        );
        assert_eq!(
            converted.witness.verification_script,
            expected_witness.verification_script
        );

        assert_eq!(header.witnesses.len(), 1);
    }

    #[test]
    fn convert_ledger_block_transfers_transactions() {
        let header = sample_ledger_header();
        let txs = Vec::new();
        let ledger_block = LedgerBlock {
            header,
            transactions: txs.clone(),
        };
        let block = NeoSystemContext::convert_ledger_block(ledger_block);
        assert_eq!(block.transactions.len(), txs.len());
        assert_eq!(block.header.index(), 10);
    }

    #[derive(Default)]
    struct EventProbe {
        added: AtomicUsize,
        removed: AtomicUsize,
        logs: AtomicUsize,
        logging: AtomicUsize,
        notify: AtomicUsize,
        wallet_changes: AtomicUsize,
    }

    impl EventProbe {
        fn added(&self) -> usize {
            self.added.load(Ordering::Relaxed)
        }

        fn removed(&self) -> usize {
            self.removed.load(Ordering::Relaxed)
        }

        fn logs(&self) -> usize {
            self.logs.load(Ordering::Relaxed)
        }

        fn logging(&self) -> usize {
            self.logging.load(Ordering::Relaxed)
        }

        fn notifies(&self) -> usize {
            self.notify.load(Ordering::Relaxed)
        }

        fn wallet_changes(&self) -> usize {
            self.wallet_changes.load(Ordering::Relaxed)
        }
    }

    impl ITransactionAddedHandler for EventProbe {
        fn memory_pool_transaction_added_handler(&self, _sender: &dyn Any, _tx: &Transaction) {
            self.added.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ITransactionRemovedHandler for EventProbe {
        fn memory_pool_transaction_removed_handler(
            &self,
            _sender: &dyn Any,
            _args: &TransactionRemovedEventArgs,
        ) {
            self.removed.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ILogHandler for EventProbe {
        fn application_engine_log_handler(
            &self,
            _sender: &ApplicationEngine,
            _log_event_args: &LogEventArgs,
        ) {
            self.logs.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ILoggingHandler for EventProbe {
        fn utility_logging_handler(&self, _source: &str, _level: LogLevel, _message: &str) {
            self.logging.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl INotifyHandler for EventProbe {
        fn application_engine_notify_handler(
            &self,
            _sender: &ApplicationEngine,
            _notify_event_args: &NotifyEventArgs,
        ) {
            self.notify.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct LoggingGuard;

    impl LoggingGuard {
        fn install<F>(hook: F) -> Self
        where
            F: Fn(String, ExternalLogLevel, String) + Send + Sync + 'static,
        {
            Utility::set_logging(Some(Box::new(hook)));
            Self
        }
    }

    impl Drop for LoggingGuard {
        fn drop(&mut self) {
            Utility::set_logging(None);
        }
    }

    impl IWalletChangedHandler for EventProbe {
        fn i_wallet_provider_wallet_changed_handler(
            &self,
            _sender: &dyn Any,
            _wallet: Option<Arc<dyn Wallet>>,
        ) {
            let _ = _wallet;
            self.wallet_changes.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[derive(Default)]
    struct DummyWallet {
        version: Version,
    }

    #[async_trait]
    impl Wallet for DummyWallet {
        fn name(&self) -> &str {
            "dummy"
        }

        fn path(&self) -> Option<&str> {
            None
        }

        fn version(&self) -> &Version {
            &self.version
        }

        async fn change_password(
            &self,
            _old_password: &str,
            _new_password: &str,
        ) -> WalletResult<bool> {
            Ok(false)
        }

        fn contains(&self, _script_hash: &UInt160) -> bool {
            false
        }

        async fn create_account(
            &self,
            _private_key: &[u8],
        ) -> WalletResult<Arc<dyn WalletAccount>> {
            Err(WalletError::Other("not implemented".to_string()))
        }

        async fn create_account_with_contract(
            &self,
            _contract: Contract,
            _key_pair: Option<KeyPair>,
        ) -> WalletResult<Arc<dyn WalletAccount>> {
            Err(WalletError::Other("not implemented".to_string()))
        }

        async fn create_account_watch_only(
            &self,
            _script_hash: UInt160,
        ) -> WalletResult<Arc<dyn WalletAccount>> {
            Err(WalletError::Other("not implemented".to_string()))
        }

        async fn delete_account(&self, _script_hash: &UInt160) -> WalletResult<bool> {
            Ok(false)
        }

        async fn export(&self, _path: &str, _password: &str) -> WalletResult<()> {
            Err(WalletError::Other("not implemented".to_string()))
        }

        fn get_account(&self, _script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>> {
            None
        }

        fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>> {
            Vec::new()
        }

        async fn get_available_balance(&self, _asset_id: &UInt256) -> WalletResult<i64> {
            Ok(0)
        }

        async fn get_unclaimed_gas(&self) -> WalletResult<i64> {
            Ok(0)
        }

        async fn import_wif(&self, _wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
            Err(WalletError::Other("not implemented".to_string()))
        }

        async fn import_nep2(
            &self,
            _nep2_key: &str,
            _password: &str,
        ) -> WalletResult<Arc<dyn WalletAccount>> {
            Err(WalletError::Other("not implemented".to_string()))
        }

        fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
            None
        }

        async fn set_default_account(&self, _script_hash: &UInt160) -> WalletResult<()> {
            Ok(())
        }

        async fn sign(&self, _data: &[u8], _script_hash: &UInt160) -> WalletResult<Vec<u8>> {
            Err(WalletError::Other("not implemented".to_string()))
        }

        async fn sign_transaction(&self, _transaction: &mut Transaction) -> WalletResult<()> {
            Ok(())
        }

        async fn unlock(&self, _password: &str) -> WalletResult<bool> {
            Ok(true)
        }

        fn lock(&self) {}

        async fn verify_password(&self, _password: &str) -> WalletResult<bool> {
            Ok(true)
        }

        async fn save(&self) -> WalletResult<()> {
            Ok(())
        }
    }

    struct TestWalletProvider {
        #[allow(clippy::type_complexity)]
        receiver: Mutex<Option<mpsc::Receiver<Option<Arc<dyn Wallet>>>>>,
    }

    impl TestWalletProvider {
        fn new() -> (Arc<Self>, mpsc::Sender<Option<Arc<dyn Wallet>>>) {
            let (tx, rx) = mpsc::channel();
            let provider = Arc::new(Self {
                receiver: Mutex::new(Some(rx)),
            });
            (provider, tx)
        }
    }

    impl IWalletProvider for TestWalletProvider {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn wallet_changed(&self) -> mpsc::Receiver<Option<Arc<dyn Wallet>>> {
            self.receiver
                .lock()
                .unwrap()
                .take()
                .expect("wallet changed receiver already taken")
        }

        fn get_wallet(&self) -> Option<Arc<dyn Wallet>> {
            None
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn local_node_defaults_match_csharp() {
        let mut settings = ProtocolSettings::default();
        settings.seed_list.clear();
        let system = NeoSystem::new(settings, None, None).expect("system to start");

        let config = ChannelsConfig {
            min_desired_connections: 0,
            max_connections: 0,
            ..Default::default()
        };

        system.start_node(config).expect("start local node");
        sleep(Duration::from_millis(50)).await;

        let snapshot = system
            .local_node_state()
            .await
            .expect("local node snapshot");
        assert_eq!(snapshot.port(), 0);
        assert_eq!(snapshot.connected_peers_count(), 0);
        assert!(snapshot.remote_nodes().is_empty());
        assert!(system.peers().await.expect("peer query").is_empty());
        assert_eq!(system.unconnected_count().await.expect("unconnected"), 0);

        let _ = system.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn add_unconnected_peers_tracks_queue() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");

        let endpoints: Vec<SocketAddr> = vec![
            "127.0.0.1:20000".parse().unwrap(),
            "127.0.0.1:20001".parse().unwrap(),
        ];

        system
            .add_unconnected_peers(endpoints.clone())
            .expect("enqueue peers");

        let count = system.unconnected_count().await.expect("unconnected count");
        assert_eq!(count, endpoints.len());

        let mut returned = system.unconnected_peers().await.expect("unconnected peers");
        returned.sort();

        let mut expected = endpoints;
        expected.sort();
        assert_eq!(returned, expected);

        let _ = system.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn transaction_event_handlers_receive_callbacks() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let handler = Arc::new(EventProbe::default());

        system
            .register_transaction_added_handler(handler.clone())
            .expect("register added");
        system
            .register_transaction_removed_handler(handler.clone())
            .expect("register removed");

        let tx = Transaction::default();
        let pool = system.mempool();
        let args = TransactionRemovedEventArgs {
            transactions: vec![tx.clone()],
            reason: TransactionRemovalReason::CapacityExceeded,
        };

        {
            let guard = pool.lock().expect("lock mempool");
            if let Some(callback) = &guard.transaction_added {
                callback(&guard, &tx);
            }
            if let Some(callback) = &guard.transaction_removed {
                callback(&guard, &args);
            }
        }

        assert_eq!(handler.added(), 1);
        assert_eq!(handler.removed(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn log_and_logging_handlers_fire() {
        let _log_guard = LOG_TEST_MUTEX.lock().unwrap();

        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let handler = Arc::new(EventProbe::default());

        system
            .register_log_handler(handler.clone())
            .expect("register log handler");
        system
            .register_logging_handler(handler.clone())
            .expect("register logging handler");
        system
            .register_notify_handler(handler.clone())
            .expect("register notify handler");

        let system_ctx = system.context();
        NativeHelpers::attach_system_context(system_ctx.clone());
        let logging_ctx = Arc::downgrade(&system_ctx);
        let _logging_guard = LoggingGuard::install(move |source, level, message| {
            if let Some(ctx) = logging_ctx.upgrade() {
                let local_level: LogLevel = level.into();
                ctx.notify_logging_handlers(&source, local_level, &message);
            }
        });

        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
        .expect("engine");
        engine.set_runtime_context(Some(system_ctx.clone()));

        // Push a log through the engine and invoke the logging hook.
        let container: Arc<dyn IVerifiable> = Arc::new(Transaction::default());
        let log_event = LogEventArgs::new(container, UInt160::default(), "hello".to_string());
        engine.push_log(log_event);
        Utility::set_log_level(ExternalLogLevel::Info);
        Utility::log("test", ExternalLogLevel::Info, "message");
        // Exercise both the Utility hook and direct notify; the hook increments log_counter.
        system_ctx.notify_logging_handlers("test", LogLevel::Info, "message");
        assert_eq!(handler.logs(), 1);
        assert!(handler.logging() >= 1);

        let notify = NotifyEventArgs::new(
            Arc::new(Transaction::default()) as Arc<dyn IVerifiable>,
            UInt160::default(),
            "evt".to_string(),
            vec![StackItem::from_int(1)],
        );
        engine.push_notification(notify);
        assert_eq!(handler.notifies(), 1);

        Utility::set_logging(None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn wallet_changed_handlers_receive_events() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let handler = Arc::new(EventProbe::default());
        system
            .register_wallet_changed_handler(handler.clone())
            .expect("register wallet handler");

        let (provider, tx) = TestWalletProvider::new();
        system
            .attach_wallet_provider(provider)
            .expect("attach wallet provider");

        tx.send(Some(Arc::new(DummyWallet::default()) as Arc<dyn Wallet>))
            .expect("send wallet");

        timeout(Duration::from_secs(1), async {
            while handler.wallet_changes() == 0 {
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("wallet handler triggered");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn local_node_actor_tracks_peers() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let endpoint: SocketAddr = "127.0.0.1:20333".parse().unwrap();

        system
            .add_peer(endpoint, Some(20333), 0, 0, 0)
            .expect("add peer should succeed");

        assert_eq!(system.peer_count().await.unwrap(), 1);
        let peers = system.peers().await.unwrap();
        assert_eq!(peers, vec![endpoint]);

        let snapshots = system.remote_node_snapshots().await.unwrap();
        assert_eq!(snapshots.len(), 1);

        assert!(system.remove_peer(endpoint).await.unwrap());
        assert_eq!(system.peer_count().await.unwrap(), 0);

        system.shutdown().await.expect("shutdown succeeds");
    }
}
