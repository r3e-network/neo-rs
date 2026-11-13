//! Shared runtime context for `NeoSystem`.
//!
//! This module exposes handles to core actors, services, and caches that are
//! shared across the node. It encapsulates read-only accessors, event
//! registration, and data retrieval helpers while keeping the heavy orchestration
//! logic in `core.rs`.

use parking_lot::{Mutex, RwLock};
use std::any::Any;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::thread;

use neo_actors::{ActorSystemHandle, EventStreamHandle};
use tokio::sync::mpsc;
use tracing::trace;

use super::actors::TransactionRouterHandle;
use super::converters::{convert_execution_block, convert_execution_header};
use super::registry::ServiceRegistry;
use super::relay::{RelayExtensibleCache, RelayExtensibleEntry};
use super::system::ReadinessStatus;
use crate::blockchain::BlockchainHandle;
use crate::events::{PluginEvent, broadcast_plugin_event};
use crate::i_event_handlers::{CommittedHandler, CommittingHandler, WalletChangedHandler};
use crate::mempool::MemoryPool;
use crate::network::p2p::{
    LocalNode, LocalNodeHandle, TaskManagerHandle,
    payloads::{extensible_payload::ExtensiblePayload, header::Header, transaction::Transaction},
};
use crate::services::StateStoreService as StateStore;
use crate::services::SystemContext;
use crate::services::{MempoolService, PeerManagerService, StateStoreService};
use neo_ledger::{HeaderCache, LedgerContext};
use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
use neo_primitives::LogEventArgs;
use neo_primitives::LogLevel;
use neo_primitives::contains_transaction_type::ContainsTransactionType;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_storage::persistence::{StoreCache, store::Store, store_provider::StoreProvider};
use neo_vm::NotifyEventArgs;
use neo_vm::VmState;
use neo_wallets::{Wallet, WalletProvider};

use super::core::NeoSystem;

/// Schema version of the canonical transaction-execution observation contract.
pub const TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION: u16 = 1;

/// Version of the fixed canonical transaction-execution classification rule.
pub const TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION: &str =
    "canonical-transaction-execution-v1";

/// Canonical execution stage that produced a read-only observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionExecutionStage {
    /// Transaction application execution during block persistence.
    Application,
}

impl TransactionExecutionStage {
    /// Stable evidence representation for this stage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
        }
    }
}

/// Coarse, non-authoritative category for a canonical execution anomaly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionExecutionExceptionCategory {
    /// The VM entered FAULT without exposing a caught exception payload.
    VmFault,
    /// The VM safely converted an execution exception into FAULT.
    CaughtVmException,
}

impl TransactionExecutionExceptionCategory {
    /// Stable evidence representation for this category.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VmFault => "vm_fault",
            Self::CaughtVmException => "caught_vm_exception",
        }
    }
}

/// Read-only evidence emitted only after the containing block commits successfully.
///
/// This record is diagnostic input only. It cannot change transaction execution,
/// state, block validity, persistence, or replay results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionExecutionObservation {
    /// Hash of the transaction whose canonical execution faulted.
    pub transaction_hash: UInt256,
    /// Height of the successfully committed containing block.
    pub block_height: u32,
    /// Hash of the successfully committed containing block.
    pub block_hash: UInt256,
    /// Canonical pipeline stage that produced the result.
    pub execution_stage: TransactionExecutionStage,
    /// Final VM state observed by canonical persistence.
    pub vm_state: VmState,
    /// Coarse category derived from the already-captured VM result.
    pub exception_category: TransactionExecutionExceptionCategory,
    /// GAS consumed by the canonical execution.
    pub gas_consumed: i64,
    /// Version of the observation payload schema.
    pub schema_version: u16,
    /// Version of the fixed classification rule.
    pub rule_version: &'static str,
    /// Diagnostic capture time; never used by canonical logic.
    pub observed_at_unix_ms: u64,
}

/// Bounded sender used by canonical persistence as a non-blocking diagnostic sink.
pub type TransactionExecutionSink = mpsc::Sender<TransactionExecutionObservation>;

