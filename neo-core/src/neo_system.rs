use std::any::Any;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::{DateTime, Utc};
use NeoRust::builder::Transaction;
use tokio::sync::{mpsc, Mutex as TokioMutex};
use tokio::task::JoinHandle;
use crate::block::{Block, Header};
use crate::contract::Contract;
use crate::ledger::header_cache::HeaderCache;
use crate::ledger::memory_pool::MemoryPool;
use crate::network::ChannelsConfig;
use crate::network::payloads::Witness;
use crate::persistence::{MemoryStore, SnapshotCache, StoreProviderTrait};
use crate::protocol_settings::ProtocolSettings;
use crate::store::Store;
use crate::uint256::UInt256;

pub struct NeoSystem {
    pub settings: Arc<ProtocolSettings>,
    pub genesis_block: Block,
    pub blockchain: BlockchainHandle,
    pub local_node: LocalNodeHandle,
    pub task_manager: TaskManagerHandle,
    pub tx_router: TxRouterHandle,
    pub mem_pool: Arc<MemoryPool>,
    pub header_cache: Arc<HeaderCache>,
    pub relay_cache: Arc<TokioMutex<HashMap<UInt256, bool>>>,
    services: Arc<TokioMutex<Vec<Box<dyn Any + Send + Sync>>>>,
    store: Arc<dyn Store<WriteBatch=()>>,
    storage_provider: Arc<dyn StoreProviderTrait>,
    start_message: Arc<TokioMutex<Option<ChannelsConfig>>>,
    suspend: Arc<AtomicI32>,
}

struct BlockchainHandle {
    sender: mpsc::Sender<BlockchainMessage>,
    handle: JoinHandle<()>,
}

struct LocalNodeHandle {
    sender: mpsc::Sender<LocalNodeMessage>,
    handle: JoinHandle<()>,
}

struct TaskManagerHandle {
    sender: mpsc::Sender<TaskManagerMessage>,
    handle: JoinHandle<()>,
}

struct TxRouterHandle {
    sender: mpsc::Sender<TxRouterMessage>,
    handle: JoinHandle<()>,
}
enum BlockchainMessage {
    Initialize,
    AddBlock(Block),
    GetBlock(UInt256),
    GetHeight,
    ValidateTransaction(Transaction),
    Persist(Block),
    Shutdown,
}

enum LocalNodeMessage {
    Start(ChannelsConfig),
    ConnectToPeer(String),
    BroadcastTransaction(Transaction),
    RequestBlockByHash(UInt256),
    RequestBlockByIndex(u32),
    Shutdown,
}

enum TaskManagerMessage {
    ScheduleTask(Task),
    CancelTask(TaskId),
    GetTaskStatus(TaskId),
    Shutdown,
}

enum TxRouterMessage {
    RouteTransaction(Transaction),
    GetTransactionStatus(UInt256),
    Shutdown,
}

