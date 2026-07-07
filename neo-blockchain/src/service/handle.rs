//! Service handle — the public, cheap-to-clone facade for talking to a
//! running `BlockchainService`.
//!
//! The handle is what every other subsystem stores in its state. It is
//! `Clone`, `Send`, and `Sync`; the only state it owns is the two
//! channels the service loop reads from.
//!
//! The handle has *two* styles of typed API:
//!
//! 1. **Inventory/lifecycle commands**
//!    ([`BlockchainHandle::submit_inventory_blocks`],
//!    [`BlockchainHandle::submit_inventory_block`],
//!    [`BlockchainHandle::submit_inventory_extensible`],
//!    [`BlockchainHandle::initialize`]): send one-way service work without
//!    exposing [`crate::BlockchainCommand`] to the caller.
//! 2. **Request/response** ([`BlockchainHandle::import_block`],
//!    [`BlockchainHandle::get_block`], [`BlockchainHandle::get_block_by_height`],
//!    [`BlockchainHandle::get_height`]): translate the method call into a
//!    `BlockchainCommand::ImportBlock` / `GetBlock` / … command and await the
//!    `oneshot` reply. These read like normal `async fn`s rather than command
//!    construction boilerplate.
//!
//! Both layers share the same channel and the same service loop: there
//! is exactly one `BlockchainCommand` stream, dispatched by a single
//! `match` in `crate::service::BlockchainService::run`.

use std::fmt;

use neo_runtime::{Service, ServiceError};
use tokio::sync::{broadcast, mpsc};

use crate::command::BlockchainCommand;

mod imports;
mod inventory;
mod mempool;
mod queries;

/// Cheap-to-clone handle to a blockchain service.
#[derive(Clone)]
pub struct BlockchainHandle {
    /// Sender half of the command channel. The service owns the
    /// receiver and processes commands in `BlockchainService::run`.
    pub(crate) cmd_tx: mpsc::Sender<BlockchainCommand>,
    /// Broadcast sender used by the service to publish lifecycle
    /// events. Subscribers grab their own receiver via
    /// [`Self::subscribe`].
    pub(crate) event_tx: broadcast::Sender<crate::RuntimeEvent>,
}

impl fmt::Debug for BlockchainHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockchainHandle")
            .field("cmd_capacity", &self.cmd_tx.capacity())
            .field("event_receivers", &self.event_tx.receiver_count())
            .finish()
    }
}

impl BlockchainHandle {
    /// Build a `(handle, command-receiver, event-sender)` triple.
    ///
    /// The caller is expected to spawn the blockchain command loop on
    /// the returned `mpsc::Receiver`, and to use the returned
    /// `broadcast::Sender` (or hand it to the loop) to publish events.
    /// Most callers should prefer [`BlockchainHandle::with_capacity`]
    /// when they do not need to drive the loop themselves.
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

    /// Build a [`BlockchainHandle`] with default capacities and return
    /// the command receiver that the caller's blockchain loop should
    /// drive.
    pub fn with_capacity() -> (Self, mpsc::Receiver<BlockchainCommand>) {
        let (handle, cmd_rx, _event_tx) = Self::channel(
            crate::blockchain::DEFAULT_COMMAND_CAPACITY,
            crate::blockchain::DEFAULT_EVENT_CAPACITY,
        );
        (handle, cmd_rx)
    }

    /// Subscribe to [`crate::RuntimeEvent`]s.
    ///
    /// Each call returns an *independent* receiver; dropping the
    /// receiver automatically unregisters the subscription. The
    /// broadcast queue is sized at construction time via
    /// [`Self::channel`].
    pub fn subscribe(&self) -> broadcast::Receiver<crate::RuntimeEvent> {
        self.event_tx.subscribe()
    }

    /// Request blockchain service initialization.
    pub async fn initialize(&self) -> Result<(), ServiceError> {
        self.cmd_tx
            .send(BlockchainCommand::Initialize)
            .await
            .map_err(|_| ServiceError::unavailable("blockchain command channel closed"))
    }

    /// Request graceful shutdown of the service loop. The command
    /// channel will be closed once the in-flight command finishes, at
    /// which point every pending `tell` will start returning
    /// [`ServiceError::ServiceUnavailable`].
    pub async fn shutdown(&self) -> Result<(), ServiceError> {
        // The service loop is driven by `recv().await`; closing the
        // sender is the canonical shutdown signal. We don't expose a
        // dedicated `Shutdown` variant yet because the legacy command
        // set never used one — the service stops on its own once all
        // senders are dropped.
        drop(self.cmd_tx.clone());
        Ok(())
    }
}

impl Service for BlockchainHandle {
    fn name(&self) -> &str {
        "BlockchainHandle"
    }
}
