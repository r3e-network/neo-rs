
use std::collections::HashSet;
use NeoRust::builder::Transaction;
use crate::ledger::transaction_removal_reason::TransactionRemovalReason;

/// Represents the event data of `MemoryPool::transaction_removed`.
pub struct TransactionRemovedEventArgs {
    /// The `Transaction`s that are being removed.
    pub transactions: HashSet<Transaction>,

    /// The reason a transaction was removed.
    pub reason: TransactionRemovalReason,
}

impl TransactionRemovedEventArgs {
    /// Creates a new instance of `TransactionRemovedEventArgs`.
    pub fn new(transactions: HashSet<Transaction>, reason: TransactionRemovalReason) -> Self {
        Self {
            transactions,
            reason,
        }
    }
}
