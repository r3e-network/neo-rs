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
use crate::error::{CoreError, CoreResult};
use crate::i_event_handlers::IServiceAddedHandler;
use crate::ledger::blockchain::{Blockchain, BlockchainCommand};
use crate::ledger::{
    block::Block as LedgerBlock, block_header::BlockHeader as LedgerBlockHeader,
    blockchain_application_executed::ApplicationExecuted, LedgerContext, MemoryPool,
};
use crate::neo_io::{BinaryWriter, Serializable};
use crate::network::p2p::{
    payloads::{
        block::Block, extensible_payload::ExtensiblePayload, header::Header,
        transaction::Transaction, witness::Witness as PayloadWitness,
    },
    ChannelsConfig, LocalNode, LocalNodeCommand, PeerCommand, RemoteNodeSnapshot, TaskManager,
    TaskManagerCommand,
};
use crate::persistence::{
    data_cache::DataCache, i_store::IStore, i_store_provider::IStoreProvider,
    track_state::TrackState, StoreCache, StoreFactory,
};
pub use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::application_engine::TEST_MODE_GAS;
use crate::smart_contract::native::helpers::NativeHelpers;
use crate::smart_contract::native::ledger_contract::{
    HashOrIndex, LedgerContract, LedgerTransactionStates, PersistedTransactionState,
};
use crate::smart_contract::trigger_type::TriggerType;
use crate::uint160::UInt160;
use crate::uint256::UInt256;
use neo_extensions::error::ExtensionError;
use neo_extensions::plugin::{
    broadcast_global_event, initialise_global_runtime, shutdown_global_runtime, PluginContext,
    PluginEvent,
};
use neo_vm::OpCode;

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

/// Lightweight handle exposing shared system facilities to actors outside the core module.
#[derive(Clone)]
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
    /// Store provider used to instantiate persistence backends.
    pub store_provider: Arc<dyn IStoreProvider>,
    /// Active persistence store.
    pub store: Arc<dyn IStore>,
    /// Cached genesis block shared with the blockchain actor.
    genesis_block: Arc<Block>,
    ledger: Arc<LedgerContext>,
    memory_pool: Arc<Mutex<MemoryPool>>,
    settings: Arc<ProtocolSettings>,
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
            .finish()
    }
}

impl NeoSystemContext {
    pub fn store_cache(&self) -> StoreCache {
        StoreCache::new_from_store(self.store.clone(), true)
    }

    /// Returns the canonical genesis block.
    pub fn genesis_block(&self) -> Arc<Block> {
        self.genesis_block.clone()
    }