/// Lightweight handle exposing shared system facilities to actors outside the core module.
pub struct NeoSystemContext {
    /// Handle to the actor runtime for scheduling and event stream access.
    pub actor_system: ActorSystemHandle,
    /// Reference to the blockchain actor hierarchy root.
    pub blockchain: BlockchainHandle,
    /// Reference to the local node actor (peer supervisor).
    pub local_node: LocalNodeHandle,
    /// Reference to the task manager actor coordinating inventory download.
    pub task_manager: TaskManagerHandle,
    /// Handle to the transaction router worker.
    pub tx_router: TransactionRouterHandle,
    /// Global service registry mirrored from the C# implementation.
    pub(crate) service_registry: Arc<ServiceRegistry>,
    /// Registered callbacks for wallet changes.
    pub wallet_changed_handlers: Arc<RwLock<Vec<Arc<dyn WalletChangedHandler + Send + Sync>>>>,
    /// Currently active wallet, if any.
    pub current_wallet: Arc<RwLock<Option<Arc<dyn Wallet>>>>,
    /// Store provider used to instantiate persistence backends.
    pub store_provider: Arc<dyn StoreProvider>,
    /// Active persistence store.
    pub store: Arc<dyn Store>,
    /// Cached genesis block shared with the blockchain actor.
    pub(crate) genesis_block: Arc<neo_protocol::execution::Block>,
    pub(crate) ledger: Arc<LedgerContext>,
    pub(crate) state_service_enabled: bool,
    pub(crate) state_store: Option<Arc<dyn StateStore>>,
    pub(crate) memory_pool: Arc<Mutex<MemoryPool>>,
    pub(crate) header_cache: Arc<HeaderCache>,
    pub(crate) local_node_state: Arc<LocalNode>,
    pub(crate) settings: Arc<ProtocolSettings>,
    pub(crate) relay_cache: Arc<RelayExtensibleCache>,
    pub(crate) system: RwLock<Option<Weak<NeoSystem>>>,
    pub(crate) committing_handlers: Arc<RwLock<Vec<Arc<dyn CommittingHandler + Send + Sync>>>>,
    pub(crate) committed_handlers: Arc<RwLock<Vec<Arc<dyn CommittedHandler + Send + Sync>>>>,
    pub(crate) transaction_execution_sink: OnceLock<TransactionExecutionSink>,
    pub(crate) dropped_transaction_execution_observations: AtomicU64,
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

    /// Opens a typed read/write cache directly over the store's latest state.
    pub fn store_cache(&self) -> StoreCache {
        StoreCache::new_from_store(self.store.clone(), false)
    }

    /// Opens a typed read/write cache over a fresh store snapshot, isolating
    /// reads from concurrent writes until the snapshot is committed.
    pub fn store_snapshot_cache(&self) -> StoreCache {
        let snapshot = self.store.snapshot();
        StoreCache::new_from_snapshot(snapshot)
    }

