//! [`TransactionRemovedEventArgs`] - event payload raised by
//! [`crate::MemoryPool::transaction_removed`] when a transaction
//! (or batch of transactions) is dropped from the mempool.

use neo_primitives::TransactionRemovalReason;
use neo_payloads::Transaction;

/// Event arguments for the `MemoryPool::transaction_removed` event.
pub struct TransactionRemovedEventArgs {
    /// The transactions that are being removed.
    pub transactions: Vec<Transaction>,
    /// The reason the transactions were removed.
    pub reason: TransactionRemovalReason,
}

impl TransactionRemovedEventArgs {
    /// Constructs a new event-args instance.
    pub fn new(transactions: Vec<Transaction>, reason: TransactionRemovalReason) -> Self {
        Self {
            transactions,
            reason,
        }
    }
}
