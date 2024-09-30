use std::collections::{HashMap, HashSet, LinkedList, VecDeque};
use std::sync::Arc;
use actix::prelude::*;
use lazy_static::lazy_static;
use crate::block::Block;
use crate::ledger::blockchain_application_executed::ledger::ApplicationExecuted;
use crate::ledger::verify_result::VerifyResult;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::trigger_type::TriggerType;
use crate::neo_system::NeoSystem;
use crate::network::LocalNode;
use crate::store::Store;
use crate::network::payloads::{IInventory, IVerifiable, Transaction};
use neo_type::{H160, H256};
use neo_vm::ScriptBuilder;
use crate::io::priority_mailbox::PriorityMailbox;
use getset::{Getters, Setters};
use serde::{Serialize, Deserialize};

pub type CommittingHandler = fn(system: &NeoSystem, block: &Block, snapshot: &dyn Store<WriteBatch=()>, application_executed_list: &[ApplicationExecuted]);
pub type CommittedHandler = fn(system: &NeoSystem, block: &Block);

/// Actor used to verify and relay `IInventory`.
#[derive(Getters, Setters)]
pub struct Blockchain {
    system: Arc<NeoSystem>,
    block_cache: HashMap<H256, Block>,
    block_cache_unverified: HashMap<u32, UnverifiedBlocksList>,
    extensible_witness_white_list: Option<HashSet<H160>>,
}

#[derive(Serialize, Deserialize)]
pub struct PersistCompleted {
    pub block: Block,
}

#[derive(Serialize, Deserialize)]
pub struct Import {
    pub blocks: Vec<Block>,
    #[serde(default = "default_verify")]
    pub verify: bool,
}

fn default_verify() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
pub struct ImportCompleted;

#[derive(Serialize, Deserialize)]
pub struct FillMemoryPool {
    pub transactions: Vec<Transaction>,
}

#[derive(Serialize, Deserialize)]
pub struct FillCompleted;

#[derive(Serialize, Deserialize)]
pub struct Reverify {
    pub inventories: Vec<Box<dyn IInventory>>,
}

#[derive(Serialize, Deserialize)]
pub struct RelayResult {
    pub inventory: Box<dyn IInventory>,
    pub result: VerifyResult,
}

struct Initialize;
struct UnverifiedBlocksList {
    blocks: LinkedList<Block>,
    nodes: HashSet<Addr<dyn Actor>>,
}

lazy_static! {
    static ref ON_PERSIST_SCRIPT: Vec<u8> = {
        let mut sb = ScriptBuilder::new();
        sb.emit_syscall(ApplicationEngine::SYSTEM_CONTRACT_NATIVE_ON_PERSIST);
        sb.to_array()
    };
    static ref POST_PERSIST_SCRIPT: Vec<u8> = {
        let mut sb = ScriptBuilder::new();
        sb.emit_syscall(ApplicationEngine::SYSTEM_CONTRACT_NATIVE_POST_PERSIST);
        sb.to_array()
    };
}

const MAX_TX_TO_REVERIFY_PER_IDLE: usize = 10;

impl Blockchain {
    pub fn new(system: Arc<NeoSystem>) -> Self {
        Self {
            system,
            block_cache: HashMap::new(),
            block_cache_unverified: HashMap::new(),
            extensible_witness_white_list: None,
        }
    }

    fn on_import(&mut self, blocks: Vec<Block>, verify: bool) {
        let mut current_height = NativeContract::Ledger::current_index(&self.system.store_view());
        for block in blocks {
            if block.header().index() <= current_height {
                continue;
            }
            if block.header().index() != current_height + 1 {
                // Handle error
                return;
            }
            if verify && !block.verify(&self.system.settings, &self.system.store_view()) {
                // Handle error
                return;
            }
            self.persist(&block);
            current_height += 1;
        }
        self.sender().do_send(ImportCompleted);
    }

    fn add_unverified_block_to_cache(&mut self, block: Block) {
        let list = self.block_cache_unverified
            .entry(block.index())
            .or_insert_with(|| UnverifiedBlocksList {
                blocks: LinkedList::new(),
                nodes: HashSet::new(),
            });

        if list.blocks.iter().any(|b| b.hash() == block.hash()) {
            return;
        }

        if !list.nodes.insert(self.sender()) {
            self.sender().do_send(Tcp::Abort);
            return;
        }

        list.blocks.push_back(block);
    }

    fn on_fill_memory_pool(&mut self, transactions: Vec<Transaction>) {
        self.system.mem_pool.invalidate_all_transactions();

        let snapshot = self.system.store_view();

        for tx in transactions {
            if NativeContract::Ledger::contains_transaction(&snapshot, &tx.hash()) {
                continue;
            }
            if NativeContract::Ledger::contains_conflict_hash(&snapshot, &tx.hash(), tx.signers().iter().map(|s| s.account()), self.system.settings.max_traceable_blocks) {
                continue;
            }
            self.system.mem_pool.try_remove_unverified(&tx.hash());
            self.system.mem_pool.try_add(&tx, &snapshot);
        }

        self.sender().do_send(FillCompleted);
    }