    /// Returns the canonical genesis block.
    pub fn genesis_block(&self) -> Arc<neo_protocol::execution::Block> {
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
    pub fn state_store(&self) -> CoreResult<Option<Arc<dyn StateStore>>> {
        if !self.state_service_enabled {
            return Ok(None);
        }
        Ok(self.state_store.clone())
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

    /// Typed ledger view - returns the shared `LedgerContext` when present.
    pub fn ledger_typed(&self) -> CoreResult<Option<Arc<LedgerContext>>> {
        self.ledger_service()
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
            Arc::new(crate::mempool::services::MutexMempoolService::new(svc))
                as Arc<dyn MempoolService + Send + Sync>
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
        self.neo_system().map_or(
            ReadinessStatus {
                block_height: 0,
                header_height: 0,
                header_lag: u32::MAX,
                healthy: false,
                rpc_ready: false,
                storage_ready: false,
            },
            |sys| sys.readiness(max_header_lag),
        )
    }

    /// Snapshot of readiness annotated with optional service checks (by name) and storage state.
    pub fn readiness_with_services(
        &self,
        max_header_lag: Option<u32>,
        rpc_service_name: Option<&str>,
        storage_ready: Option<bool>,
    ) -> ReadinessStatus {
        let status = self.readiness(max_header_lag);
        let rpc_ready = rpc_service_name.is_none_or(|name| self.has_named_service(name));
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
        let result = self
            .system
            .read()
            .as_ref()
            .and_then(std::sync::Weak::upgrade);

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
            .is_some_and(|weak| weak.strong_count() > 0)
    }

    /// Broadcasts a plugin lifecycle event to all registered listeners.
    pub fn broadcast_plugin_event(&self, event: PluginEvent) {
        broadcast_plugin_event(&event);
    }

    /// Registers a handler invoked before each block's state is committed.
    pub fn register_committing_handler(
        &self,
        handler: Arc<dyn CommittingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.committing_handlers.write().push(handler);
        Ok(())
    }

    /// Registers a handler invoked after each block has been persisted.
    pub fn register_committed_handler(
        &self,
        handler: Arc<dyn CommittedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.committed_handlers.write().push(handler);
        Ok(())
    }

    /// Registers the single bounded sink used for canonical execution observations.
    ///
    /// Registration is one-time. Persistence only performs `try_send`; a full or
    /// closed queue is ignored and can never affect the canonical result.
    pub fn register_transaction_execution_sink(
        &self,
        sink: TransactionExecutionSink,
    ) -> Result<(), TransactionExecutionSink> {
        self.transaction_execution_sink.set(sink)
    }

    /// Returns a lock-free snapshot of capacity currently available to canonical persistence.
    ///
    /// A missing, full, or closed sink reports zero so persistence does not allocate
    /// per-transaction staging for evidence that cannot currently be accepted.
    pub(crate) fn transaction_execution_sink_available_capacity(&self) -> usize {
        self.transaction_execution_sink
            .get()
            .filter(|sink| !sink.is_closed())
            .map_or(0, tokio::sync::mpsc::Sender::capacity)
    }

    /// Attempts to enqueue diagnostic evidence without locks, waits, or callbacks.
    pub(crate) fn try_emit_transaction_execution_observation(
        &self,
        observation: TransactionExecutionObservation,
    ) -> bool {
        self.transaction_execution_sink
            .get()
            .is_some_and(|sink| sink.try_send(observation).is_ok())
    }

    /// Returns the number of post-commit observations dropped by the bounded bridge.
    pub fn dropped_transaction_execution_observation_count(&self) -> u64 {
        self.dropped_transaction_execution_observations
            .load(Ordering::Relaxed)
    }

    pub(crate) fn record_dropped_transaction_execution_observations(&self, dropped: usize) {
        let dropped = u64::try_from(dropped).unwrap_or(u64::MAX);
        self.dropped_transaction_execution_observations
            .fetch_add(dropped, Ordering::Relaxed);
    }

    /// Registers a wallet-changed handler and immediately replays the
    /// current wallet to it.
    pub fn register_wallet_changed_handler(
        &self,
        handler: Arc<dyn WalletChangedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let handler_clone = handler.clone();
        self.wallet_changed_handlers.write().push(handler);
        let current = self.current_wallet.read().clone();
        handler_clone.wallet_provider_wallet_changed_handler(self, current);
        Ok(())
    }

    /// Application-log notification; intentionally a no-op on this context.
    pub fn notify_application_log(&self, _engine: &ApplicationEngine, _args: &LogEventArgs) {}

    /// Logging notification; intentionally a no-op on this context.
    pub fn notify_logging_handlers(&self, _source: &str, _level: LogLevel, _message: &str) {}

    /// Notify-event notification; intentionally a no-op on this context.
    pub fn notify_application_notify(&self, _engine: &ApplicationEngine, _args: &NotifyEventArgs) {}

    /// Updates the active wallet, replays it to registered handlers, and
    /// broadcasts a `WalletChanged` plugin event.
    pub fn notify_wallet_changed(&self, sender: &dyn Any, wallet: Option<Arc<dyn Wallet>>) {
        (*self.current_wallet.write()).clone_from(&wallet);
        let handlers = { self.wallet_changed_handlers.read().clone() };
        for handler in handlers {
            handler.wallet_provider_wallet_changed_handler(sender, wallet.clone());
        }
        let wallet_name = wallet
            .as_ref()
            .map(|w| w.name().to_string())
            .unwrap_or_default();
        self.broadcast_plugin_event(PluginEvent::WalletChanged { wallet_name });
    }

    /// Attaches a wallet provider: spawns a listener thread that forwards
    /// wallet changes into this context for the provider's lifetime.
    pub fn attach_wallet_provider(
        context: &Arc<Self>,
        provider: Arc<dyn WalletProvider + Send + Sync>,
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

    /// Returns the shared registry of committing handlers.
    pub fn committing_handlers(
        &self,
    ) -> Arc<RwLock<Vec<Arc<dyn CommittingHandler + Send + Sync>>>> {
        Arc::clone(&self.committing_handlers)
    }

    /// Returns the shared registry of committed handlers.
    pub fn committed_handlers(&self) -> Arc<RwLock<Vec<Arc<dyn CommittedHandler + Send + Sync>>>> {
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
        self.service_registry.get_service::<T>()
    }

    /// Returns `true` if a named service is registered.
    pub fn has_named_service(&self, name: &str) -> bool {
        self.service_registry.has_named_service(name)
    }

    #[cfg(test)]
    pub(crate) fn hydrate_ledger_from_store(
        store_cache: &StoreCache,
        ledger: &Arc<LedgerContext>,
        header_cache: &Arc<HeaderCache>,
    ) -> Result<(), super::storage::StorageError> {
        super::storage::hydrate_ledger(store_cache, ledger, header_cache)
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
    pub fn try_get_block(&self, hash: &UInt256) -> Option<neo_ledger::Block> {
        if let Some(block) = self.ledger.get_block(hash) {
            return Some(block);
        }

        let ledger_contract = LedgerContract::new();
        let store_cache = self.store_cache();
        ledger_contract
            .get_block(&store_cache, HashOrIndex::Hash(*hash))
            .ok()
            .flatten()
            .map(convert_execution_block)
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
        match ledger_contract.contains_transaction(&store_cache, hash) {
            Ok(true) => return ContainsTransactionType::ExistsInLedger,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "ledger contains_transaction failed; treating as ExistsInLedger (fail-closed)"
                );
                return ContainsTransactionType::ExistsInLedger;
            }
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
            .unwrap_or_else(|error| {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "ledger contains_conflict_hash failed; treating as conflict (fail-closed)"
                );
                true
            })
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
                Ok(Some(block)) => headers.push(convert_execution_header(block.header)),
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
    pub fn record_transaction(
        &self,
        transaction: Transaction,
    ) -> neo_smart_contract::error::CoreResult<UInt256> {
        self.ledger.insert_transaction(transaction).map_err(|e| {
            neo_smart_contract::error::CoreError::invalid_operation(format!(
                "Failed to insert transaction: {e}"
            ))
        })
    }

    /// Removes a transaction from the mempool if present.
    pub fn remove_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.ledger.remove_transaction(hash)
    }

    /// Registers a block with the in-memory ledger cache and returns its hash.
    pub fn record_block(
        &self,
        block: neo_ledger::Block,
    ) -> neo_smart_contract::error::CoreResult<UInt256> {
        self.ledger.insert_block(block).map_err(|e| {
            neo_smart_contract::error::CoreError::invalid_operation(format!(
                "Failed to insert block: {e}"
            ))
        })
    }

    /// Registers an extensible payload with the in-memory ledger cache.
    pub fn record_extensible(
        &self,
        payload: ExtensiblePayload,
    ) -> neo_smart_contract::error::CoreResult<UInt256> {
        let entry = RelayExtensibleEntry::try_new(payload.clone())?;
        let hash = entry.hash();
        self.relay_cache.add(entry);
        self.ledger.insert_extensible(payload)?;
        Ok(hash)
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
