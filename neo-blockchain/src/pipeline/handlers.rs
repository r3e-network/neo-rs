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
use neo_payloads::InventoryType;
use neo_primitives::VerifyResult;

#[path = "../handlers/mod.rs"]
mod service_handlers;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::RelayResult`] notification.
    pub(crate) async fn handle_relay_result(&self, result: RelayResult) {
        if result.inventory_type == InventoryType::Extensible
            && result.result != VerifyResult::Succeed
        {
            // C# Neo v3.10.1: invalid ExtensiblePayload relay results are
            // returned to the sender but are not published to the event stream.
            return;
        }

        let _ = self.event_tx.send(crate::RuntimeEvent::RelayResult {
            hash: result.hash,
            inventory_type: result.inventory_type,
            block_index: result.block_index,
            result: result.result,
        });
    }
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
#[path = "../tests/pipeline/handlers.rs"]
mod tests;
