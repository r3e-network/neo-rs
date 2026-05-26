//! Task manager actor: tracks peer sessions, inventory requests, and retries.
//!
//! The actor-facing state stays in this module. Actor-independent decisions and
//! flow helpers are split into focused child modules:
//!
//! - `scheduling`: header and block-index request planning.
//! - `request_flow`: request dispatch and peer task selection.
//! - `completion_flow`: inventory completion and persistence callbacks.
//! - `block_validation`: block/hash consistency checks.
//! - `restart_flow` and `timeout_pruning`: restart and timer cleanup paths.
//! - `state`, `session_lifecycle`, and `peer_commands`: small support modules.
//!
//! This keeps protocol behavior centralized while separating pure scheduling
//! decisions from mailbox delivery.
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

use super::payloads::{VersionPayload, block::Block, inv_payload::InvPayload};
use super::task_session::TaskSession;
use crate::UInt256;
use crate::ledger::{PersistCompleted, RelayResult};
use crate::neo_system::NeoSystemContext;
use crate::runtime::{
    Actor, ActorContext, ActorRef, ActorResult, Cancelable, EventStreamHandle, Props, Terminated,
};
use async_trait::async_trait;
use neo_io_crate::HashSetCache;
use scheduling::TaskCounter;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

mod block_validation;
mod completion_flow;
mod handle;
mod peer_commands;
mod request_flow;
mod restart_flow;
mod scheduling;
mod session_lifecycle;
mod state;
mod timeout_pruning;
use peer_commands::send_mempool;

pub use handle::TaskManagerHandle;

/// Interval for task manager housekeeping.
const TIMER_INTERVAL: Duration = Duration::from_secs(30);
/// Timeout applied to in-flight inventory requests.
const TASK_TIMEOUT: Duration = Duration::from_secs(60);
/// Maximum concurrent sessions that may share the same inventory hash task.
/// Matches C# `MaxConCurrentTasks = 3`.  Keeping this low ensures that only
/// a few peers request headers concurrently; the rest fall through to block
/// index requests, preventing header fetching from starving block sync.
const MAX_CONCURRENT_TASKS: u32 = 3;
/// Maximum number of sequential block heights requested in a single getblkbyidx round.
///
/// Smaller batches reduce head-of-line blocking when a slow peer gets assigned
/// the earliest range needed by persistence.
const MAX_BLOCK_INDEX_BATCH: u32 = 1000;
/// Number of batch windows to keep in flight globally.
///
/// This allows multiple peers to download disjoint height ranges concurrently
/// without giving any single peer an excessively large assignment.
const BLOCK_INDEX_WINDOW_MULTIPLIER: u32 = 10;
const HEADER_TASK_HASH: UInt256 = UInt256::zero();
struct SessionEntry {
    actor: ActorRef,
    session: TaskSession,
}

fn request_mempool_once(actor: &ActorRef, session: &mut TaskSession) -> bool {
    if session.mempool_sent {
        return false;
    }

    session.mempool_sent = true;
    if let Err(error) = send_mempool(actor) {
        warn!(
            target: "neo",
            actor = %actor.path(),
            %error,
            "failed to request mempool from peer"
        );
    }
    true
}

