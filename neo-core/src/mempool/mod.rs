//! # Neo Mempool
//!
//! Transaction mempool (memory pool) for Neo N3 blockchain node.
//!
//! **IMPORTANT**: This crate provides a **lightweight, standalone** mempool implementation.
//! For full C# Neo parity (including transaction verification context, conflict detection,
//! and reverification logic), use `neo_core::ledger::MemoryPool` instead.
//!
//! ## When to use this crate
//!
//! - **Testing**: Lightweight testing scenarios without full neo-core dependency
//! - **Standalone tools**: CLI tools that only need basic mempool tracking
//! - **Custom implementations**: Building alternative mempool strategies
//!
//! ## When to use neo-core's `MemoryPool`
//!
//! - **Full node operation**: Production nodes requiring C# Neo compatibility
//! - **Consensus participation**: When transaction verification context is needed
//! - **Plugin development**: RPC plugins that interact with the canonical mempool
//!
//! ## Features
//!
//! - Transaction pool management
//! - Fee-based prioritization
//! - Transaction validation queue
//! - Duplicate detection
//! - Expiration handling
//!
//! ## Architecture
//!
//! The mempool maintains pending transactions that have been received
//! but not yet included in a block. Transactions are ordered by fee
//! priority and removed when they expire or are included in a block.

mod error;
mod policy;
mod pool;
mod transaction_entry;

pub use error::{MempoolError, MempoolResult};
pub use policy::FeePolicy;
pub use pool::{Mempool, MempoolConfig};
pub use transaction_entry::{TransactionEntry, TransactionEntryParams};

/// Default maximum mempool capacity
pub const DEFAULT_MAX_TRANSACTIONS: usize = 50_000;

/// Default transaction expiration (in blocks)
pub const DEFAULT_EXPIRATION_BLOCKS: u32 = 5760; // ~24 hours at 15 sec/block

#[cfg(test)]
mod tests {
    // Tests are inline within source files
}
