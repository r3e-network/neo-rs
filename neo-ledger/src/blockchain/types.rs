//! Blockchain command types and relay results.

use neo_primitives::{UInt256, VerifyResult};
use neo_protocol::execution::Block;
use std::sync::Arc;

// Re-export RelayResult from primitives
pub use neo_primitives::RelayResult;

/// Result of classifying an incoming block relative to current height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportDisposition {
    /// Block already seen (height <= current).
    AlreadySeen,
    /// Block is the next expected (height == current + 1).
    NextExpected,
    /// Block is from the future (height > current + 1).
    FutureGap,
}

/// Classifies an incoming block based on its index relative to current blockchain height.
pub fn classify_import_block(block_index: u32, current_height: u32) -> ImportDisposition {
    if block_index <= current_height {
        ImportDisposition::AlreadySeen
    } else if block_index == current_height + 1 {
        ImportDisposition::NextExpected
    } else {
        ImportDisposition::FutureGap
    }
}

/// List of unverified blocks pending import.
pub type UnverifiedBlocksList = Vec<Arc<Block>>;

/// Event emitted after a block has been persisted successfully.
#[derive(Debug, Clone)]
pub struct PersistCompleted {
    pub block: Arc<Block>,
}

/// Event emitted when block persistence fails.
#[derive(Debug, Clone)]
pub struct PersistFailed {
    pub block: Arc<Block>,
    pub error: String,
}

/// Blockchain command sent to the blockchain actor.
#[derive(Debug, Clone)]
pub enum BlockchainCommand {
    /// Import a block.
    Import(Import),
    /// Fill the memory pool.
    FillMemoryPool(FillMemoryPool),
    /// Reverify an inventory item.
    Reverify(Reverify),
}

/// Import block command.
#[derive(Debug, Clone)]
pub struct Import {
    pub block: Arc<Block>,
}

/// Import completed event.
#[derive(Debug, Clone)]
pub struct ImportCompleted {
    pub block: Arc<Block>,
}

/// Fill memory pool command.
#[derive(Debug, Clone)]
pub struct FillMemoryPool {
    pub transactions: Vec<UInt256>,
}

/// Fill completed event.
#[derive(Debug, Clone)]
pub struct FillCompleted;

/// Reverify inventory command.
#[derive(Debug, Clone)]
pub struct Reverify {
    pub items: Vec<ReverifyItem>,
}

/// Item to reverify.
#[derive(Debug, Clone)]
pub struct ReverifyItem {
    pub hash: UInt256,
}

/// Preverify completed event.
#[derive(Debug, Clone)]
pub struct PreverifyCompleted {
    pub hash: UInt256,
    pub result: VerifyResult,
}

/// Blockchain actor handle (placeholder for actual actor handle).
#[derive(Clone)]
pub struct BlockchainHandle;

/// Inventory payload wrapper.
#[derive(Debug, Clone)]
pub enum InventoryPayload {
    Block(Arc<Block>),
}

#[cfg(test)]
pub fn should_schedule_reverify_idle(
    _reverify_cache_len: usize,
    _max_cache_size: usize,
) -> bool {
    true
}
