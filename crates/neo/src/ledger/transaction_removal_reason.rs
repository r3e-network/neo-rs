//! Transaction removal reason implementation.
//!
//! This module provides the TransactionRemovalReason functionality exactly matching C# Neo TransactionRemovalReason.

// No using directives in C# file

/// namespace Neo.Ledger -> public enum TransactionRemovalReason : byte

/// The reason a transaction was removed.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionRemovalReason {
    /// The transaction was rejected since it was the lowest priority transaction and the memory pool capacity was exceeded.
    CapacityExceeded = 0,

    /// The transaction was rejected due to failing re-validation after a block was persisted.
    NoLongerValid = 1,

    /// The transaction was rejected due to conflict with higher priority transactions with Conflicts attribute.
    Conflict = 2,
}
