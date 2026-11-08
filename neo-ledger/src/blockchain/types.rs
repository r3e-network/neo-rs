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
    /// Initialize the blockchain
    Initialize,
    /// Preverify completed event
    PreverifyCompleted(PreverifyCompleted),
    /// Inventory block command
    InventoryBlock {
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    },
    /// Inventory extensible command
    InventoryExtensible {
        payload: neo_protocol::payloads::Extensible,
        relay: bool,
    },
    /// Headers command
    Headers(Vec<Arc<Block>>),
    /// Drain unverified blocks
    DrainUnverified,
    /// Idle reverify command
    Idle,
    /// Persist completed event
    PersistCompleted(PersistCompleted),
    /// Fill completed event
    FillCompleted(FillCompleted),
    /// Relay result
    RelayResult(RelayResult<()>),
}

/// Import block command.
#[derive(Debug, Clone)]
pub struct Import {
    pub block: Arc<Block>,
    pub blocks: Vec<Arc<Block>>,
    pub verify: bool,
}

/// Import completed event.
#[derive(Debug, Clone)]
pub struct ImportCompleted {
    pub block: Arc<Block>,
}

/// Fill memory pool command.
#[derive(Debug, Clone)]
pub struct FillMemoryPool {
    pub transactions: Vec<neo_protocol::payloads::Transaction>,
}

/// Fill completed event.
#[derive(Debug, Clone)]
pub struct FillCompleted;

/// Reverify inventory command.
#[derive(Debug, Clone)]
pub struct Reverify {
    pub items: Vec<ReverifyItem>,
    pub inventories: Vec<ReverifyItem>,
}

/// Item to reverify.
#[derive(Debug, Clone)]
pub struct ReverifyItem {
    pub hash: UInt256,
    pub payload: InventoryPayload,
}

/// Preverify completed event.
#[derive(Debug, Clone)]
pub struct PreverifyCompleted {
    pub hash: UInt256,
    pub result: VerifyResult,
    pub transaction: neo_protocol::payloads::Transaction,
    pub relay: bool,
}

/// Blockchain actor handle (placeholder for actual actor handle).
#[derive(Clone, Debug)]
pub struct BlockchainHandle;

impl BlockchainHandle {
    /// Stub: Send a message from an async context
    pub async fn tell_from_async<M>(&self, _msg: M, _sender: impl std::any::Any) -> Result<(), String>
    where
        M: std::any::Any + Send + 'static,
    {
        // TODO: Implement actual actor messaging
        Ok(())
    }

    /// Stub: Send a message with sender
    pub fn tell_from<M>(&self, _msg: M, _sender: impl std::any::Any) -> Result<(), String>
    where
        M: std::any::Any + Send + 'static,
    {
        // TODO: Implement actual actor messaging
        Ok(())
    }

    /// Stub: Get raw actor reference
    pub fn raw_ref(&self) -> &Self {
        self
    }

    /// Stub: Send a message
    pub fn tell<M>(&self, _msg: M) -> Result<(), String>
    where
        M: std::any::Any + Send + 'static,
    {
        // TODO: Implement actual actor messaging
        Ok(())
    }

    /// Stub: Create a new handle
    pub fn new() -> Self {
        Self
    }
}

impl Default for BlockchainHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Inventory payload wrapper.
#[derive(Debug, Clone)]
pub enum InventoryPayload {
    Block(Arc<Block>),
    Transaction(neo_protocol::payloads::Transaction),
    Extensible(neo_protocol::payloads::Extensible),
    Raw(neo_primitives::InventoryType, Vec<u8>),
}

#[cfg(test)]
pub fn should_schedule_reverify_idle(
    _reverify_cache_len: usize,
    _max_cache_size: usize,
) -> bool {
    true
}
