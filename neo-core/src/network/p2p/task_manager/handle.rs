use super::TaskManagerCommand;
use crate::UInt256;
use crate::services::SystemContext;
use crate::network::p2p::payloads::{VersionPayload, block::Block, inv_payload::InvPayload};
use crate::runtime::{ActorRef, ActorRuntimeResult};
use std::sync::Arc;

/// Typed facade for sending commands to the task manager actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskManagerHandle {
    raw: ActorRef,
}

impl TaskManagerHandle {
    /// Wraps a raw actor reference with the task manager command boundary.
    pub fn new(raw: ActorRef) -> Self {
        Self { raw }
    }

    /// Returns the raw actor reference for watcher/runtime integration.
    pub fn raw_ref(&self) -> &ActorRef {
        &self.raw
    }

    /// Sends a raw task manager command.
    pub fn tell(&self, command: TaskManagerCommand) -> ActorRuntimeResult<()> {
        self.raw.tell(command)
    }

    /// Attaches the shared system context to the task manager.
    pub fn attach_system(&self, context: Arc<dyn SystemContext>) -> ActorRuntimeResult<()> {
        self.tell(TaskManagerCommand::AttachSystem { context })
    }

    /// Registers a peer session.
    pub fn register_peer(&self, peer: ActorRef, version: VersionPayload) -> ActorRuntimeResult<()> {
        self.tell(TaskManagerCommand::Register { peer, version })
    }

    /// Updates a peer's last advertised block index.
    pub fn update_peer(&self, peer: ActorRef, last_block_index: u32) -> ActorRuntimeResult<()> {
        self.tell(TaskManagerCommand::Update {
            peer,
            last_block_index,
        })
    }

    /// Announces new inventory tasks from a peer.
    pub fn new_tasks(&self, peer: ActorRef, payload: InvPayload) -> ActorRuntimeResult<()> {
        self.tell(TaskManagerCommand::NewTasks { peer, payload })
    }

    /// Restarts inventory tasks for a specific peer.
    pub fn restart_tasks(&self, peer: ActorRef, payload: InvPayload) -> ActorRuntimeResult<()> {
        self.tell(TaskManagerCommand::RestartTasks { peer, payload })
    }

    /// Broadcasts inventory restart requests across registered peer sessions.
    pub fn broadcast_restart_tasks(&self, payload: InvPayload) -> ActorRuntimeResult<()> {
        self.tell(TaskManagerCommand::BroadcastRestartTasks { payload })
    }

    /// Marks a peer-delivered inventory item as completed.
    pub fn inventory_completed(
        &self,
        peer: ActorRef,
        hash: UInt256,
        block: Option<Block>,
        block_index: Option<u32>,
    ) -> ActorRuntimeResult<()> {
        self.tell(TaskManagerCommand::InventoryCompleted {
            peer,
            hash,
            block: Box::new(block),
            block_index,
        })
    }

    /// Records that a peer delivered headers.
    pub fn headers(&self, peer: ActorRef) -> ActorRuntimeResult<()> {
        self.tell(TaskManagerCommand::Headers { peer })
    }
}

impl From<ActorRef> for TaskManagerHandle {
    fn from(raw: ActorRef) -> Self {
        Self::new(raw)
    }
}
