//! [`NewTransactionEventArgs`] - event payload raised by
//! [`crate::MemoryPool::new_transaction`] when a fresh transaction
//! is about to be admitted into the mempool.

use neo_payloads::Transaction;
use neo_storage::DataCache;

/// Event arguments for the `MemoryPool::new_transaction` event.
///
/// Subscribers inspect the supplied transaction + snapshot and may
/// set `cancel = true` to veto the admission.
pub struct NewTransactionEventArgs {
    /// The transaction being validated.
    pub transaction: Transaction,
    /// The snapshot used during validation.
    pub snapshot: DataCache,
    /// Whether the transaction should be rejected by policy.
    pub cancel: bool,
}

impl NewTransactionEventArgs {
    /// Constructs a fresh `NewTransactionEventArgs` for the given
    /// transaction and snapshot.
    pub fn new(transaction: Transaction, snapshot: DataCache) -> Self {
        Self {
            transaction,
            snapshot,
            cancel: false,
        }
    }

    /// Cancels the admission of the transaction.
    pub fn cancel(&mut self) {
        self.cancel = true;
    }
}
