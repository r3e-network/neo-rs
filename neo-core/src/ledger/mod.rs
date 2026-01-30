//! Ledger module for Neo blockchain
//!
//! This module provides ledger functionality matching the C# Neo.Ledger namespace.
//!
//! ## MemoryPool
//!
//! The [`MemoryPool`](crate::ledger::MemoryPool) in this module is the **canonical, C# parity implementation**.
//! It includes:
//! - Transaction verification context (sender fee tracking, oracle responses)
//! - Conflict attribute detection and resolution
//! - Verified/unverified transaction queues
//! - Transaction reverification logic
//! - Event callbacks for transaction lifecycle
//!
//! For lightweight use cases (testing, standalone tools), see the `neo-mempool` crate.

/// Block structure and operations.
pub mod block;
/// Block header structure.
pub mod block_header;
/// Blockchain state management.
#[cfg(feature = "runtime")]
pub mod blockchain;
/// Application execution results.
pub mod blockchain_application_executed;
/// Genesis block generation.
pub mod genesis;
/// Header caching for sync.
pub mod header_cache;
/// Ledger context for operations.
pub mod ledger_context;
/// Transaction memory pool.
pub mod memory_pool;
/// New transaction event arguments.
pub mod new_transaction_event_args;
/// Pool item wrapper for transactions.
pub mod pool_item;
/// Transaction removal reasons.
pub mod transaction_removal_reason;
/// Transaction removed event arguments.
pub mod transaction_removed_event_args;
/// Transaction routing logic.
pub mod transaction_router;
pub mod transaction_verification_context;
pub mod verify_result;

// Re-export commonly used types
pub use block::Block;
pub use block_header::BlockHeader;
#[cfg(feature = "runtime")]
pub use blockchain::{
    Blockchain, BlockchainCommand, FillCompleted, FillMemoryPool, Import, ImportCompleted,
    PersistCompleted, PreverifyCompleted, RelayResult, Reverify, ReverifyItem,
};
pub use blockchain_application_executed::ApplicationExecuted;
pub use genesis::create_genesis_block;
pub use header_cache::HeaderCache;
pub use ledger_context::LedgerContext;
pub use memory_pool::MemoryPool;
pub use new_transaction_event_args::NewTransactionEventArgs;
pub use pool_item::PoolItem;
pub use transaction_removal_reason::TransactionRemovalReason;
pub use transaction_removed_event_args::TransactionRemovedEventArgs;
pub use transaction_router::TransactionRouter;
pub use transaction_verification_context::TransactionVerificationContext;
pub use verify_result::VerifyResult;

// Compatibility types for callers that referenced the old `neo-core::ledger` surface.
#[cfg(not(feature = "runtime"))]
/// Relay result (runtime implementation lives behind `neo-core/runtime`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayResult {
    /// Successfully relayed
    Succeed,
    /// Already exists
    AlreadyExists,
    /// Out of memory
    OutOfMemory,
    /// Unable to verify
    UnableToVerify,
    /// Invalid
    Invalid,
    /// Policy fail
    PolicyFail,
    /// Unknown
    Unknown,
}

#[cfg(not(feature = "runtime"))]
/// Persist completed event (runtime implementation lives behind `neo-core/runtime`).
#[derive(Debug, Clone)]
pub struct PersistCompleted {
    /// Block index that was persisted
    pub block_index: u32,
}
