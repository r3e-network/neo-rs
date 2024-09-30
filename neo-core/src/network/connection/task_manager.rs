use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use actix::{Actor, Context, Handler, Addr};
use crate::neo_system::NeoSystem;
use crate::network::PeerMessage::Timer;
use crate::network::TaskSession;
use crate::network::payloads::{InvPayload, InventoryType, VersionPayload};
use crate::block::Block;

pub struct TaskManager {
    system: NeoSystem,
    known_hashes: HashSetCache<H256>,
    global_inv_tasks: HashMap<H256, i32>,
    global_index_tasks: HashMap<u32, i32>,
    sessions: HashMap<Addr<dyn Actor<Context=()>>, TaskSession>,
    timer: actix::Cancelable,
    last_seen_persisted_index: u32,
}

impl TaskManager {
    const TIMER_INTERVAL: Duration = Duration::from_secs(30);
    const TASK_TIMEOUT: Duration = Duration::from_secs(60);
    const HEADER_TASK_HASH: H256 = H256::zero();
    const MAX_CONCURRENT_TASKS: i32 = 3;

    pub fn new(system: NeoSystem) -> Self {
        let known_hashes = HashSetCache::new(system.mem_pool.capacity() * 2 / 5);
        let timer = Context::current().run_interval(Self::TIMER_INTERVAL, |act, ctx| {
            act.handle(Timer, ctx);
        });

        Self {
            system,
            known_hashes,
            global_inv_tasks: HashMap::new(),
            global_index_tasks: HashMap::new(),
            sessions: HashMap::new(),
            timer,
            last_seen_persisted_index: 0,
        }
    }

    fn has_header_task(&self) -> bool {
        self.global_inv_tasks.contains_key(&Self::HEADER_TASK_HASH)
    }

    fn on_register(&mut self, version: VersionPayload, addr: Addr<dyn Actor<Context=()>>) {
        let session = TaskSession::new(version);
        self.sessions.insert(addr, session);
    }

    fn on_update(&mut self, msg: Update, addr: Addr<dyn Actor<Context=()>>) {
        if let Some(session) = self.sessions.get_mut(&addr) {
            session.last_seen = Instant::now();
            session.available_tasks += msg.available_tasks;
        }
    }

    fn on_new_tasks(&mut self, payload: InvPayload, addr: Addr<dyn Actor<Context=()>>) {
        if let Some(session) = self.sessions.get_mut(&addr) {
            for hash in payload.hashes {
                if !self.known_hashes.contains(&hash) {
                    self.known_hashes.insert(hash);
                    match payload.type_ {
                        InventoryType::TX => {
                            self.global_inv_tasks.entry(hash).or_insert(0);
                        },
                        InventoryType::Block => {
                            if !self.has_header_task() {
                                self.global_inv_tasks.insert(Self::HEADER_TASK_HASH, 0);
                            }
                            self.global_inv_tasks.entry(hash).or_insert(0);
                        },
                        _ => {}
                    }
                }
            }
        }
    }

    fn on_restart_tasks(&mut self, payload: InvPayload) {
        for hash in payload.hashes {
            self.global_inv_tasks.entry(hash).and_modify(|count| *count = 0);
        }
    }

    fn on_timer(&mut self) {
        let now = Instant::now();
        self.sessions.retain(|_, session| {
            now.duration_since(session.last_seen) < Self::TASK_TIMEOUT
        });

        self.global_inv_tasks.retain(|&hash, count| {
            if *count >= Self::MAX_CONCURRENT_TASKS {
                false
            } else {
                *count += 1;
                true
            }
        });

        self.global_index_tasks.retain(|_, count| {
            if *count >= Self::MAX_CONCURRENT_TASKS {
                false
            } else {
                *count += 1;
                true
            }
        });

        if let Some(persisted_index) = self.system.blockchain.persisted_header_index() {
            if persisted_index > self.last_seen_persisted_index {
                self.last_seen_persisted_index = persisted_index;
                for index in self.last_seen_persisted_index..=persisted_index {
                    self.global_index_tasks.entry(index).or_insert(0);
                }
            }
        }
    }

