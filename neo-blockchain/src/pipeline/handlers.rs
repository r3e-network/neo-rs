//! Service method handlers for [`crate::service::BlockchainService`].
//!
//! Each command variant has a corresponding `async fn` method on the
//! service. The dispatch loop in [`crate::service::BlockchainService::dispatch`]
//! just `match`es on the command enum and calls the right method. The service
//! stays explicit: no dynamic downcasting and no per-message trait machinery.
//!
//! The handlers own the service-side Neo protocol decisions: block/header
//! sequencing, native persistence, transaction admission, extensible payload
//! verification, and cache maintenance.

use crate::relay_result::RelayResult;
use crate::service::{BlockchainService, MempoolLike};

#[path = "../handlers/block_inventory.rs"]
mod block_inventory;
#[path = "../handlers/empty_fast_forward.rs"]
mod empty_fast_forward;
#[path = "../handlers/extensible.rs"]
mod extensible;
#[path = "../handlers/headers.rs"]
mod headers;
#[path = "../handlers/import.rs"]
mod import;
#[path = "../handlers/initialize.rs"]
mod initialize;
#[path = "../handlers/persist_completed.rs"]
mod persist_completed;
#[path = "../handlers/reverify.rs"]
mod reverify;
#[path = "../handlers/transactions.rs"]
mod transactions;
#[path = "../handlers/verification.rs"]
mod verification;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::RelayResult`] notification.
    pub(crate) async fn handle_relay_result(&self, _result: RelayResult) {}
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
#[path = "../tests/pipeline/handlers.rs"]
mod tests;
