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

use super::payloads::{InvPayload, VersionPayload};
use super::task_session::TaskSession;
use crate::neo_system::NeoSystemContext;
use crate::UInt256;
use akka::{Actor, ActorContext, ActorRef, ActorResult, Cancelable, Props, Terminated};
use async_trait::async_trait;
use std::any::{type_name_of_val, Any};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tracing::{trace, warn};

/// Interval for task manager housekeeping (matches C# `TimerInterval`).
const TIMER_INTERVAL: Duration = Duration::from_secs(30);
/// Timeout applied to in-flight inventory requests (matches C# `TaskTimeout`).
const TASK_TIMEOUT: Duration = Duration::from_secs(60);
struct SessionEntry {
    actor: ActorRef,
    session: TaskSession,
}

/// Actor-independent state for the task manager.
pub struct TaskManager {
    system: Option<Arc<NeoSystemContext>>,
    sessions: HashMap<String, SessionEntry>,
    known_hashes: HashSet<UInt256>,
    global_inv_tasks: HashMap<UInt256, u32>,
    global_index_tasks: HashMap<u32, u32>,
    timer_interval: Duration,
    task_timeout: Duration,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            system: None,
            sessions: HashMap::new(),
            known_hashes: HashSet::new(),
            global_inv_tasks: HashMap::new(),
            global_index_tasks: HashMap::new(),
            timer_interval: TIMER_INTERVAL,
            task_timeout: TASK_TIMEOUT,
        }
    }

    pub fn props() -> Props {
        Props::new(|| TaskManagerActor::new(Self::new()))
    }

    fn attach_system(&mut self, context: Arc<NeoSystemContext>) {
        trace!(target: "neo", "task manager attached to system context");
        self.system = Some(context);
        // Once mempool is ported we will size the known-hash cache based on its capacity.
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
        self.sessions.insert(path, SessionEntry { actor, session });
    }

    fn update_session(&mut self, actor: &ActorRef, last_block_index: u32) {
        let path = actor.path().to_string();
        if let Some(entry) = self.sessions.get_mut(&path) {
            entry.session.update_last_block_index(last_block_index);
        }
    }

    fn remove_session_by_ref(&mut self, actor: &ActorRef) {
        let path = actor.path().to_string();
        self.sessions.remove(&path);
    }

    fn on_new_tasks(&mut self, actor: &ActorRef, payload: InvPayload) {
        let path = actor.path().to_string();
        let Some(entry) = self.sessions.get_mut(&path) else {
            trace!(target: "neo", actor = %path, "ignoring NewTasks from unknown session");
            return;
        };

        if payload.is_empty() {
            return;
        }

        // Filter hashes that we already processed globally.
        let mut unique_hashes = Vec::new();
        for hash in payload.hashes.iter().copied() {
            if self.known_hashes.insert(hash) {
                unique_hashes.push(hash);
            }
        }

        if unique_hashes.is_empty() {
            return;
        }

        // Track in session for bookkeeping. The full task scheduling logic will be
        // ported alongside inventory processing.
        for hash in &unique_hashes {
            entry.session.register_inv_task(*hash);
            *self.global_inv_tasks.entry(*hash).or_insert(0) += 1;
        }
    }

    fn on_restart_tasks(&mut self, actor: &ActorRef, payload: InvPayload) {
        let path = actor.path().to_string();
        let Some(entry) = self.sessions.get_mut(&path) else {
            trace!(target: "neo", actor = %path, "ignoring RestartTasks from unknown session");
            return;
        };

        for hash in payload.hashes.iter().copied() {
            entry.session.register_inv_task(hash);
        }
    }

    fn complete_inventory(&mut self, actor: &ActorRef, hash: UInt256) {
        let path = actor.path().to_string();
        let Some(entry) = self.sessions.get_mut(&path) else {
            trace!(target: "neo", actor = %path, "inventory completion for unknown session");
            return;
        };

        if entry.session.complete_inv_task(&hash) {
            if let Some(count) = self.global_inv_tasks.get_mut(&hash) {
                if *count > 0 {
                    *count -= 1;
                }
                if *count == 0 {
                    self.global_inv_tasks.remove(&hash);
                }
            }
        }
    }

    fn prune_timeouts(&mut self) {
        let timeout = self.task_timeout;
        for entry in self.sessions.values_mut() {
            let expired = entry.session.prune_timed_out_inv_tasks(timeout);
            for hash in expired {
                if let Some(count) = self.global_inv_tasks.get_mut(&hash) {
                    if *count > 0 {
                        *count -= 1;
                    }
                    if *count == 0 {
                        self.global_inv_tasks.remove(&hash);
                    }
                }
            }
        }
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
        let message_type_name = type_name_of_val(envelope.as_ref());

        match envelope.downcast::<TaskManagerCommand>() {
            Ok(command) => {
                match *command {
                    TaskManagerCommand::AttachSystem { context } => {
                        self.state.attach_system(context);
                        self.schedule_timer(ctx);
                    }
                    TaskManagerCommand::Register { version } => {
                        if let Some(sender) = ctx.sender() {
                            self.state.register_session(sender, version, ctx);
                        } else {
                            warn!(target: "neo", "register command without sender");
                        }
                    }
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
                    TaskManagerCommand::InventoryCompleted { hash } => {
                        if let Some(sender) = ctx.sender() {
                            self.state.complete_inventory(&sender, hash);
                        }
                    }
                    TaskManagerCommand::TimerTick => {
                        self.state.prune_timeouts();
                    }
                }
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
                        message_type = %message_type_name,
                        "unknown message routed to task manager actor"
                    );
                    drop(other);
                    Ok(())
                }
            },
        }
    }

    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        self.cancel_timer();
        Ok(())
    }
}

/// Message variants handled by [`TaskManagerActor`].
#[derive(Debug, Clone)]
pub enum TaskManagerCommand {
    AttachSystem { context: Arc<NeoSystemContext> },
    Register { version: VersionPayload },
    Update { last_block_index: u32 },
    NewTasks { payload: InvPayload },
    RestartTasks { payload: InvPayload },
    InventoryCompleted { hash: UInt256 },
    TimerTick,
}
