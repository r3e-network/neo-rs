use super::BlockchainCommand;
use crate::runtime::{ActorRef, ActorRuntimeResult};

/// Typed facade for sending commands to the blockchain actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockchainHandle {
    raw: ActorRef,
}

impl BlockchainHandle {
    /// Wraps a raw actor reference with the blockchain command boundary.
    pub fn new(raw: ActorRef) -> Self {
        Self { raw }
    }

    /// Returns the raw actor reference for watcher/runtime integration.
    pub fn raw_ref(&self) -> &ActorRef {
        &self.raw
    }

    /// Sends a blockchain command without an actor sender.
    pub fn tell(&self, command: BlockchainCommand) -> ActorRuntimeResult<()> {
        self.raw.tell(command)
    }

    /// Sends a blockchain command with an optional actor sender.
    pub fn tell_from(
        &self,
        command: BlockchainCommand,
        sender: Option<ActorRef>,
    ) -> ActorRuntimeResult<()> {
        self.raw.tell_from(command, sender)
    }

    /// Sends a blockchain command with mailbox backpressure.
    pub async fn tell_async(&self, command: BlockchainCommand) -> ActorRuntimeResult<()> {
        self.raw.tell_async(command).await
    }

    /// Sends a blockchain command with an optional sender and mailbox backpressure.
    pub async fn tell_from_async(
        &self,
        command: BlockchainCommand,
        sender: Option<ActorRef>,
    ) -> ActorRuntimeResult<()> {
        self.raw.tell_from_async(command, sender).await
    }
}

impl From<ActorRef> for BlockchainHandle {
    fn from(raw: ActorRef) -> Self {
        Self::new(raw)
    }
}
