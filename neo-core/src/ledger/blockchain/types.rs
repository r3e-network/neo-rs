//
// types.rs - Message types and enums for Blockchain actor
//

use super::*;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct UnverifiedBlocksList {
    pub(super) blocks: Vec<Block>,
    nodes: HashSet<String>,
}

impl UnverifiedBlocksList {
    #[allow(dead_code)]
    pub(super) fn new() -> Self {
        Self {
            blocks: Vec::new(),
            nodes: HashSet::new(),
        }
    }
}

/// Notification that a block has been persisted to storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistCompleted {
    /// The block that was persisted.
    pub block: Block,
}

/// Request to import blocks into the blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    /// Blocks to import.
    pub blocks: Vec<Block>,
    /// Whether to verify blocks before importing.
    pub verify: bool,
}

impl Default for Import {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            verify: true,
        }
    }
}

/// Notification that block import has completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCompleted;

/// Request to fill the memory pool with transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillMemoryPool {
    /// Transactions to add to the memory pool.
    pub transactions: Vec<Transaction>,
}

/// Notification that memory pool fill has completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillCompleted;

/// Inventory payload types for relay and verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InventoryPayload {
    /// A block payload.
    Block(Box<Block>),
    /// A transaction payload.
    Transaction(Box<Transaction>),
    /// An extensible payload.
    Extensible(Box<ExtensiblePayload>),
    /// Raw inventory data with type.
    Raw(InventoryType, Vec<u8>),
}

/// Item to be reverified in the blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverifyItem {
    /// The inventory payload to reverify.
    pub payload: InventoryPayload,
    /// Optional block index context.
    #[serde(default)]
    pub block_index: Option<u32>,
}

/// Request to reverify inventory items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reverify {
    /// Items to reverify.
    pub inventories: Vec<ReverifyItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImportDisposition {
    AlreadySeen,
    NextExpected,
    FutureGap,
}

pub(super) fn classify_import_block(current_height: u32, block_index: u32) -> ImportDisposition {
    if block_index <= current_height {
        ImportDisposition::AlreadySeen
    } else if block_index == current_height.saturating_add(1) {
        ImportDisposition::NextExpected
    } else {
        ImportDisposition::FutureGap
    }
}

#[cfg(test)]
pub(super) fn should_schedule_reverify_idle(more_pending: bool, header_backlog: bool) -> bool {
    more_pending && !header_backlog
}

pub use crate::ledger::transaction_router::PreverifyCompleted;

/// Result of a relay operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResult {
    /// Hash of the relayed inventory.
    pub hash: UInt256,
    /// Type of inventory that was relayed.
    pub inventory_type: InventoryType,
    /// Optional block index context.
    pub block_index: Option<u32>,
    /// Verification result.
    pub result: VerifyResult,
}

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
        block: Block,
        /// Whether to relay.
        relay: bool,
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
    /// Attach the system context.
    AttachSystem(Arc<NeoSystemContext>),
}
