use std::any::Any;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, RwLock,
};
use std::time::Duration;

use akka::{
    Actor, ActorContext, ActorRef, ActorResult, ActorSystem, ActorSystemHandle, EventStreamHandle,
    Props,
};
use async_trait::async_trait;
use tokio::sync::oneshot;
use tracing::{debug, warn};

use crate::error::{CoreError, CoreResult};
use crate::ledger::blockchain::{Blockchain, BlockchainCommand};
use crate::network::p2p::{
    payloads::{
        block::Block, extensible_payload::ExtensiblePayload, header::Header,
        transaction::Transaction,
    },
    ChannelsConfig, LocalNode, LocalNodeCommand, PeerCommand, RemoteNodeSnapshot, TaskManager,
    TaskManagerCommand,
};
use crate::persistence::{i_store::IStore, i_store_provider::IStoreProvider, StoreFactory};
use crate::uint256::UInt256;

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

/// Global protocol settings (matches C# `Neo.ProtocolSettings`).
#[derive(Debug, Clone)]
pub struct ProtocolSettings {
    pub network: u32,
    pub address_version: u8,
    pub standby_committee: Vec<neo_cryptography::ECPoint>,
    pub validators_count: u32,
    pub seed_list: Vec<String>,
    pub milliseconds_per_block: u32,
    pub max_valid_until_block_increment: u32,
    pub max_transactions_per_block: u32,
    pub memory_pool_max_transactions: i32,
    pub max_traceable_blocks: u32,
    pub initial_gas_distribution: u64,
    pub hardforks: HashMap<crate::hardfork::Hardfork, u32>,
}

impl ProtocolSettings {
    /// Constructs the protocol settings using Neo's production defaults (equivalent to `ProtocolSettings.Default`).
    pub fn mainnet() -> Self {
        Self {
            network: 0x334F454Eu32,
            address_version: 53,
            standby_committee: Vec::new(),
            validators_count: 7,
            seed_list: vec!["seed1.neo.org".into(), "seed2.neo.org".into()],
            milliseconds_per_block: crate::constants::MILLISECONDS_PER_BLOCK,
            max_valid_until_block_increment: 5760,
            max_transactions_per_block: crate::constants::MAX_TRANSACTIONS_PER_BLOCK,
            memory_pool_max_transactions: crate::constants::MEMORY_POOL_MAX_TRANSACTIONS,
            max_traceable_blocks: crate::constants::MAX_TRACEABLE_BLOCKS,
            initial_gas_distribution: 52_000_000_000_000,
            hardforks: HashMap::new(),
        }
    }

    /// Returns the time between blocks expressed as a [`Duration`].
    pub fn time_per_block(&self) -> Duration {
        Duration::from_millis(self.milliseconds_per_block as u64)
    }
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self::mainnet()
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
    services: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
    plugin_manager: Arc<RwLock<PluginManager>>,
    store_provider: Arc<dyn IStoreProvider>,
    store: Arc<dyn IStore>,
    ledger: Arc<LedgerContext>,
    context: Arc<NeoSystemContext>,
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
    pub services: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>>,
    /// Plugin manager shared state for hot-pluggable extensions.
    pub plugin_manager: Arc<RwLock<PluginManager>>,
    /// Store provider used to instantiate persistence backends.
    pub store_provider: Arc<dyn IStoreProvider>,
    /// Active persistence store.
    pub store: Arc<dyn IStore>,
    ledger: Arc<LedgerContext>,
}

impl NeoSystemContext {
    /// Returns the current best block index known to the system. This will be wired
    /// to the ledger subsystem once the persistence layer is fully ported.
    pub fn current_block_index(&self) -> u32 {
        self.ledger.current_height()
    }

