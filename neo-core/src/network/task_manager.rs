use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use actix::Actor;
use crate::io::caching::HashSetCache;
use crate::neo_system::NeoSystem;
use crate::network::PeerMessage::Timer;
use crate::network::TaskSession;
use crate::uint256::UInt256;

pub struct TaskManager {
    system: NeoSystem,
    known_hashes: HashSetCache<UInt256>,
    global_inv_tasks: HashMap<UInt256, i32>,
    global_index_tasks: HashMap<u32, i32>,
    sessions: HashMap<ActorRef, TaskSession>,
    timer: Cancelable,
    last_seen_persisted_index: u32,
}

impl TaskManager {
    const TIMER_INTERVAL: Duration = Duration::from_secs(30);
    const TASK_TIMEOUT: Duration = Duration::from_mins(1);
    const HEADER_TASK_HASH: UInt256 = UInt256::zero();
    const MAX_CONCURRENT_TASKS: i32 = 3;

    pub fn new(system: NeoSystem) -> Self {
        let known_hashes = HashSetCache::new(system.mem_pool.capacity() * 2 / 5);
        let timer = Context::system().scheduler().schedule_tell_repeatedly_cancelable(
            Self::TIMER_INTERVAL,
            Self::TIMER_INTERVAL,
            Context::self(),
            Timer,
            ActorRef::no_sender(),
        );

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

    // ... implement other methods ...

    pub fn props(system: NeoSystem) -> Props {
        Props::new(move || TaskManager::new(system)).with_mailbox("task-manager-mailbox")
    }
}

impl Actor for TaskManager {
    type Context = Context<Self>;

    fn receive(&mut self, msg: Self::Message, ctx: &mut Self::Context) {
        match msg {
            Register(version) => self.on_register(version, ctx.sender()),
            Update(update) => self.on_update(update, ctx.sender()),
            NewTasks(payload) => self.on_new_tasks(payload, ctx.sender()),
            RestartTasks(payload) => self.on_restart_tasks(payload),
            Headers(headers) => self.on_headers(headers, ctx.sender()),
            Inventory(inventory) => self.on_task_completed(inventory, ctx.sender()),
            PersistCompleted(block) => self.on_persist_completed(block),
            RelayResult { inventory, result } => {
                if let Some(invalid_block) = inventory.downcast_ref::<Block>() {
                    if result == VerifyResult::Invalid {
                        self.on_invalid_block(invalid_block);
                    }
                }
            }
            Timer => self.on_timer(),
            Terminated(actor) => self.on_terminated(actor),
        }
    }
}

// ... implement TaskManagerMailbox ...
