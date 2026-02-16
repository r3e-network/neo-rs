//! Task manager actor: tracks inventory tasks and relays to peers.
//!
//! This module implements the task manager that coordinates block and header
//! synchronization across multiple peers, mirroring the C# `Neo.Network.P2P.TaskManager`.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    TaskManager Actor                         │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                   Task Tracking                          ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  ││
//! │  │  │ Pending  │  │ In-Flight│  │ Known Hashes         │  ││
//! │  │  │ Tasks    │  │ Requests │  │ (already have)       │  ││
//! │  │  └──────────┘  └──────────┘  └──────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                   Session Management                     ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  ││
//! │  │  │ Peer     │  │ Task     │  │ Timeout              │  ││
//! │  │  │ Sessions │  │ Queues   │  │ Handling             │  ││
//! │  │  └──────────┘  └──────────┘  └──────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Components
//!
//! - [`TaskManager`]: Actor coordinating inventory synchronization
//! - [`TaskManagerCommand`]: Messages for task registration and completion
//! - [`TaskSession`]: Per-peer session tracking pending requests
//!
//! # Synchronization Flow
//!
//! 1. Receive `INV` message with block/header hashes
//! 2. Filter out already-known hashes
//! 3. Queue unknown hashes as pending tasks
//! 4. Dispatch `GETDATA`/`GETHEADERS` to available peers
//! 5. Track in-flight requests with timeout
//! 6. Handle responses and mark tasks complete
//! 7. Retry timed-out tasks with different peers
//!
//! # Configuration
//!
//! - `TIMER_INTERVAL`: 30s housekeeping interval
//! - `TASK_TIMEOUT`: 60s request timeout
//! - `MAX_CONCURRENT_TASKS`: 3 parallel requests per peer
//
// Copyright (C) 2015-2025 The Neo Project.
//
// task_manager.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::payloads::{
    InventoryType, VersionPayload,
    block::Block,
    get_block_by_index_payload::GetBlockByIndexPayload,
    header::Header,
    inv_payload::{HEADER_PREFETCH_COUNT, InvPayload, MAX_HASHES_COUNT},
};
use super::task_session::TaskSession;
use crate::UInt256;
use crate::akka::{
    Actor, ActorContext, ActorRef, ActorResult, Cancelable, EventStreamHandle, Props, Terminated,
};
use crate::ledger::{PersistCompleted, RelayResult, VerifyResult};
use crate::neo_system::NeoSystemContext;
use crate::network::p2p::{NetworkMessage, ProtocolMessage, RemoteNodeCommand};
use crate::smart_contract::native::LedgerContract;
use async_trait::async_trait;
use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tracing::{trace, warn};

/// Interval for task manager housekeeping (optimized for faster sync).
const TIMER_INTERVAL: Duration = Duration::from_secs(3);
/// Timeout applied to in-flight inventory requests (reduced for faster recovery).
const TASK_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum concurrent tasks per peer (significantly increased for faster sync).
const MAX_CONCURRENT_TASKS: u32 = 30;
const HEADER_TASK_HASH: UInt256 = UInt256 {
    value1: 0,
    value2: 0,
    value3: 0,
    value4: 0,
};
struct SessionEntry {
    actor: ActorRef,
    session: TaskSession,
}

