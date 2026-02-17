//! Shared runtime context for `NeoSystem`.
//!
//! This module exposes handles to core actors, services, and caches that are
//! shared across the node. It encapsulates read-only accessors, event
//! registration, and data retrieval helpers while keeping the heavy orchestration
//! logic in `core.rs`.

use parking_lot::{Mutex, RwLock};
use std::any::Any;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::thread;

use crate::akka::{ActorRef, ActorSystemHandle, EventStreamHandle};
use tracing::{trace, warn};

use super::converters::{convert_ledger_block, convert_ledger_header};
use super::registry::ServiceRegistry;
use super::relay::{RelayExtensibleCache, RelayExtensibleEntry, LEDGER_HYDRATION_WINDOW};
use super::system::{ReadinessStatus, STATE_STORE_SERVICE};
use crate::contains_transaction_type::ContainsTransactionType;
use crate::error::{CoreError, CoreResult};
use crate::events::{broadcast_plugin_event, PluginEvent};
use crate::extensions::log_level::LogLevel;
use crate::i_event_handlers::{
    ICommittedHandler, ICommittingHandler, ILogHandler, ILoggingHandler, INotifyHandler,
    IServiceAddedHandler, ITransactionAddedHandler, ITransactionRemovedHandler,
    IWalletChangedHandler,
};
use crate::ledger::{HeaderCache, LedgerContext, MemoryPool};
use crate::network::p2p::{
    payloads::{
        block::Block, extensible_payload::ExtensiblePayload, header::Header,
        transaction::Transaction,
    },
    LocalNode,
};
use crate::persistence::{i_store::IStore, i_store_provider::IStoreProvider, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::services::SystemContext;
use crate::services::{
    LedgerService, LockedMempoolService, MempoolService, PeerManagerService, StateStoreService,
};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::log_event_args::LogEventArgs;
use crate::smart_contract::native::ledger_contract::{HashOrIndex, LedgerContract};
use crate::smart_contract::notify_event_args::NotifyEventArgs;
use crate::state_service::StateStore;
use crate::wallets::{IWalletProvider, Wallet};
use neo_primitives::{UInt160, UInt256};

use super::core::NeoSystem;

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
    pub(crate) service_registry: Arc<ServiceRegistry>,
    /// Registered callbacks for service additions.
    pub service_added_handlers: Arc<RwLock<Vec<Arc<dyn IServiceAddedHandler + Send + Sync>>>>,
    /// Registered callbacks for wallet changes.
    pub wallet_changed_handlers: Arc<RwLock<Vec<Arc<dyn IWalletChangedHandler + Send + Sync>>>>,
    /// Currently active wallet, if any.
    pub current_wallet: Arc<RwLock<Option<Arc<dyn Wallet>>>>,
    /// Store provider used to instantiate persistence backends.
    pub store_provider: Arc<dyn IStoreProvider>,
    /// Active persistence store.
    pub store: Arc<dyn IStore>,
    /// Cached genesis block shared with the blockchain actor.
    pub(crate) genesis_block: Arc<Block>,
    pub(crate) ledger: Arc<LedgerContext>,
    pub(crate) state_service_enabled: bool,
    pub(crate) state_store: Arc<StateStore>,
    pub(crate) memory_pool: Arc<Mutex<MemoryPool>>,
    pub(crate) header_cache: Arc<HeaderCache>,
    pub(crate) local_node_state: Arc<LocalNode>,
    pub(crate) settings: Arc<ProtocolSettings>,
    pub(crate) relay_cache: Arc<RelayExtensibleCache>,
    pub(crate) system: RwLock<Option<Weak<NeoSystem>>>,
    pub(crate) committing_handlers: Arc<RwLock<Vec<Arc<dyn ICommittingHandler + Send + Sync>>>>,
    pub(crate) committed_handlers: Arc<RwLock<Vec<Arc<dyn ICommittedHandler + Send + Sync>>>>,
    pub(crate) transaction_added_handlers:
        Arc<RwLock<Vec<Arc<dyn ITransactionAddedHandler + Send + Sync>>>>,
    pub(crate) transaction_removed_handlers:
        Arc<RwLock<Vec<Arc<dyn ITransactionRemovedHandler + Send + Sync>>>>,
    pub(crate) log_handlers: Arc<RwLock<Vec<Arc<dyn ILogHandler + Send + Sync>>>>,
    pub(crate) logging_handlers: Arc<RwLock<Vec<Arc<dyn ILoggingHandler + Send + Sync>>>>,
    pub(crate) notify_handlers: Arc<RwLock<Vec<Arc<dyn INotifyHandler + Send + Sync>>>>,
    /// Fast sync mode - disables expensive event publishing during initial sync
    pub(crate) fast_sync_mode: Arc<AtomicBool>,
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
            .field("store_provider", &"StoreProvider")
            .field("store", &"Store")
            .field("ledger_height", &self.ledger.current_height())
            .field("memory_pool_size", &self.memory_pool.lock().count())
            .field("cached_headers", &self.header_cache.count())
            .finish()
    }
}