/// Actor-independent state for the task manager.
pub struct TaskManager {
    system: Option<Arc<NeoSystemContext>>,
    sessions: HashMap<String, SessionEntry>,
    known_hashes: HashSetCache<UInt256>,
    event_stream: Option<EventStreamHandle>,
    last_seen_persisted_index: u32,
    global_inv_tasks: TaskCounter<UInt256>,
    global_index_tasks: TaskCounter<u32>,
    timer_interval: Duration,
    task_timeout: Duration,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            system: None,
            sessions: HashMap::with_capacity(32),
            known_hashes: HashSetCache::new(1024),
            event_stream: None,
            last_seen_persisted_index: 0,
            global_inv_tasks: TaskCounter::with_capacity(256),
            global_index_tasks: TaskCounter::with_capacity(256),
            timer_interval: TIMER_INTERVAL,
            task_timeout: TASK_TIMEOUT,
        }
    }

    pub fn props() -> Props {
        Props::new(|| TaskManagerActor::new(Self::new()))
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Actor-runtime wrapper around [`TaskManager`].
pub struct TaskManagerActor {
    state: TaskManager,
    timer: Option<Cancelable>,
}

enum TaskManagerMessage {
    Command(TaskManagerCommand),
    PersistCompleted(PersistCompleted),
    RelayResult(RelayResult),
    Terminated(Terminated),
}

impl TaskManagerMessage {
    fn from_envelope(envelope: Box<dyn Any + Send>) -> Result<Self, Box<dyn Any + Send>> {
        match envelope.downcast::<TaskManagerCommand>() {
            Ok(command) => Ok(Self::Command(*command)),
            Err(envelope) => match envelope.downcast::<PersistCompleted>() {
                Ok(persist) => Ok(Self::PersistCompleted(*persist)),
                Err(envelope) => match envelope.downcast::<RelayResult>() {
                    Ok(result) => Ok(Self::RelayResult(*result)),
                    Err(envelope) => match envelope.downcast::<Terminated>() {
                        Ok(terminated) => Ok(Self::Terminated(*terminated)),
                        Err(envelope) => Err(envelope),
                    },
                },
            },
        }
    }
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

    fn handle_message(&mut self, message: TaskManagerMessage, ctx: &mut ActorContext) {
        match message {
            TaskManagerMessage::Command(command) => match command {
                TaskManagerCommand::AttachSystem { context } => {
                    self.state.attach_system(context, ctx);
                    self.schedule_timer(ctx);
                }
                TaskManagerCommand::Register { peer, version } => {
                    self.state.register_session(peer, version, ctx);
                }
                TaskManagerCommand::Update {
                    peer,
                    last_block_index,
                } => {
                    self.state.update_session(&peer, last_block_index);
                }
                TaskManagerCommand::NewTasks { peer, payload } => {
                    self.state.on_new_tasks(&peer, payload);
                }
                TaskManagerCommand::RestartTasks { peer, payload } => {
                    self.state.on_restart_tasks(&peer, payload);
                }
                TaskManagerCommand::BroadcastRestartTasks { payload } => {
                    self.state.broadcast_restart_tasks(payload);
                }
                TaskManagerCommand::InventoryCompleted {
                    peer,
                    hash,
                    block,
                    block_index,
                } => {
                    self.state
                        .complete_inventory(&peer, hash, *block, block_index);
                }
                TaskManagerCommand::Headers { peer } => {
                    self.state.on_headers(&peer);
                }
                TaskManagerCommand::TimerTick => {
                    self.state.prune_timeouts();
                }
            },
            TaskManagerMessage::PersistCompleted(persist) => {
                self.state.on_persist_completed(&persist.block);
            }
            TaskManagerMessage::RelayResult(result) => {
                self.state.on_relay_result(&result);
            }
            TaskManagerMessage::Terminated(terminated) => {
                self.state.remove_session_by_ref(&terminated.actor);
            }
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

        match TaskManagerMessage::from_envelope(envelope) {
            Ok(message) => self.handle_message(message, ctx),
            Err(other) => {
                warn!(
                    target: "neo",
                    message_type_id = ?message_type_id,
                    "unknown message routed to task manager actor"
                );
                drop(other);
            }
        }
        Ok(())
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
        peer: ActorRef,
        version: VersionPayload,
    },
    Update {
        peer: ActorRef,
        last_block_index: u32,
    },
    NewTasks {
        peer: ActorRef,
        payload: InvPayload,
    },
    RestartTasks {
        peer: ActorRef,
        payload: InvPayload,
    },
    BroadcastRestartTasks {
        payload: InvPayload,
    },
    InventoryCompleted {
        peer: ActorRef,
        hash: UInt256,
        block: Box<Option<Block>>,
        block_index: Option<u32>,
    },
    Headers {
        peer: ActorRef,
    },
    TimerTick,
}

#[cfg(test)]
mod tests;