/// Actor-independent state for the task manager.
pub struct TaskManager {
    system: Option<Arc<NeoSystemContext>>,
    sessions: HashMap<String, SessionEntry>,
    known_hashes: HashSet<UInt256>,
    known_hash_order: VecDeque<UInt256>,
    known_hash_capacity: usize,
    event_stream: Option<EventStreamHandle>,
    last_seen_persisted_index: u32,
    global_inv_tasks: HashMap<UInt256, u32>,
    global_index_tasks: HashMap<u32, u32>,
    timer_interval: Duration,
    task_timeout: Duration,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            system: None,
            sessions: HashMap::with_capacity(32),
            known_hashes: HashSet::with_capacity(4096),
            known_hash_order: VecDeque::with_capacity(4096),
            known_hash_capacity: 1024,
            event_stream: None,
            last_seen_persisted_index: 0,
            global_inv_tasks: HashMap::with_capacity(256),
            global_index_tasks: HashMap::with_capacity(256),
            timer_interval: TIMER_INTERVAL,
            task_timeout: TASK_TIMEOUT,
        }
    }

    pub fn props() -> Props {
        Props::new(|| TaskManagerActor::new(Self::new()))
    }

    fn attach_system(&mut self, context: Arc<NeoSystemContext>, ctx: &ActorContext) {
        trace!(target: "neo", "task manager attached to system context");
        let capacity = context.memory_pool().lock().capacity.max(100);
        self.known_hash_capacity = capacity;
        let stream = context.event_stream();
        stream.subscribe::<PersistCompleted>(ctx.self_ref());
        stream.subscribe::<RelayResult>(ctx.self_ref());
        self.event_stream = Some(stream);
        self.system = Some(context);
    }

    fn has_header_task(&self) -> bool {
        self.global_inv_tasks.contains_key(&HEADER_TASK_HASH)
    }

    fn increment_inv_task(&mut self, hash: UInt256) -> bool {
        let entry = self.global_inv_tasks.entry(hash).or_insert(0);
        if *entry >= MAX_CONCURRENT_TASKS {
            return false;
        }
        *entry += 1;
        true
    }

    fn decrement_inv_task(&mut self, hash: &UInt256) {
        if let Some(entry) = self.global_inv_tasks.get_mut(hash) {
            if *entry > 1 {
                *entry -= 1;
            } else {
                self.global_inv_tasks.remove(hash);
            }
        }
    }

    fn increment_index_task(&mut self, index: u32) -> bool {
        let entry = self.global_index_tasks.entry(index).or_insert(0);
        if *entry >= MAX_CONCURRENT_TASKS {
            return false;
        }
        *entry += 1;
        true
    }

    fn decrement_index_task(&mut self, index: u32) {
        if let Some(entry) = self.global_index_tasks.get_mut(&index) {
            if *entry > 1 {
                *entry -= 1;
            } else {
                self.global_index_tasks.remove(&index);
            }
        }
    }

    fn forget_hash(&mut self, hash: &UInt256) {
        if self.known_hashes.remove(hash) {
            self.known_hash_order.retain(|candidate| candidate != hash);
        }
    }

    fn remove_available_task_from_all(&mut self, hash: &UInt256) {
        for entry in self.sessions.values_mut() {
            entry.session.available_tasks.remove(hash);
        }
    }

    fn register_session(
        &mut self,
        actor: ActorRef,
        version: VersionPayload,
        ctx: &mut ActorContext,
    ) {
        let path = actor.path().to_string();
        if self.sessions.contains_key(&path) {
            trace!(target: "neo", actor = %path, "task session already registered");
            return;
        }

        if let Err(err) = ctx.watch(&actor) {
            warn!(target: "neo", actor = %path, error = %err, "failed to watch peer session");
        }

        let session = TaskSession::new(&version);
        self.sessions.insert(
            path.clone(),
            SessionEntry {
                actor: actor.clone(),
                session,
            },
        );
        self.request_tasks_for_path(&path);
    }

    fn update_session(&mut self, actor: &ActorRef, last_block_index: u32) {
        let path = actor.path().to_string();
        if let Some(entry) = self.sessions.get_mut(&path) {
            entry.session.update_last_block_index(last_block_index);
        }
        self.request_tasks_for_path(&path);
    }

    fn remove_session_by_ref(&mut self, actor: &ActorRef) {
        let path = actor.path().to_string();
        if let Some(entry) = self.sessions.remove(&path) {
            for hash in entry.session.inv_tasks.keys() {
                self.decrement_inv_task(hash);
            }
            for index in entry.session.index_tasks.keys() {
                self.decrement_index_task(*index);
            }
        }
    }

    fn is_known_hash(&self, hash: &UInt256) -> bool {
        self.known_hashes.contains(hash)
    }

    fn record_known_hash(&mut self, hash: UInt256) {
        if self.known_hashes.insert(hash) {
            self.known_hash_order.push_back(hash);
        }

        while self.known_hash_order.len() > self.known_hash_capacity {
            if let Some(evicted) = self.known_hash_order.pop_front() {
                self.known_hashes.remove(&evicted);
            } else {
                break;
            }
        }
    }

    fn request_tasks_for_path(&mut self, path: &str) {
        self.with_session_mut(path, |entry, this| {
            this.request_tasks_entry(entry);
        });
    }

    fn request_tasks_all(&mut self) {
        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        for path in session_paths {
            self.with_session_mut(&path, |entry, this| {
                this.request_tasks_entry(entry);
            });
        }
    }

    fn request_tasks_entry(&mut self, entry: &mut SessionEntry) {
        if entry.session.has_too_many_tasks() {
            return;
        }

        let system = match &self.system {
            Some(context) => Arc::clone(context),
            None => return,
        };

        let actor = entry.actor.clone();
        let session = &mut entry.session;

        let store_cache = system.store_cache();
        let ledger_contract = LedgerContract::new();

        if !session.available_tasks.is_empty() {
            session
                .available_tasks
                .retain(|hash| !self.known_hashes.contains(hash));
            session
                .available_tasks
                .retain(|hash| !ledger_contract.contains_block(&store_cache, hash));

            let candidates: Vec<UInt256> = session.available_tasks.iter().copied().collect();
            let mut scheduled = Vec::with_capacity(candidates.len());
            let mut to_remove = Vec::with_capacity(candidates.len());
            for hash in candidates {
                if self.increment_inv_task(hash) {
                    session.register_inv_task(hash);
                    scheduled.push(hash);
                    to_remove.push(hash);
                }
            }
            for hash in to_remove {
                session.available_tasks.remove(&hash);
            }

            if !scheduled.is_empty() {
                for group in InvPayload::create_group(InventoryType::Block, scheduled.clone()) {
                    let message = NetworkMessage::new(ProtocolMessage::GetData(group));
                    if let Err(error) = actor.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            actor = %actor.path(),
                            %error,
                            "failed to request available tasks from peer"
                        );
                    }
                }
                return;
            }
        }

        let current_height = ledger_contract
            .current_index(&store_cache)
            .unwrap_or(0)
            .max(self.last_seen_persisted_index);

        let ledger = system.ledger();
        let header_cache = system.header_cache();
        let mut header_height = header_cache
            .last()
            .map(|header| header.index())
            .unwrap_or_else(|| ledger.highest_header_index());
        header_height = header_height.max(ledger.highest_header_index());

        if (!self.has_header_task()
            || self
                .global_inv_tasks
                .get(&HEADER_TASK_HASH)
                .copied()
                .unwrap_or(0)
                < MAX_CONCURRENT_TASKS)
            && header_height < session.last_block_index
            && self.increment_inv_task(HEADER_TASK_HASH)
        {
            session.register_inv_task(HEADER_TASK_HASH);
            let payload = GetBlockByIndexPayload::create(header_height + 1, HEADER_PREFETCH_COUNT);
            let message = NetworkMessage::new(ProtocolMessage::GetHeaders(payload));
            if let Err(error) = actor.tell(RemoteNodeCommand::Send(message)) {
                warn!(
                    target: "neo",
                    actor = %actor.path(),
                    %error,
                    "failed to request headers from peer"
                );
                self.decrement_inv_task(&HEADER_TASK_HASH);
                session.complete_inv_task(&HEADER_TASK_HASH);
            }
            return;
        }

        if current_height < session.last_block_index {
            let mut start_height = current_height + 1;
            while self.global_index_tasks.contains_key(&start_height)
                || session.received_block.contains_key(&start_height)
            {
                start_height = start_height.saturating_add(1);
                if start_height > session.last_block_index {
                    break;
                }
            }

            let limit_height = current_height.saturating_add(MAX_HASHES_COUNT as u32);

            if start_height <= session.last_block_index && start_height < limit_height {
                let mut end_height = start_height;
                while end_height < session.last_block_index
                    && end_height + 1 < limit_height
                    && !self.global_index_tasks.contains_key(&(end_height + 1))
                    && !session.received_block.contains_key(&(end_height + 1))
                {
                    end_height += 1;
                }

                let count = (end_height - start_height + 1).min(MAX_HASHES_COUNT as u32);
                let mut granted = 0u32;
                for offset in 0..count {
                    let index = start_height + offset;
                    if self.increment_index_task(index) {
                        session.register_index_task(index);
                        granted += 1;
                    }
                }

                if granted > 0 {
                    let payload = GetBlockByIndexPayload::create(start_height, granted as i16);
                    let message = NetworkMessage::new(ProtocolMessage::GetBlockByIndex(payload));
                    if let Err(error) = actor.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            actor = %actor.path(),
                            %error,
                            "failed to request blocks by index from peer"
                        );
                        for offset in 0..granted {
                            let index = start_height + offset;
                            self.decrement_index_task(index);
                            session.complete_index_task(index);
                        }
                    }
                    return;
                }
            }
        }

        if !session.mempool_sent {
            session.mempool_sent = true;
            if let Err(error) = actor.tell(RemoteNodeCommand::Send(NetworkMessage::new(
                ProtocolMessage::Mempool,
            ))) {
                warn!(
                    target: "neo",
                    actor = %actor.path(),
                    %error,
                    "failed to request mempool from peer"
                );
            }
        }
    }

    fn with_session_mut<F>(&mut self, path: &str, mut f: F)
    where
        F: FnMut(&mut SessionEntry, &mut TaskManager),
    {
        if let Some(mut entry) = self.sessions.remove(path) {
            f(&mut entry, self);
            self.sessions.insert(path.to_string(), entry);
        }
    }

    fn on_headers(&mut self, actor: &ActorRef) {
        let path = actor.path().to_string();
        if !self.sessions.contains_key(&path) {
            trace!(target: "neo", actor = %path, "ignoring headers from unknown session");
            return;
        }

        self.with_session_mut(&path, |entry, this| {
            if entry.session.complete_inv_task(&HEADER_TASK_HASH) {
                this.decrement_inv_task(&HEADER_TASK_HASH);
            }
            this.request_tasks_entry(entry);
        });
    }

    fn on_new_tasks(&mut self, actor: &ActorRef, payload: InvPayload) {
        let path = actor.path().to_string();
        if payload.is_empty() {
            return;
        }

        if !self.sessions.contains_key(&path) {
            trace!(target: "neo", actor = %path, "ignoring NewTasks from unknown session");
            return;
        }

        let system = match &self.system {
            Some(context) => Arc::clone(context),
            None => return,
        };

        let store_cache = system.store_cache();
        let ledger_contract = LedgerContract::new();
        let current_height = ledger_contract
            .current_index(&store_cache)
            .unwrap_or(0)
            .max(self.last_seen_persisted_index);
        let ledger = system.ledger();
        let header_cache = system.header_cache();
        let mut header_height = header_cache
            .last()
            .map(|header| header.index())
            .unwrap_or_else(|| ledger.highest_header_index());
        header_height = header_height.max(ledger.highest_header_index());
        drop(store_cache);

        let inventory_type = payload.inventory_type;
        let hashes = payload.hashes.clone();
        let actor_ref = actor.clone();
        self.with_session_mut(&path, move |entry, this| {
            if current_height < header_height
                && (inventory_type == InventoryType::Transaction
                    || (inventory_type == InventoryType::Block
                        && current_height
                            < entry
                                .session
                                .last_block_index
                                .saturating_sub(MAX_HASHES_COUNT as u32)))
            {
                this.request_tasks_entry(entry);
                return;
            }

            let mut pending = Vec::with_capacity(hashes.len());
            for hash in hashes.iter().copied() {
                if this.is_known_hash(&hash) {
                    continue;
                }

                if inventory_type == InventoryType::Block
                    && this.global_inv_tasks.contains_key(&hash)
                {
                    entry.session.available_tasks.insert(hash);
                }

                if this.global_inv_tasks.contains_key(&hash) {
                    continue;
                }

                pending.push(hash);
            }

            if pending.is_empty() {
                this.request_tasks_entry(entry);
                return;
            }

            let mut scheduled = Vec::with_capacity(pending.len());
            for hash in pending.into_iter() {
                if this.increment_inv_task(hash) {
                    entry.session.register_inv_task(hash);
                    scheduled.push(hash);
                }
            }

            if !scheduled.is_empty() {
                for group in InvPayload::create_group(inventory_type, scheduled.clone()) {
                    let message = NetworkMessage::new(ProtocolMessage::GetData(group));
                    if let Err(error) = actor_ref.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            actor = %actor_ref.path(),
                            %error,
                            "failed to request inventory data from peer"
                        );
                    }
                }
            } else {
                this.request_tasks_entry(entry);
            }
        });

        self.request_tasks_for_path(&path);
    }

    fn on_restart_tasks(&mut self, actor: &ActorRef, payload: InvPayload) {
        let path = actor.path().to_string();
        if !self.sessions.contains_key(&path) {
            trace!(target: "neo", actor = %path, "ignoring RestartTasks from unknown session");
            return;
        }

        let actor_ref = actor.clone();
        let inventory_type = payload.inventory_type;
        let hashes: Vec<UInt256> = payload.hashes.clone();
        self.with_session_mut(&path, move |entry, this| {
            let mut scheduled = Vec::with_capacity(hashes.len());
            for hash in hashes.iter().copied() {
                this.forget_hash(&hash);
                this.decrement_inv_task(&hash);
                if this.increment_inv_task(hash) {
                    entry.session.register_inv_task(hash);
                    scheduled.push(hash);
                }
            }

            if !scheduled.is_empty() {
                for group in InvPayload::create_group(inventory_type, scheduled.clone()) {
                    let message = NetworkMessage::new(ProtocolMessage::GetData(group));
                    if let Err(error) = actor_ref.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            actor = %actor_ref.path(),
                            %error,
                            "failed to restart inventory fetch from peer"
                        );
                    }
                }
            }
        });

        self.request_tasks_for_path(&path);
    }

    fn complete_inventory(
        &mut self,
        actor: &ActorRef,
        hash: UInt256,
        block: Option<Block>,
        block_index: Option<u32>,
    ) {
        if hash != HEADER_TASK_HASH {
            self.record_known_hash(hash);
        }
        self.remove_available_task_from_all(&hash);

        let path = actor.path().to_string();
        let Some(mut entry) = self.sessions.remove(&path) else {
            trace!(target: "neo", actor = %path, "inventory completion for unknown session");
            return;
        };

        let actor_ref = entry.actor.clone();

        if entry.session.complete_inv_task(&hash) {
            self.decrement_inv_task(&hash);
        }

        let mut index_to_release = block_index;
        if let Some(ref block_ref) = block {
            index_to_release = Some(block_ref.index());
        }

        if let Some(index) = index_to_release {
            if entry.session.complete_index_task(index) {
                self.decrement_index_task(index);
            }
        }

        let mut should_request = true;
        let mut disconnect_peer = false;

        if let Some(block_payload) = block {
            should_request = false;
            let mut block_payload = block_payload;
            let index = block_payload.index();
            let incoming_hash = block_payload.hash();
            let conflict = entry
                .session
                .received_block
                .get(&index)
                .map(|existing| {
                    let mut existing_clone = existing.clone();
                    existing_clone.hash() != incoming_hash
                })
                .unwrap_or(false);

            if conflict {
                disconnect_peer = true;
            } else if !entry.session.received_block.contains_key(&index) {
                entry.session.store_received_block(index, block_payload);
            }
        }

        if hash == HEADER_TASK_HASH {
            entry.session.available_tasks.remove(&hash);
        }

        self.sessions.insert(path.clone(), entry);

        if disconnect_peer {
            if let Err(error) = actor_ref.tell(RemoteNodeCommand::Disconnect {
                reason: "conflicting block received".to_string(),
            }) {
                warn!(
                    target: "neo",
                    actor = %actor_ref.path(),
                    %error,
                    "failed to disconnect peer after conflicting block"
                );
            }
            return;
        }

        if should_request {
            self.request_tasks_for_path(&path);
        }
    }

    fn on_persist_completed(&mut self, block: &Block) {
        self.last_seen_persisted_index = block.index();
        let index = block.index();
        let mut persisted = block.clone();
        let hash = persisted.hash();

        let session_count = self.sessions.len();
        let mut to_disconnect = Vec::with_capacity(session_count);
        let mut to_request = Vec::with_capacity(session_count);

        for (path, entry) in self.sessions.iter_mut() {
            if let Some(stored) = entry.session.received_block.remove(&index) {
                let mut stored = stored;
                if stored.hash() == hash {
                    to_request.push(path.clone());
                } else {
                    to_disconnect.push(entry.actor.clone());
                }
            }
        }

        for actor in to_disconnect {
            if let Err(error) = actor.tell(RemoteNodeCommand::Disconnect {
                reason: "persisted block hash mismatch".to_string(),
            }) {
                warn!(
                    target: "neo",
                    actor = %actor.path(),
                    %error,
                    "failed to disconnect peer after mismatched block persistence"
                );
            }
        }

        for path in to_request {
            self.request_tasks_for_path(&path);
        }
    }

    fn on_relay_result(&mut self, result: &RelayResult) {
        if result.result != VerifyResult::Invalid || result.inventory_type != InventoryType::Block {
            return;
        }

        self.on_invalid_block(&result.hash, result.block_index);
    }

    fn on_invalid_block(&mut self, hash: &UInt256, block_index: Option<u32>) {
        let mut offenders = Vec::with_capacity(self.sessions.len());

        for entry in self.sessions.values() {
            let mut matches = false;
            if let Some(index) = block_index
                && let Some(stored) = entry.session.received_block.get(&index)
            {
                let mut candidate = stored.clone();
                matches = candidate.hash() == *hash;
            } else {
                for stored in entry.session.received_block.values() {
                    let mut candidate = stored.clone();
                    if candidate.hash() == *hash {
                        matches = true;
                        break;
                    }
                }
            }

            if matches {
                offenders.push(entry.actor.clone());
            }
        }

        for actor in offenders {
            if let Err(error) = actor.tell(RemoteNodeCommand::Disconnect {
                reason: "invalid block relayed".to_string(),
            }) {
                warn!(
                    target: "neo",
                    actor = %actor.path(),
                    %error,
                    "failed to disconnect peer carrying invalid block"
                );
            }
        }

        self.request_tasks_all();
    }

    fn prune_timeouts(&mut self) {
        let timeout = self.task_timeout;
        let session_paths: Vec<String> = self.sessions.keys().cloned().collect();
        for path in session_paths {
            self.with_session_mut(&path, |entry, this| {
                let expired = entry.session.prune_timed_out_inv_tasks(timeout);
                for hash in expired {
                    this.decrement_inv_task(&hash);
                }

                let expired_indexes = entry.session.prune_timed_out_index_tasks(timeout);
                for index in expired_indexes {
                    this.decrement_index_task(index);
                }
            });
        }
        self.request_tasks_all();
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Akka actor wrapper around [`TaskManager`].
pub struct TaskManagerActor {
    state: TaskManager,
    timer: Option<Cancelable>,
}

impl TaskManagerActor {
    pub fn new(state: TaskManager) -> Self {
        Self { state, timer: None }
    }

    fn schedule_timer(&mut self, ctx: &mut ActorContext) {
        if self.timer.is_some() {
            return;
        }
        let cancelable = ctx.schedule_tell_repeatedly_cancelable(
            self.state.timer_interval,
            self.state.timer_interval,
            &ctx.self_ref(),
            TaskManagerCommand::TimerTick,
            None,
        );
        self.timer = Some(cancelable);
    }

    fn cancel_timer(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
    }
}

impl Default for TaskManagerActor {
    fn default() -> Self {
        Self::new(TaskManager::new())
    }
}

#[async_trait]
impl Actor for TaskManagerActor {
    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        let message_type_id = envelope.as_ref().type_id();

        match envelope.downcast::<TaskManagerCommand>() {
            Ok(command) => {
                match *command {
                    TaskManagerCommand::AttachSystem { context } => {
                        self.state.attach_system(context, ctx);
                        self.schedule_timer(ctx);
                    }
                    TaskManagerCommand::Register { version } => match ctx.sender() {
                        Some(sender) => {
                            self.state.register_session(sender, version, ctx);
                        }
                        _ => {
                            warn!(target: "neo", "register command without sender");
                        }
                    },
                    TaskManagerCommand::Update { last_block_index } => {
                        if let Some(sender) = ctx.sender() {
                            self.state.update_session(&sender, last_block_index);
                        }
                    }
                    TaskManagerCommand::NewTasks { payload } => {
                        if let Some(sender) = ctx.sender() {
                            self.state.on_new_tasks(&sender, payload);
                        }
                    }
                    TaskManagerCommand::RestartTasks { payload } => {
                        if let Some(sender) = ctx.sender() {
                            self.state.on_restart_tasks(&sender, payload);
                        }
                    }
                    TaskManagerCommand::InventoryCompleted {
                        hash,
                        block,
                        block_index,
                    } => {
                        if let Some(sender) = ctx.sender() {
                            self.state
                                .complete_inventory(&sender, hash, *block, block_index);
                        }
                    }
                    TaskManagerCommand::Headers { .. } => {
                        if let Some(sender) = ctx.sender() {
                            self.state.on_headers(&sender);
                        }
                    }
                    TaskManagerCommand::TimerTick => {
                        self.state.prune_timeouts();
                    }
                }
                Ok(())
            }
            Err(envelope) => match envelope.downcast::<PersistCompleted>() {
                Ok(persist) => {
                    self.state.on_persist_completed(&persist.block);
                    Ok(())
                }
                Err(envelope) => match envelope.downcast::<RelayResult>() {
                    Ok(result) => {
                        self.state.on_relay_result(&result);
                        Ok(())
                    }
                    Err(envelope) => match envelope.downcast::<Terminated>() {
                        Ok(terminated) => {
                            self.state.remove_session_by_ref(&terminated.actor);
                            Ok(())
                        }
                        Err(other) => {
                            warn!(
                                target: "neo",
                                message_type_id = ?message_type_id,
                                "unknown message routed to task manager actor"
                            );
                            drop(other);
                            Ok(())
                        }
                    },
                },
            },
        }
    }

    async fn post_stop(&mut self, ctx: &mut ActorContext) -> ActorResult {
        self.cancel_timer();
        if let Some(stream) = self.state.event_stream.take() {
            stream.unsubscribe_all(&ctx.self_ref());
        }
        Ok(())
    }
}

/// Message variants handled by [`TaskManagerActor`].
#[derive(Debug, Clone)]
pub enum TaskManagerCommand {
    AttachSystem {
        context: Arc<NeoSystemContext>,
    },
    Register {
        version: VersionPayload,
    },
    Update {
        last_block_index: u32,
    },
    NewTasks {
        payload: InvPayload,
    },
    RestartTasks {
        payload: InvPayload,
    },
    InventoryCompleted {
        hash: UInt256,
        block: Box<Option<Block>>,
        block_index: Option<u32>,
    },
    Headers {
        headers: Vec<Header>,
    },
    TimerTick,
}
