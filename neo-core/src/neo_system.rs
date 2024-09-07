// Copyright (C) 2015-2024 The Neo Project.
//
// neo_system.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::sync::Arc;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;
use crate::block::Block;
use crate::protocol_settings::ProtocolSettings;
use crate::store::Store;

pub struct NeoSystem {
    pub settings: ProtocolSettings,
    pub actor_system: ActorSystem,
    pub genesis_block: Block,
    pub blockchain: ActorRef,
    pub local_node: ActorRef,
    pub task_manager: ActorRef,
    pub tx_router: ActorRef,
    pub store_view: Arc<dyn Store>,
    pub mem_pool: MemoryPool,
    pub header_cache: HeaderCache,
    pub relay_cache: RelayCache,
    services: Arc<Vec<Box<dyn Any + Send + Sync>>>,
    store: Arc<dyn Store>,
    storage_provider: Arc<dyn StoreProvider>,
    start_message: Option<ChannelsConfig>,
    suspend: AtomicI32,
}

impl NeoSystem {
    pub fn new(settings: ProtocolSettings, storage_provider: Option<Arc<dyn StoreProvider>>, storage_path: Option<&str>) -> Self {
        let storage_provider = storage_provider.unwrap_or_else(|| Arc::new(MemoryStoreProvider::new()));
        let store = storage_provider.get_store(storage_path);
        let genesis_block = Self::create_genesis_block(&settings);
        let mem_pool = MemoryPool::new();
        let actor_system = ActorSystem::new("NeoSystem");

        let mut system = Self {
            settings,
            actor_system: actor_system.clone(),
            genesis_block,
            blockchain: ActorRef::default(),
            local_node: ActorRef::default(),
            task_manager: ActorRef::default(),
            tx_router: ActorRef::default(),
            store_view: store.clone(),
            mem_pool,
            header_cache: HeaderCache::new(),
            relay_cache: RelayCache::new(100),
            services: Arc::new(Vec::new()),
            store,
            storage_provider,
            start_message: None,
            suspend: AtomicI32::new(0),
        };

        system.blockchain = actor_system.spawn_actor(Props::new(move |_| Blockchain::new(system.clone())));
        system.local_node = actor_system.spawn_actor(Props::new(move |_| LocalNode::new(system.clone())));
        system.task_manager = actor_system.spawn_actor(Props::new(move |_| TaskManager::new(system.clone())));
        system.tx_router = actor_system.spawn_actor(Props::new(move |_| TransactionRouter::new(system.clone())));

        for plugin in Plugin::get_plugins() {
            plugin.on_system_loaded(&system);
        }

        system.blockchain.ask(BlockchainMessage::Initialize).wait().unwrap();

        system
    }

    fn create_genesis_block(settings: &ProtocolSettings) -> Block {
        Block {
            header: Header {
                prev_hash: UInt256::zero(),
                merkle_root: UInt256::zero(),
                timestamp: 1468595301000, // 2016-07-15 15:08:21 UTC
                nonce: 2083236893,
                index: 0,
                primary_index: 0,
                next_consensus: Contract::get_bft_address(&settings.standby_validators),
                witness: Witness {
                    invocation_script: vec![],
                    verification_script: vec![OpCode::PUSH1 as u8],
                },
                ..Default::default()
            },
            transactions: vec![],
        }
    }

    pub fn add_service<T: 'static + Send + Sync>(&mut self, service: T) {
        Arc::get_mut(&mut self.services).unwrap().push(Box::new(service));
        // Trigger ServiceAdded event if needed
    }

    pub fn get_service<T: 'static>(&self, filter: Option<Box<dyn Fn(&T) -> bool>>) -> Option<&T> {
        self.services.iter()
            .filter_map(|s| s.downcast_ref::<T>())
            .find(|s| filter.as_ref().map_or(true, |f| f(s)))
    }

    pub fn ensure_stopped(&self, actor: &ActorRef) {
        let inbox = self.actor_system.create_inbox();
        inbox.watch(actor);
        self.actor_system.stop(actor);
        inbox.receive(Duration::from_secs(300)).unwrap();
    }

    pub fn load_store(&self, path: &str) -> Arc<dyn Store> {
        self.storage_provider.get_store(Some(path))
    }

    pub fn resume_node_startup(&mut self) -> bool {
        if self.suspend.fetch_sub(1, Ordering::SeqCst) != 1 {
            return false;
        }
        if let Some(message) = self.start_message.take() {
            self.local_node.tell(message);
        }
        true
    }

    pub fn start_node(&mut self, config: ChannelsConfig) {
        self.start_message = Some(config);
        if self.suspend.load(Ordering::SeqCst) == 0 {
            self.local_node.tell(self.start_message.take().unwrap());
        }
    }

    pub fn suspend_node_startup(&self) {
        self.suspend.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_snapshot_cache(&self) -> SnapshotCache {
        SnapshotCache::new(self.store.get_snapshot())
    }

    pub fn contains_transaction(&self, hash: &UInt256) -> ContainsTransactionType {
        if self.mem_pool.contains_key(hash) {
            ContainsTransactionType::ExistsInPool
        } else if self.store_view.contains_transaction(hash) {
            ContainsTransactionType::ExistsInLedger
        } else {
            ContainsTransactionType::NotExist
        }
    }

    pub fn contains_conflict_hash(&self, hash: &UInt256, signers: &[UInt160]) -> bool {
        self.store_view.contains_conflict_hash(hash, signers, self.settings.max_traceable_blocks)
    }
}

impl Drop for NeoSystem {
    fn drop(&mut self) {
        self.ensure_stopped(&self.local_node);
        self.ensure_stopped(&self.blockchain);
        for plugin in Plugin::get_plugins() {
            plugin.dispose();
        }
        self.actor_system.terminate();
        self.actor_system.wait_for_terminate();
    }
}

pub enum ContainsTransactionType {
    ExistsInPool,
    ExistsInLedger,
    NotExist,
}