impl NeoSystem {
    pub async fn new(
        settings: Arc<ProtocolSettings>,
        storage_provider: Option<Arc<dyn StoreProviderTrait>>,
        storage_path: Option<String>,
    ) -> Self {
        let storage_provider = storage_provider.unwrap_or_else(|| Arc::new(MemoryStore::new()) as Arc<dyn StoreProviderTrait>);
        let store = storage_provider.get_store(storage_path);
        let genesis_block = Self::create_genesis_block(&settings);
        let mempool = Arc::new(MemoryPool::new());
        let header_cache = Arc::new(HeaderCache::new());

        let (blockchain_sender, blockchain_receiver) = mpsc::channel(100);
        let (local_node_sender, local_node_receiver) = mpsc::channel(100);
        let (task_manager_sender, task_manager_receiver) = mpsc::channel(100);
        let (tx_router_sender, tx_router_receiver) = mpsc::channel(100);

        let system = Arc::new(Self {
            settings: settings.clone(),
            genesis_block,
            blockchain: BlockchainHandle {
                sender: blockchain_sender,
                handle: tokio::spawn(blockchain_actor(blockchain_receiver, store.clone())),
            },
            local_node: LocalNodeHandle {
                sender: local_node_sender,
                handle: tokio::spawn(local_node_actor(local_node_receiver)),
            },
            task_manager: TaskManagerHandle {
                sender: task_manager_sender,
                handle: tokio::spawn(task_manager_actor(task_manager_receiver)),
            },
            tx_router: TxRouterHandle {
                sender: tx_router_sender,
                handle: tokio::spawn(tx_router_actor(tx_router_receiver)),
            },
            mem_pool: mempool,
            header_cache,
            relay_cache: Arc::new(TokioMutex::new(HashMap::new())),
            services: Arc::new(TokioMutex::new(Vec::new())),
            store,
            storage_provider,
            start_message: Arc::new(TokioMutex::new(None)),
            suspend: Arc::new(AtomicI32::new(0)),
        });

        for plugin in Plugin::plugins() {
            plugin.on_system_loaded(&system);
        }

        system.blockchain.sender.send(BlockchainMessage::Initialize).await.unwrap();

        system
    }

    fn create_genesis_block(settings: &ProtocolSettings) -> Block {
        let timestamp = DateTime::parse_from_rfc3339("2016-07-15T15:08:21Z")
            .unwrap()
            .timestamp_millis();

        Block {
            header: Header {
                hash: None,
                version: 0,
                prev_hash: UInt256::zero(),
                merkle_root: UInt256::zero(),
                timestamp,
                nonce: 2083236893,
                index: 0,
                primary_index: 0,
                next_consensus: Contract::get_bft_address(&settings.standby_validators()),
                witness: Witness {
                    invocation_script: Vec::new(),
                    verification_script: vec![OpCode::Push1 as u8],
                },
                primary: 0,
                witnesses: Default::default(),
            },
            transactions: Vec::new(),
        }
    }

    pub async fn add_service<T: 'static + Send + Sync>(&self, service: T) {
        let mut services = self.services.lock().await;
        services.push(Box::new(service));
        // Trigger ServiceAdded event here if needed
    }

    pub async fn get_service<T: 'static>(&self, filter: Option<impl Fn(&T) -> bool>) -> Option<Arc<T>> {
        let services = self.services.lock().await;
        services.iter()
            .filter_map(|s| s.downcast_ref::<T>())
            .find(|s| filter.as_ref().map_or(true, |f| f(s)))
            .map(|s| Arc::new(s.clone()))
    }

    pub async fn ensure_stopped(&self, handle: &JoinHandle<()>) {
        // Signal the actor to stop (you need to implement this mechanism)
        // Then wait for it to finish
        handle.abort();
        let _ = handle.await;
    }

    pub fn load_store(&self, path: &str) -> Arc<dyn Store> {
        self.storage_provider.get_store(Some(path.to_string()))
    }

    pub async fn resume_node_startup(&self) -> bool {
        if self.suspend.fetch_sub(1, Ordering::SeqCst) != 1 {
            return false;
        }
        let mut start_message = self.start_message.lock().await;
        if let Some(config) = start_message.take() {
            self.local_node.sender.send(LocalNodeMessage::Start(config)).await.unwrap();
        }
        true
    }

    pub async fn start_node(&self, config: ChannelsConfig) {
        let mut start_message = self.start_message.lock().await;
        *start_message = Some(config.clone());

        if self.suspend.load(Ordering::SeqCst) == 0 {
            if let Some(config) = start_message.take() {
                self.local_node.sender.send(LocalNodeMessage::Start(config)).await.unwrap();
            }
        }
    }

    pub fn suspend_node_startup(&self) {
        self.suspend.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_snapshot_cache(&self) -> SnapshotCache {
        SnapshotCache::new(self.store.get_snapshot())
    }

    pub async fn contains_transaction(&self, hash: &UInt256) -> ContainsTransactionType {
        if self.mem_pool.contains_key(hash) {
            ContainsTransactionType::ExistsInPool
        } else if NativeContract::Ledger.contains_transaction(&self.store, hash).await {
            ContainsTransactionType::ExistsInLedger
        } else {
            ContainsTransactionType::NotExist
        }
    }

    pub async fn contains_conflict_hash(&self, hash: &UInt256, signers: &[UInt160]) -> bool {
        NativeContract::Ledger.contains_conflict_hash(
            &self.store,
            hash,
            signers,
            self.settings.max_traceable_blocks,
        ).await
    }
}
impl Drop for NeoSystem {
    fn drop(&mut self) {
        // Signal all actors to shut down
        let _ = self.blockchain.sender.try_send(BlockchainMessage::Shutdown);
        let _ = self.local_node.sender.try_send(LocalNodeMessage::Shutdown);
        let _ = self.task_manager.sender.try_send(TaskManagerMessage::Shutdown);
        let _ = self.tx_router.sender.try_send(TxRouterMessage::Shutdown);

        // Wait for all actors to finish
        let _ = self.runtime.block_on(async {
            let _ = self.blockchain.handle.await;
            let _ = self.local_node.handle.await;
            let _ = self.task_manager.handle.await;
            let _ = self.tx_router.handle.await;
        });

        // Dispose plugins
        for plugin in Plugin::plugins() {
            plugin.dispose();
        }

        // Dispose header cache
        self.header_cache.dispose();

        // Close the store
        self.store.close();

        println!("NeoSystem shut down successfully");
    }
}

