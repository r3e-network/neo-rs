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
//!    [`BlockchainHandle::initialize`], [`BlockchainHandle::shutdown`]): send
//!    one-way service work without exposing [`crate::BlockchainCommand`] to the
//!    caller.
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

use neo_runtime::Service;
use tokio::sync::{broadcast, mpsc};

use crate::command::BlockchainCommand;

mod construction;
mod events;
mod imports;
mod inventory;
mod lifecycle;
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

impl Service for BlockchainHandle {
    fn name(&self) -> &str {
        "BlockchainHandle"
    }
}
