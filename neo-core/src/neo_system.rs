use std::any::Any;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use chrono::{DateTime, Utc};
use actix::prelude::*;
use tokio::sync::mpsc;
use neo_type::{H160, H256};
use crate::contract::Contract;
use crate::ledger::header_cache::HeaderCache;
use crate::ledger::memory_pool::MemoryPool;
use crate::network::{ChannelsConfig, LocalNode, TaskManager};
use crate::network::payloads::{Block, Transaction, Witness};
use crate::persistence::{MemoryStore, SnapshotCache, StoreProviderTrait};
use crate::protocol_settings::ProtocolSettings;
use crate::store::Store;
use crate::transaction::Transaction;
use crate::native_contract::NativeContract;
use crate::plugin::Plugin;
use crate::utility;
use crate::vm::OpCode;
use serde::{Serialize, Deserialize};
use getset::{Getters, Setters};
use crate::ledger::blockchain::Blockchain;
use crate::ledger::transaction_router::ledger::TransactionRouter;

#[derive(Getters, Setters)]
pub struct NeoSystem {
    #[getset(get = "pub")]
    settings: Arc<ProtocolSettings>,
    #[getset(get = "pub")]
    genesis_block: Block,
    #[getset(get = "pub")]
    blockchain: Addr<Blockchain>,
    #[getset(get = "pub")]
    local_node: Addr<LocalNode>,
    #[getset(get = "pub")]
    task_manager: Addr<TaskManager>,
    #[getset(get = "pub")]
    tx_router: Addr<TransactionRouter>,
    #[getset(get = "pub")]
    mem_pool: Arc<MemoryPool>,
    #[getset(get = "pub")]
    header_cache: Arc<HeaderCache>,
    #[getset(get = "pub")]
    relay_cache: Arc<Mutex<HashMap<H256, bool>>>,
    services: Arc<Mutex<Vec<Box<dyn Any + Send + Sync>>>>,
    store: Arc<dyn Store>,
    storage_provider: Arc<dyn StoreProviderTrait>,
    start_message: Arc<Mutex<Option<ChannelsConfig>>>,
    suspend: Arc<AtomicI32>,
}

impl NeoSystem {
    pub fn new(
        settings: Arc<ProtocolSettings>,
        storage_provider: Option<Arc<dyn StoreProviderTrait>>,
        storage_path: Option<String>,
    ) -> Self {
        let storage_provider = storage_provider.unwrap_or_else(|| Arc::new(MemoryStore::new()) as Arc<dyn StoreProviderTrait>);
        let store = storage_provider.get_store(storage_path);
        let genesis_block = Self::create_genesis_block(&settings);
        let mem_pool = Arc::new(MemoryPool::new());
        let header_cache = Arc::new(HeaderCache::new());

        let system = System::new();
        let blockchain = system.block_on(Blockchain::new(store.clone()).start());
        let local_node = system.block_on(LocalNode::new().start());
        let task_manager = system.block_on(TaskManager::new().start());
        let tx_router = system.block_on(TransactionRouter::new().start());

        let neo_system = Self {
            settings,
            genesis_block,
            blockchain,
            local_node,
            task_manager,
            tx_router,
            mem_pool,
            header_cache,
            relay_cache: Arc::new(Mutex::new(HashMap::new())),
            services: Arc::new(Mutex::new(Vec::new())),
            store,
            storage_provider,
            start_message: Arc::new(Mutex::new(None)),
            suspend: Arc::new(AtomicI32::new(0)),
        };

        for plugin in Plugin::plugins() {
            plugin.on_system_loaded(&neo_system);
        }

        system.block_on(neo_system.blockchain.send(BlockchainMessage::Initialize));

        neo_system
    }

    fn create_genesis_block(settings: &ProtocolSettings) -> Block {
        let timestamp = DateTime::parse_from_rfc3339("2016-07-15T15:08:21Z")
            .unwrap()
            .timestamp_millis();

        Block {
            header: Header {
                prev_hash: H256::zero(),
                merkle_root: H256::zero(),
                timestamp,
                nonce: 2083236893,
                index: 0,
                next_consensus: Contract::get_bft_address(&settings.standby_validators()),
                witness: Witness {
                    invocation_script: Vec::new(),
                    verification_script: vec![OpCode::Push1 as u8],
                },
                ..Default::default()
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

    pub fn load_store(&self, path: &str) -> Arc<dyn Store> {
        self.storage_provider.get_store(Some(path.to_string()))
    }

    pub async fn resume_node_startup(&self) -> bool {
        if self.suspend.fetch_sub(1, Ordering::SeqCst) != 1 {
            return false;
        }
        let mut start_message = self.start_message.lock().await;
        if let Some(config) = start_message.take() {
            self.local_node.do_send(LocalNodeMessage::Start(config));
        }
        true
    }

    pub async fn start_node(&self, config: ChannelsConfig) {
        let mut start_message = self.start_message.lock().await;
        *start_message = Some(config.clone());

        if self.suspend.load(Ordering::SeqCst) == 0 {
            if let Some(config) = start_message.take() {
                self.local_node.do_send(LocalNodeMessage::Start(config));
            }
        }
    }

    pub fn suspend_node_startup(&self) {
        self.suspend.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_snapshot_cache(&self) -> SnapshotCache {
        SnapshotCache::new(self.store.get_snapshot())
    }

    pub async fn contains_transaction(&self, hash: &H256) -> ContainsTransactionType {
        if self.mem_pool.contains_key(hash) {
            ContainsTransactionType::ExistsInPool
        } else if NativeContract::Ledger.contains_transaction(&self.store, hash).await {
            ContainsTransactionType::ExistsInLedger
        } else {
            ContainsTransactionType::NotExist
        }
    }

    pub async fn contains_conflict_hash(&self, hash: &H256, signers: &[H160]) -> bool {
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
        self.blockchain.do_send(BlockchainMessage::Shutdown);
        self.local_node.do_send(LocalNodeMessage::Shutdown);
        self.task_manager.do_send(TaskManagerMessage::Shutdown);
        self.tx_router.do_send(TxRouterMessage::Shutdown);

        // Wait for all actors to finish
        System::current().stop();

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

#[derive(Message)]
#[rtype(result = "()")]
pub enum BlockchainMessage {
    Initialize,
    AddBlock(Block),
    GetBlock(H256),
    GetHeight,
    ValidateTransaction(Transaction),
    Persist(Block),
    Shutdown,
}

#[derive(Message)]
#[rtype(result = "()")]
pub enum LocalNodeMessage {
    Start(ChannelsConfig),
    ConnectToPeer(String),
    BroadcastTransaction(Transaction),
    RequestBlockByHash(H256),
    RequestBlockByIndex(u32),
    Shutdown,
}

#[derive(Message)]
#[rtype(result = "()")]
pub enum TaskManagerMessage {
    ScheduleTask(Task),
    CancelTask(TaskId),
    GetTaskStatus(TaskId),
    Shutdown,
}

#[derive(Message)]
#[rtype(result = "()")]
pub enum TxRouterMessage {
    RouteTransaction(Transaction),
    GetTransactionStatus(H256),
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainsTransactionType {
    NotExist,
    ExistsInPool,
    ExistsInLedger,
}