    pub fn props(system: NeoSystem) -> actix::Props {
        actix::Props::new(move || TaskManager::new(system))
    }
}

impl Actor for TaskManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(1000);
    }
}

impl Handler<Register> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: Register, ctx: &mut Self::Context) {
        self.on_register(msg.version, ctx.address());
    }
}

impl Handler<Update> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: Update, ctx: &mut Self::Context) {
        self.on_update(msg, ctx.address());
    }
}

impl Handler<NewTasks> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: NewTasks, ctx: &mut Self::Context) {
        self.on_new_tasks(msg.payload, ctx.address());
    }
}

impl Handler<RestartTasks> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: RestartTasks, _: &mut Self::Context) {
        self.on_restart_tasks(msg.payload);
    }
}

impl Handler<Headers> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: Headers, ctx: &mut Self::Context) {
        self.on_headers(msg.headers, ctx.address());
    }
}

impl Handler<Inventory> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: Inventory, ctx: &mut Self::Context) {
        self.on_task_completed(msg.inventory, ctx.address());
    }
}

impl Handler<PersistCompleted> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: Blockchain::PersistCompleted, _: &mut Self::Context) {
        self.on_persist_completed(msg.block);
    }
}

impl Handler<RelayResult> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: RelayResult, _: &mut Self::Context) {
        if let Some(invalid_block) = msg.inventory.downcast_ref::<Block>() {
            if msg.result == VerifyResult::Invalid {
                self.on_invalid_block(invalid_block);
            }
        }
    }
}

impl Handler<Timer> for TaskManager {
    type Result = ();

    fn handle(&mut self, _: Timer, _: &mut Self::Context) {
        self.on_timer();
    }
}

impl Handler<actix::Terminated> for TaskManager {
    type Result = ();

    fn handle(&mut self, msg: actix::Terminated, _: &mut Self::Context) {
        self.on_terminated(msg.actor);
    }
}


use actix::dev::{Envelope, MessageData, PriorityQueue};
use actix::prelude::*;
use std::collections::VecDeque;
use neo_io::{CacheInterface, HashSetCache};
use neo_type::H256;
use crate::ledger::blockchain::{Blockchain, PersistCompleted, RelayResult};
use crate::payload::Inventory;

pub struct TaskManagerMailbox {
    inner: VecDeque<Envelope<TaskManager>>,
}

impl PriorityMailbox for TaskManagerMailbox {
    fn new() -> Self {
        TaskManagerMailbox {
            inner: VecDeque::new(),
        }
    }

    fn enqueue(&mut self, msg: Envelope<TaskManager>) {
        match msg.message() {
            Register { .. } | Update { .. } | RestartTasks { .. } => {
                self.inner.push_front(msg);
            }
            NewTasks { payload } if payload.inv_type == InventoryType::Block || payload.inv_type == InventoryType::Extensible => {
                self.inner.push_front(msg);
            }
            _ => {
                self.inner.push_back(msg);
            }
        }
    }

    fn dequeue(&mut self) -> Option<Envelope<TaskManager>> {
        self.inner.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn filter(&mut self, f: impl Fn(&Envelope<TaskManager>) -> bool) {
        self.inner.retain(f);
    }
}

impl TaskManagerMailbox {
    fn shall_drop(&self, msg: &Envelope<TaskManager>) -> bool {
        if let NewTasks { payload } = msg.message() {
            self.inner.iter().any(|e| {
                if let NewTasks { payload: existing_payload } = e.message() {
                    existing_payload.inv_type == payload.inv_type && existing_payload.hashes == payload.hashes
                } else {
                    false
                }
            })
        } else {
            false
        }
    }
}