async fn blockchain_actor(mut rx: mpsc::Receiver<BlockchainMessage>, store: Arc<dyn Store>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            BlockchainMessage::Initialize => {
                println!("Initializing blockchain");
                // Perform initialization tasks
                // e.g., load the latest state from the store
            },
            BlockchainMessage::AddBlock(block) => {
                println!("Adding block: {}", block.hash());
                // Validate and add the block to the chain
                // Update the store
            },
            BlockchainMessage::GetBlock(hash) => {
                println!("Retrieving block: {}", hash);
                // Fetch the block from the store
                // You might want to send the result back to the requester
            },
            BlockchainMessage::GetHeight => {
                println!("Getting current height");
                // Retrieve the current height from the store
                // You might want to send the result back to the requester
            },
            BlockchainMessage::ValidateTransaction(tx) => {
                println!("Validating transaction: {}", tx.hash());
                // Perform transaction validation
                // You might want to send the result back to the requester
            },
            BlockchainMessage::Persist(block) => {
                println!("Persisting block: {}", block.hash());
                // Persist the block to the store
            },
            BlockchainMessage::Shutdown => {
                println!("Shutting down blockchain actor");
                break;
            }
        }
    }
    println!("Blockchain actor shut down");
}

async fn local_node_actor(mut rx: mpsc::Receiver<LocalNodeMessage>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            LocalNodeMessage::Start(config) => {
                println!("Starting local node with config: {:?}", config);
                // Initialize network connections based on the config
            },
            LocalNodeMessage::ConnectToPeer(address) => {
                println!("Connecting to peer: {}", address);
                // Establish connection to the specified peer
            },
            LocalNodeMessage::BroadcastTransaction(tx) => {
                println!("Broadcasting transaction: {}", tx.hash());
                // Broadcast the transaction to connected peers
            },
            LocalNodeMessage::RequestBlockByHash(hash) => {
                println!("Requesting block by hash: {}", hash);
                // Request the block from peers
            },
            LocalNodeMessage::RequestBlockByIndex(index) => {
                println!("Requesting block by index: {}", index);
                // Request the block from peers
            },
            LocalNodeMessage::Shutdown => {
                println!("Shutting down local node actor");
                break;
            }
        }
    }
    println!("Local node actor shut down");
}

pub enum ContainsTransactionType {
    NotExist,
    ExistsInPool,
    ExistsInLedger,
}