    /// Attempts to retrieve a transaction from the local mempool or persisted store.
    pub fn try_get_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.ledger.get_transaction(hash)
    }

    /// Attempts to retrieve a block by its hash from the local store.
    pub fn try_get_block(&self, hash: &UInt256) -> Option<Block> {
        self.ledger.get_block(hash)
    }

    /// Attempts to retrieve an extensible payload by hash.
    pub fn try_get_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload> {
        self.ledger.get_extensible(hash)
    }

    /// Returns block hashes starting at the provided hash, limited by `count`.
    pub fn block_hashes_from(&self, hash_start: &UInt256, count: usize) -> Vec<UInt256> {
        self.ledger.block_hashes_from(hash_start, count)
    }

    /// Returns headers starting from the supplied index.
    pub fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header> {
        self.ledger.headers_from_index(index_start, count)
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

#[derive(Default)]
pub struct LedgerContext {
    best_height: AtomicU32,
    hashes_by_index: RwLock<Vec<UInt256>>,
    headers_by_index: RwLock<Vec<Option<Header>>>,
    blocks_by_hash: RwLock<HashMap<UInt256, Block>>,
    extensibles_by_hash: RwLock<HashMap<UInt256, ExtensiblePayload>>,
    transactions_by_hash: RwLock<HashMap<UInt256, Transaction>>,
}

impl LedgerContext {
    pub fn current_height(&self) -> u32 {
        self.best_height.load(Ordering::Relaxed)
    }

    pub fn insert_transaction(&self, transaction: Transaction) -> UInt256 {
        let hash = transaction.hash();
        self.transactions_by_hash
            .write()
            .unwrap()
            .insert(hash, transaction);
        hash
    }

    pub fn remove_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.transactions_by_hash.write().unwrap().remove(hash)
    }

    pub fn get_transaction(&self, hash: &UInt256) -> Option<Transaction> {
        self.transactions_by_hash.read().unwrap().get(hash).cloned()
    }

    pub fn insert_block(&self, mut block: Block) -> UInt256 {
        let header = block.header.clone();
        let index = header.index() as usize;
        let hash = block.hash();

        self.blocks_by_hash.write().unwrap().insert(hash, block);

        {
            let mut hashes = self.hashes_by_index.write().unwrap();
            if hashes.len() <= index {
                hashes.resize(index + 1, UInt256::zero());
            }
            hashes[index] = hash;
        }

        {
            let mut headers = self.headers_by_index.write().unwrap();
            if headers.len() <= index {
                headers.resize(index + 1, None);
            }
            headers[index] = Some(header);
        }

        self.best_height.fetch_max(index as u32, Ordering::Relaxed);
        hash
    }

    pub fn get_block(&self, hash: &UInt256) -> Option<Block> {
        self.blocks_by_hash.read().unwrap().get(hash).cloned()
    }

    pub fn insert_extensible(&self, mut payload: ExtensiblePayload) -> UInt256 {
        let hash = payload.hash();
        self.extensibles_by_hash
            .write()
            .unwrap()
            .insert(hash, payload);
        hash
    }

    pub fn get_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload> {
        self.extensibles_by_hash.read().unwrap().get(hash).cloned()
    }

    pub fn block_hashes_from(&self, hash_start: &UInt256, count: usize) -> Vec<UInt256> {
        if count == 0 {
            return Vec::new();
        }

        let hashes = self.hashes_by_index.read().unwrap();
        let Some(start_pos) = hashes.iter().position(|hash| hash == hash_start) else {
            return Vec::new();
        };

        hashes
            .iter()
            .skip(start_pos + 1)
            .filter(|hash| **hash != UInt256::zero())
            .take(count)
            .cloned()
            .collect()
    }

    pub fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header> {
        if count == 0 {
            return Vec::new();
        }

        let headers = self.headers_by_index.read().unwrap();
        let mut collected = Vec::new();
        let mut index = index_start as usize;

        while index < headers.len() && collected.len() < count {
            match &headers[index] {
                Some(header) => collected.push(header.clone()),
                None => break,
            }
            index += 1;
        }

        collected
    }

    pub fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        self.transactions_by_hash
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
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
    ) -> CoreResult<Self> {
        let actor_system = ActorSystem::new("neo").map_err(to_core_error)?;
        let settings_arc = Arc::new(settings.clone());

        let services: Arc<RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>> =
            Arc::new(RwLock::new(HashMap::new()));
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

        {
            let mut guard = services
                .write()
                .map_err(|_| CoreError::system("service registry poisoned"))?;
            guard.insert(
                "LocalNode".to_string(),
                local_node_state.clone() as Arc<dyn Any + Send + Sync>,
            );
            guard.insert(
                "Store".to_string(),
                store.clone() as Arc<dyn Any + Send + Sync>,
            );
            guard.insert(
                "StoreProvider".to_string(),
                store_provider.clone() as Arc<dyn Any + Send + Sync>,
            );
            guard.insert(
                "Ledger".to_string(),
                ledger.clone() as Arc<dyn Any + Send + Sync>,
            );
        }

        let blockchain = actor_system
            .actor_of(Blockchain::props(Arc::new(())), "blockchain")
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

        blockchain
            .tell(BlockchainCommand::Initialize)
            .map_err(to_core_error)?;

        let context = Arc::new(NeoSystemContext {
            actor_system: actor_system.handle(),
            blockchain: blockchain.clone(),
            local_node: local_node.clone(),
            task_manager: task_manager.clone(),
            tx_router: tx_router.clone(),
            services: services.clone(),
            plugin_manager: plugin_manager.clone(),
            store_provider: store_provider.clone(),
            store: store.clone(),
            ledger: ledger.clone(),
        });
        local_node_state.set_system_context(context.clone());
        task_manager
            .tell(TaskManagerCommand::AttachSystem {
                context: context.clone(),
            })
            .map_err(to_core_error)?;

        Ok(Self {
            settings,
            actor_system,
            blockchain,
            local_node,
            task_manager,
            tx_router,
            services,
            plugin_manager,
            store_provider,
            store,
            ledger,
            context,
        })
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
    pub fn add_service<T>(&self, name: impl Into<String>, service: T) -> CoreResult<()>
    where
        T: Send + Sync + 'static,
    {
        let mut guard = self
            .services
            .write()
            .map_err(|_| CoreError::system("service registry poisoned"))?;
        guard.insert(name.into(), Arc::new(service));
        Ok(())
    }

    /// Retrieves a previously registered service by name.
    pub fn get_service<T>(&self, name: &str) -> CoreResult<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        let guard = self
            .services
            .read()
            .map_err(|_| CoreError::system("service registry poisoned"))?;
        Ok(guard
            .get(name)
            .and_then(|service| service.clone().downcast::<T>().ok()))
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
    pub async fn shutdown(self) -> CoreResult<()> {
        self.actor_system.shutdown().await.map_err(to_core_error)
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
                warn!(target: "neo", message_type = %payload.type_id().name(), "unknown message routed to transaction router actor");
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

    #[tokio::test]
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
