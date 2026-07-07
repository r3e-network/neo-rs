//! Blockchain handle event subscription.
//!
//! Event subscriptions are read-only views over the service broadcast channel.
//! Keeping this separate from construction and command APIs makes the handle's
//! event surface explicit for consensus, indexers, RPC bridges, and shutdown
//! waiters.

use tokio::sync::broadcast;

use super::BlockchainHandle;

impl BlockchainHandle {
    /// Subscribe to [`crate::RuntimeEvent`]s.
    ///
    /// Each call returns an independent receiver; dropping the receiver
    /// automatically unregisters the subscription. The broadcast queue is sized
    /// at construction time via [`Self::channel`].
    pub fn subscribe(&self) -> broadcast::Receiver<crate::RuntimeEvent> {
        self.event_tx.subscribe()
    }
}
