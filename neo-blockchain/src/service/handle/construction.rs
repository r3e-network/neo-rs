//! Blockchain handle construction helpers.
//!
//! Construction is kept separate from command-category APIs so the handle root
//! can define the shared channel fields while child modules own how callers
//! obtain or use those channels.

use tokio::sync::{broadcast, mpsc};

use super::BlockchainHandle;
use crate::command::BlockchainCommand;

impl BlockchainHandle {
    /// Build a `(handle, command-receiver, event-sender)` triple.
    ///
    /// The caller is expected to spawn the blockchain command loop on the
    /// returned `mpsc::Receiver`, and to use the returned `broadcast::Sender`
    /// (or hand it to the loop) to publish events. Most callers should prefer
    /// [`BlockchainHandle::with_capacity`] when they do not need to drive the
    /// loop themselves.
    pub fn channel(
        cmd_capacity: usize,
        event_capacity: usize,
    ) -> (
        Self,
        mpsc::Receiver<BlockchainCommand>,
        broadcast::Sender<crate::RuntimeEvent>,
    ) {
        let (cmd_tx, cmd_rx) = mpsc::channel(cmd_capacity);
        let (event_tx, _event_rx) = broadcast::channel(event_capacity);
        let handle = Self {
            cmd_tx,
            event_tx: event_tx.clone(),
        };
        (handle, cmd_rx, event_tx)
    }

    /// Build a [`BlockchainHandle`] with default capacities and return the
    /// command receiver that the caller's blockchain loop should drive.
    pub fn with_capacity() -> (Self, mpsc::Receiver<BlockchainCommand>) {
        let (handle, cmd_rx, _event_tx) = Self::channel(
            crate::blockchain::DEFAULT_COMMAND_CAPACITY,
            crate::blockchain::DEFAULT_EVENT_CAPACITY,
        );
        (handle, cmd_rx)
    }
}