    fn on_initialize(&mut self) {
        if !NativeContract::Ledger::initialized(&self.system.store_view()) {
            self.persist(&self.system.genesis_block);
        }
        self.sender().do_send(());
    }

    fn on_inventory(&mut self, inventory: Box<dyn IInventory>, relay: bool) {
        let result = match inventory.as_any().downcast_ref::<Block>() {
            Some(block) => self.on_new_block(block),
            None => match inventory.as_any().downcast_ref::<Transaction>() {
                Some(transaction) => self.on_new_transaction(transaction),
                None => match inventory.as_any().downcast_ref::<ExtensiblePayload>() {
                    Some(payload) => self.on_new_extensible_payload(payload),
                    None => VerifyResult::Invalid,
                },
            },
        };

        if result == VerifyResult::Succeed && relay {
            self.system.local_node.do_send(LocalNode::RelayDirectly { inventory });
        }
        self.send_relay_result(inventory, result);
    }

    fn persist(&mut self, block: &Block) {
        let snapshot = self.system.store_view();
        let persisting_block = block.clone();
        let mut engine = ApplicationEngine::new(TriggerType::ON_PERSIST, &persisting_block, &snapshot, self.system.settings.gas_free);
        engine.load_script(&ON_PERSIST_SCRIPT);
        if engine.execute().is_ok() {
            engine.commit();
        }
        let mut list_executed = Vec::new();
        list_executed.push(ApplicationExecuted::new(&engine));

        if let Some(handler) = self.system.settings.committing_handler {
            handler(&self.system, &persisting_block, &snapshot, &list_executed);
        }

        snapshot.commit();

        if let Some(handler) = self.system.settings.committed_handler {
            handler(&self.system, &persisting_block);
        }

        self.sender().do_send(PersistCompleted { block: persisting_block });
    }

    fn send_relay_result(&self, inventory: Box<dyn IInventory>, result: VerifyResult) {
        let rr = RelayResult {
            inventory,
            result,
        };
        self.sender().do_send(rr);
        self.system.event_bus.publish(RelayResultReason::new(inventory, result));
    }

    fn update_extensible_witness_white_list(settings: &ProtocolSettings, snapshot: &Store) -> HashSet<H160> {
        let committee = NativeContract::NEO.get_committee_members(snapshot);
        let validators = NativeContract::NEO.get_next_block_validators(snapshot);
        let mut white_list = HashSet::new();
        white_list.extend(committee);
        white_list.extend(validators);
        white_list.extend(settings.extensible_witness_white_list.iter().cloned());
        white_list
    }
}

impl Actor for Blockchain {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(1000);
    }
}

impl Handler<Initialize> for Blockchain {
    type Result = ();

    fn handle(&mut self, _msg: Initialize, _ctx: &mut Self::Context) -> Self::Result {
        self.on_initialize();
    }
}

impl Handler<Import> for Blockchain {
    type Result = ();

    fn handle(&mut self, msg: Import, _ctx: &mut Self::Context) -> Self::Result {
        self.on_import(msg.blocks, msg.verify);
    }
}

impl Handler<FillMemoryPool> for Blockchain {
    type Result = ();

    fn handle(&mut self, msg: FillMemoryPool, _ctx: &mut Self::Context) -> Self::Result {
        self.on_fill_memory_pool(msg.transactions);
    }
}

impl Handler<Reverify> for Blockchain {
    type Result = ();

    fn handle(&mut self, msg: Reverify, _ctx: &mut Self::Context) -> Self::Result {
        for inventory in msg.inventories {
            self.on_inventory(inventory, false);
        }
    }
}

pub fn create_blockchain_actor(system: Arc<NeoSystem>) -> Addr<Blockchain> {
    let mailbox = SyncArbiter::start(1, move || {
        let settings = Arc::new(system.settings.clone());
        let config = Arc::new(system.config.clone());
        PriorityMailbox::new(settings, config)
    });

    Actor::create(|_| Blockchain::new(system.clone()))
        .with_mailbox(mailbox)
        .start()
}

struct BlockchainMailbox {
    inner: VecDeque<Envelope<Blockchain>>,
}

impl PriorityMailbox for BlockchainMailbox {
    fn new() -> Self {
        BlockchainMailbox {
            inner: VecDeque::new(),
        }
    }

    fn enqueue(&mut self, msg: Envelope<Blockchain>) {
        match msg.message() {
            // High priority messages
            PersistCompleted { .. } | ImportCompleted | FillCompleted => {
                self.inner.push_front(msg);
            }
            // Medium priority messages
            Import { .. } | FillMemoryPool { .. } | Reverify { .. } => {
                let index = self.inner.iter().position(|e| !matches!(e.message(), 
                    PersistCompleted { .. } | ImportCompleted | FillCompleted
                )).unwrap_or(self.inner.len());
                self.inner.insert(index, msg);
            }
            // Low priority messages (default)
            _ => {
                self.inner.push_back(msg);
            }
        }
    }

    fn dequeue(&mut self) -> Option<Envelope<Blockchain>> {
        self.inner.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}