    /// Shared access to the ledger context.
    pub fn ledger(&self) -> Arc<LedgerContext> {
        self.ledger.clone()
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

    fn convert_ledger_block(block: LedgerBlock) -> Block {
        Block {
            header: Self::convert_ledger_header(block.header),
            transactions: block.transactions,
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
            .get_block(&store_cache, HashOrIndex::Hash(hash.clone()))
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
            ledger_contract.get_block(&store_cache, HashOrIndex::Hash(hash_start.clone()))
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
        self.ledger.insert_extensible(payload)
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
        let mut store_cache = self.context.store_cache();
        let snapshot = Arc::new(DataCache::new(false));

        let mut on_persist_engine = ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            Arc::clone(&snapshot),
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
            let container: Arc<dyn crate::IVerifiable> = Arc::new(tx.clone());
            let mut tx_engine = ApplicationEngine::new(
                TriggerType::Verification,
                Some(container),
                Arc::clone(&snapshot),
                Some(ledger_block.clone()),
                self.settings.clone(),
                TEST_MODE_GAS,
                None,
            )?;

            tx_engine.set_state(tx_states);

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
        }

        on_persist_engine.set_state(tx_states);
        on_persist_engine.native_post_persist()?;
        let post_persist_exec = ApplicationExecuted::new(&mut on_persist_engine);
        self.actor_system
            .event_stream()
            .publish(post_persist_exec.clone());
        executed.push(post_persist_exec);

        for (key, trackable) in snapshot.tracked_items() {
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
        self.context.record_block(block);

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

        let store_provider = storage_provider.unwrap_or_else(|| {
            StoreFactory::get_store_provider("Memory")
                .expect("default memory store provider must be registered")
        });
        let store = store_provider.get_store(storage_path.as_deref().unwrap_or(""));

        let user_agent = format!("/neo-rs:{}/", env!("CARGO_PKG_VERSION"));
        let local_node_state = Arc::new(LocalNode::new(settings_arc.clone(), 10333, user_agent));
        local_node_state.set_seed_list(settings.seed_list.clone());
        let ledger = Arc::new(LedgerContext::default());
        let memory_pool = Arc::new(Mutex::new(MemoryPool::new(&settings)));

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
            .actor_of(Props::new(TransactionRouterActor::default), "tx_router")
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
            store_provider: store_provider.clone(),
            store: store.clone(),
            genesis_block: genesis_block.clone(),
            ledger: ledger.clone(),
            memory_pool: memory_pool.clone(),
            settings: settings_arc.clone(),
        });

        if let Ok(mut pool) = memory_pool.lock() {
            let local_node_ref = local_node.clone();
            let blockchain_ref = blockchain.clone();
            pool.transaction_relay = Some(Box::new(move |tx: &Transaction| {
                let mut writer = BinaryWriter::new();
                if let Err(error) = tx.serialize(&mut writer) {
                    debug!(
                        target: "neo",
                        %error,
                        "failed to serialize transaction for mempool rebroadcast"
                    );
                    return;
                }

                let payload = writer.to_bytes();
                if let Err(error) = local_node_ref.tell_from(
                    LocalNodeCommand::RelayDirectly { payload },
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

    /// Fetches the shared local node snapshot for advanced operations.
    pub async fn local_node_state(&self) -> CoreResult<Arc<LocalNode>> {
        self.ask_local_node(|reply| LocalNodeCommand::GetInstance { reply })
            .await
    }

    /// Records a relay broadcast via the local node actor.
    pub fn relay_directly(&self, payload: Vec<u8>) -> CoreResult<()> {
        self.local_node
            .tell(LocalNodeCommand::RelayDirectly { payload })
            .map_err(to_core_error)
    }

    /// Records a direct send broadcast via the local node actor.
    pub fn send_directly(&self, payload: Vec<u8>) -> CoreResult<()> {
        self.local_node
            .tell(LocalNodeCommand::SendDirectly { payload })
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

    /// Gracefully shuts down the actor hierarchy.
    pub async fn shutdown(self: Arc<Self>) -> CoreResult<()> {
        let event = PluginEvent::NodeStopping;
        if let Err(err) = broadcast_global_event(&event).await {
            warn!("failed to broadcast NodeStopping event: {}", err);
        }
        if let Err(err) = shutdown_global_runtime().await {
            warn!("failed to shutdown plugin runtime: {}", err);
        }
        match Arc::try_unwrap(self) {
            Ok(system) => system.actor_system.shutdown().await.map_err(to_core_error),
            Err(_) => Err(CoreError::system(
                "cannot shutdown NeoSystem while shared references remain",
            )),
        }
    }
}

fn to_core_error(err: akka::AkkaError) -> CoreError {
    CoreError::system(err.to_string())
}

// === Actor definitions ====================================================

#[derive(Default)]
struct TransactionRouterActor;

#[async_trait]
impl Actor for TransactionRouterActor {
    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        match envelope.downcast::<TransactionRouterMessage>() {
            Ok(message) => {
                debug!(target: "neo", ?message, "transaction router message received");
                Ok(())
            }
            Err(payload) => {
                warn!(
                    target: "neo",
                    message_type = ?payload.type_id(),
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
    RouteTransaction { hash: UInt256, payload: Vec<u8> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::witness::Witness as PayloadWitness;
    use crate::{UInt160, UInt256};

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
