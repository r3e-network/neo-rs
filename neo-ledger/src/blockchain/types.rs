//! Blockchain command types and relay results.

use neo_actors::ActorRef;
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
pub fn classify_import_block(current_height: u32, block_index: u32) -> ImportDisposition {
    if block_index <= current_height {
        ImportDisposition::AlreadySeen
    } else if block_index == current_height.saturating_add(1) {
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

/// Blockchain actor handle wrapping an ActorRef.
///
/// Provides type-safe messaging to the blockchain actor with proper error
/// handling for mailbox capacity and actor lifecycle. All messaging methods
/// delegate to the underlying ActorRef, converting actor runtime errors to
/// String-based errors for consistency with the ledger layer API.
///
/// # Error Handling
///
/// All messaging methods return `Result<(), String>`:
/// - `Ok(())` - Message successfully enqueued to the actor's mailbox
/// - `Err(msg)` - Mailbox full, actor stopped, or system shutdown
///
/// Callers should handle mailbox-full errors by either:
/// 1. Using `tell_from_async` to apply backpressure
/// 2. Dropping the message if it's non-critical
/// 3. Logging and continuing with degraded functionality
///
/// # Implementation Note
///
/// This is the authoritative BlockchainHandle implementation for neo-ledger.
/// Higher layers (such as neo-system) may re-export or wrap this type, but
/// they should delegate to this implementation rather than reimplementing
/// the actor messaging logic.
///
/// # Reference
///
/// See C# Neo: `NeoSystem.Blockchain` (property returning the blockchain actor reference)
#[derive(Clone, Debug)]
pub struct BlockchainHandle {
    actor_ref: ActorRef,
}

impl BlockchainHandle {
    /// Creates a new blockchain handle wrapping the given actor reference.
    ///
    /// # Arguments
    ///
    /// * `actor_ref` - Reference to the blockchain actor spawned by the actor system
    pub fn new(actor_ref: ActorRef) -> Self {
        Self { actor_ref }
    }

    /// Returns a reference to the underlying actor reference.
    ///
    /// Useful for advanced operations like watching the actor for termination.
    pub fn raw_ref(&self) -> &ActorRef {
        &self.actor_ref
    }

    /// Sends a message to the blockchain actor without specifying a sender.
    ///
    /// Uses non-blocking `try_send` - if the mailbox is full, returns an error
    /// immediately rather than waiting.
    ///
    /// # Type Parameters
    ///
    /// * `M` - Message type, must be `Any + Send + 'static` for actor messaging
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - The actor's mailbox is full (bounded capacity exceeded)
    /// - The actor has stopped
    /// - The actor system is shutting down
    ///
    /// # Example
    ///
    /// ```ignore
    /// handle.tell(BlockchainCommand::Initialize)?;
    /// handle.tell(BlockchainCommand::Import(import_cmd))?;
    /// ```
    pub fn tell<M>(&self, msg: M) -> Result<(), String>
    where
        M: std::any::Any + Send + 'static,
    {
        self.actor_ref.tell(msg).map_err(|e| e.to_string())
    }

    /// Sends a message to the blockchain actor with an explicit sender.
    ///
    /// Uses non-blocking `try_send` - if the mailbox is full, returns an error
    /// immediately. The optional actor reference is preserved in the envelope
    /// so request/reply handlers can return their terminal result to the caller.
    ///
    /// # Type Parameters
    ///
    /// * `M` - Message type, must be `Any + Send + 'static`
    ///
    /// # Arguments
    ///
    /// * `msg` - The message to send
    /// * `sender` - Optional actor reference receiving the reply
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - The actor's mailbox is full
    /// - The actor has stopped
    /// - The actor system is shutting down
    pub fn tell_from<M>(&self, msg: M, sender: Option<ActorRef>) -> Result<(), String>
    where
        M: std::any::Any + Send + 'static,
    {
        self.actor_ref
            .tell_from(msg, sender)
            .map_err(|e| e.to_string())
    }

    /// Sends a message to the blockchain actor from an async context.
    ///
    /// Uses backpressured `send` - if the mailbox is full, this will await
    /// capacity rather than returning an error immediately. This is useful
    /// for critical messages that should not be dropped.
    ///
    /// # Type Parameters
    ///
    /// * `M` - Message type, must be `Any + Send + 'static`
    ///
    /// # Arguments
    ///
    /// * `msg` - The message to send
    /// * `sender` - Optional actor reference receiving the reply
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - The actor has stopped
    /// - The actor system is shutting down
    ///
    /// # Note
    ///
    /// Unlike `tell` and `tell_from`, this method will NOT fail if the mailbox
    /// is temporarily full - it will wait for capacity. However, it will still
    /// fail if the actor or system is permanently stopped.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Critical persist operation - wait for mailbox capacity
    /// handle.tell_from_async(
    ///     BlockchainCommand::PersistCompleted(event),
    ///     Some(actor_context.self_ref())
    /// ).await?;
    /// ```
    pub async fn tell_from_async<M>(&self, msg: M, sender: Option<ActorRef>) -> Result<(), String>
    where
        M: std::any::Any + Send + 'static,
    {
        self.actor_ref
            .tell_from_async(msg, sender)
            .await
            .map_err(|e| e.to_string())
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
pub fn should_schedule_reverify_idle(_reverify_cache_len: usize, _max_cache_size: usize) -> bool {
    true
}
