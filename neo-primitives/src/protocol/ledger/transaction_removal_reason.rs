//! `TransactionRemovalReason` - matches C# Neo.Ledger.TransactionRemovalReason exactly.
//!
//! This is the single source of truth for transaction removal reasons. Both
//! `neo-core::ledger` and `neo-p2p` re-export this type for backward compatibility.

use crate::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    /// The reason a transaction was removed from the memory pool.
    pub TransactionRemovalReason {
        /// The transaction was rejected since it was the lowest priority transaction
        /// and the memory pool capacity was exceeded.
        CapacityExceeded = 0,
        /// The transaction was rejected due to failing re-validation after a block was persisted.
        NoLongerValid = 1,
        /// The transaction was rejected due to conflict with higher priority transactions
        /// with Conflicts attribute.
        Conflict = 2,
    }
}

#[cfg(test)]
#[path = "../../tests/protocol/ledger/transaction_removal_reason.rs"]
mod tests;
