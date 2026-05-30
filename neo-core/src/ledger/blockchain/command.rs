//! Commands accepted by the Blockchain actor.

use super::*;
use std::sync::Arc;

/// Commands accepted by the Blockchain actor.
#[derive(Debug, Clone)]
pub enum BlockchainCommand {
    /// Notification that a block was persisted.
    PersistCompleted(PersistCompleted),
    /// Request to import blocks.
    Import(Import),
    /// Request to fill the memory pool.
    FillMemoryPool(FillMemoryPool),
    /// Notification that fill completed.
    FillCompleted,
    /// Request to reverify inventories.
    Reverify(Reverify),
    /// Inventory block received.
    InventoryBlock {
        /// The block.
        block: Arc<Block>,
        /// Whether to relay.
        relay: bool,
        /// Whether state-independent verification (signatures) was already performed.
        pre_verified: bool,
    },
    /// Extensible payload received.
    InventoryExtensible {
        /// The extensible payload.
        payload: ExtensiblePayload,
        /// Whether to relay.
        relay: bool,
    },
    /// Preverification completed.
    PreverifyCompleted(PreverifyCompleted),
    /// Headers received.
    Headers(Vec<Header>),
    /// Idle tick for background processing.
    Idle,
    /// Relay result notification.
    RelayResult(RelayResult),
    /// Initialize the blockchain actor.
    Initialize,
    /// Check unverified cache and persist any ready consecutive blocks.
    /// Self-scheduled by the actor when blocks are parked in the unverified cache
    /// to ensure persistence continues even when the specific InventoryBlock message
    /// for the next-to-persist block is delayed in the mailbox.
    DrainUnverified,
    /// Attach the system context.
    AttachSystem(Arc<NeoSystemContext>),
}