impl NeoSystemContext {
    /// Retrieves a service by name using a typed handle backed by the registry.
    fn typed_service<T>(&self, name: &str) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        if let Some(service) = self.service_registry.get_typed::<T>()? {
            return Ok(Some(service));
        }
        self.service_registry.get_named_service::<T>(name)
    }

    pub fn store_cache(&self) -> StoreCache {
        StoreCache::new_from_store(self.store.clone(), false)
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

    /// Access the registered RPC service for this network if present.
    pub fn rpc_service<T>(&self) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        self.typed_service::<T>(&self.rpc_service_name())
    }

    /// Shared access to the ledger context.
    pub fn ledger(&self) -> Arc<LedgerContext> {
        self.ledger.clone()
    }

    /// Access the registered state store service if present.
    pub fn state_store(&self) -> CoreResult<Option<Arc<StateStore>>> {
        if !self.state_service_enabled {
            return Ok(None);
        }
        if let Some(service) = self.typed_service::<StateStore>(STATE_STORE_SERVICE)? {
            return Ok(Some(service));
        }
        Ok(Some(self.state_store.clone()))
    }

    /// Typed state store view.
    pub fn state_store_typed(&self) -> CoreResult<Option<Arc<dyn StateStoreService>>> {
        Ok(self
            .state_store()?
            .map(|svc| svc as Arc<dyn StateStoreService>))
    }

    /// Access the shared ledger context service if present.
    pub fn ledger_service(&self) -> CoreResult<Option<Arc<LedgerContext>>> {
        if let Some(service) = self.typed_service::<LedgerContext>("Ledger")? {
            return Ok(Some(service));
        }
        Ok(Some(self.ledger.clone()))
    }

    /// Typed ledger view.
    pub fn ledger_typed(&self) -> CoreResult<Option<Arc<dyn LedgerService>>> {
        Ok(self
            .ledger_service()?
            .map(|svc| svc as Arc<dyn LedgerService>))
    }

    /// Access the shared memory pool service if present.
    pub fn mempool_service(&self) -> CoreResult<Option<Arc<Mutex<MemoryPool>>>> {
        if let Some(service) = self.typed_service::<Mutex<MemoryPool>>("MemoryPool")? {
            return Ok(Some(service));
        }
        Ok(Some(self.memory_pool.clone()))
    }

    /// Typed mempool service view for consumers that only need trait methods.
    pub fn mempool_typed(
        &self,
    ) -> CoreResult<Option<Arc<dyn MempoolService + Send + Sync + 'static>>> {
        Ok(self.mempool_service()?.map(|svc| {
            Arc::new(LockedMempoolService::new(svc)) as Arc<dyn MempoolService + Send + Sync>
        }))
    }

    /// Access the registered local node state service if present.
    pub fn local_node_service(&self) -> CoreResult<Option<Arc<LocalNode>>> {
        if let Some(service) = self.typed_service::<LocalNode>("LocalNode")? {
            return Ok(Some(service));
        }
        Ok(Some(self.local_node_state.clone()))
    }

    /// Check if fast sync mode is enabled (skips expensive event publishing).
    pub fn is_fast_sync_mode(&self) -> bool {
        self.fast_sync_mode.load(Ordering::Relaxed)
    }

    /// Enable fast sync mode for initial chain synchronization.
    pub fn enable_fast_sync_mode(&self) {
        self.fast_sync_mode.store(true, Ordering::Relaxed);
    }

    /// Disable fast sync mode (e.g., after initial sync complete).
    pub fn disable_fast_sync_mode(&self) {
        self.fast_sync_mode.store(false, Ordering::Relaxed);
    }

    /// Typed peer manager view.
    pub fn peer_manager_typed(&self) -> CoreResult<Option<Arc<dyn PeerManagerService>>> {
        Ok(self
            .local_node_service()?
            .map(|svc| svc as Arc<dyn PeerManagerService>))
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
        *self.system.write() = Some(system);
    }

    /// Attempts to upgrade to a strong reference to the NeoSystem.
    ///
    /// Returns `None` if:
    /// - The weak reference was never set
    /// - The NeoSystem has been dropped (shutdown in progress)
    ///
    /// Callers should handle `None` gracefully, typically by aborting the
    /// current operation or returning early.
    pub fn neo_system(&self) -> Option<Arc<NeoSystem>> {
        let result = self.system.read().as_ref().and_then(|weak| weak.upgrade());

        if result.is_none() {
            // Log at trace level to avoid spam during normal shutdown
            trace!(
                target: "neo",
                "NeoSystem weak reference upgrade failed - system may be shutting down or not yet initialized"
            );
        }

        result
    }

    /// Returns `true` if the NeoSystem is still alive and accessible.
    ///
    /// This is a lightweight check that doesn't acquire a strong reference.
    pub fn is_system_alive(&self) -> bool {
        self.system
            .read()
            .as_ref()
            .map(|weak| weak.strong_count() > 0)
            .unwrap_or(false)
    }

    pub fn broadcast_plugin_event(&self, event: PluginEvent) {
        broadcast_plugin_event(&event);
    }

    pub fn register_committing_handler(
        &self,
        handler: Arc<dyn ICommittingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.committing_handlers.write().push(handler);
        Ok(())
    }

    pub fn register_committed_handler(
        &self,
        handler: Arc<dyn ICommittedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.committed_handlers.write().push(handler);
        Ok(())
    }

    pub fn register_transaction_added_handler(
        &self,
        handler: Arc<dyn ITransactionAddedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.transaction_added_handlers.write().push(handler);
        Ok(())
    }

    pub fn register_transaction_removed_handler(
        &self,
        handler: Arc<dyn ITransactionRemovedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.transaction_removed_handlers.write().push(handler);
        Ok(())
    }

    pub fn register_log_handler(
        &self,
        handler: Arc<dyn ILogHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.log_handlers.write().push(handler);
        Ok(())
    }

    pub fn register_logging_handler(
        &self,
        handler: Arc<dyn ILoggingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.logging_handlers.write().push(handler);
        Ok(())
    }

    pub fn register_notify_handler(
        &self,
        handler: Arc<dyn INotifyHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.notify_handlers.write().push(handler);
        Ok(())
    }

    pub fn register_wallet_changed_handler(
        &self,
        handler: Arc<dyn IWalletChangedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let handler_clone = handler.clone();
        self.wallet_changed_handlers.write().push(handler);
        let current = self.current_wallet.read().clone();
        handler_clone.i_wallet_provider_wallet_changed_handler(self, current);
        Ok(())
    }

    pub fn notify_application_log(&self, engine: &ApplicationEngine, args: &LogEventArgs) {
        let handlers = { self.log_handlers.read().clone() };
        for handler in handlers {
            handler.application_engine_log_handler(engine, args);
        }
    }

    pub fn notify_logging_handlers(&self, source: &str, level: LogLevel, message: &str) {
        let handlers = { self.logging_handlers.read().clone() };
        for handler in handlers {
            handler.utility_logging_handler(source, level, message);
        }
    }

    pub fn notify_application_notify(&self, engine: &ApplicationEngine, args: &NotifyEventArgs) {
        let handlers = { self.notify_handlers.read().clone() };
        for handler in handlers {
            handler.application_engine_notify_handler(engine, args);
        }
    }

    pub fn notify_wallet_changed(&self, sender: &dyn Any, wallet: Option<Arc<dyn Wallet>>) {
        *self.current_wallet.write() = wallet.clone();
        let handlers = { self.wallet_changed_handlers.read().clone() };
        for handler in handlers {
            handler.i_wallet_provider_wallet_changed_handler(sender, wallet.clone());
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

    pub fn transaction_added_handlers(
        &self,
    ) -> Arc<RwLock<Vec<Arc<dyn ITransactionAddedHandler + Send + Sync>>>> {
        Arc::clone(&self.transaction_added_handlers)
    }

    pub fn transaction_removed_handlers(
        &self,
    ) -> Arc<RwLock<Vec<Arc<dyn ITransactionRemovedHandler + Send + Sync>>>> {
        Arc::clone(&self.transaction_removed_handlers)
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
        self.service_registry.get_service::<T>()
    }

    /// Returns `true` if a named service is registered.
    pub fn has_named_service(&self, name: &str) -> bool {
        self.service_registry.has_named_service(name)
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

        let start = height.saturating_sub(LEDGER_HYDRATION_WINDOW.saturating_sub(1));
        if start > 0 {
            trace!(
                target: "neo",
                tip = height,
                window = LEDGER_HYDRATION_WINDOW,
                start,
                "bounded ledger hydration"
            );
        }
        for index in start..=height {
            match ledger_contract.get_block(store_cache, HashOrIndex::Index(index)) {
                Ok(Some(block)) => {
                    let payload_block = convert_ledger_block(block);
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
            .map(convert_ledger_block)
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
    /// Returns the `Transaction` by cloning from the internal `Arc<Transaction>`.
    pub fn try_get_transaction_from_mempool(&self, hash: &UInt256) -> Option<Transaction> {
        self.memory_pool
            .lock()
            .try_get(hash)
            .map(|arc| (*arc).clone())
    }

    /// Determines whether a transaction exists in the mempool or persisted store.
    pub fn contains_transaction(&self, hash: &UInt256) -> ContainsTransactionType {
        if self.memory_pool.lock().contains_key(hash) {
            return ContainsTransactionType::ExistsInPool;
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
                Ok(Some(block)) => headers.push(convert_ledger_header(block.header)),
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

impl SystemContext for NeoSystemContext {
    fn store_cache(&self) -> StoreCache {
        NeoSystemContext::store_cache(self)
    }

    fn protocol_settings(&self) -> Arc<ProtocolSettings> {
        NeoSystemContext::protocol_settings(self)
    }

    fn current_block_index(&self) -> u32 {
        self.ledger.current_height()
    }

    fn block_hash_at(&self, index: u32) -> Option<UInt256> {
        NeoSystemContext::block_hash_at(self, index)
    }

    fn mempool_count(&self) -> usize {
        self.memory_pool.lock().count()
    }

    fn mempool_contains(&self, hash: &UInt256) -> bool {
        self.memory_pool.lock().contains_key(hash)
    }

    fn header_height(&self) -> u32 {
        self.ledger.highest_header_index()
    }

    fn is_ready(&self) -> bool {
        self.is_ready(Some(20))
    }

    fn notify_application_log(&self, engine: &ApplicationEngine, args: &LogEventArgs) {
        NeoSystemContext::notify_application_log(self, engine, args);
    }

    fn notify_application_notify(&self, engine: &ApplicationEngine, args: &NotifyEventArgs) {
        NeoSystemContext::notify_application_notify(self, engine, args);
    }
}
