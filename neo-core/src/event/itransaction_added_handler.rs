use std::any::Any;
use crate::network::payloads::Transaction;

/// Trait for handling TransactionAdded events from the MemoryPool.
pub trait ITransactionAddedHandler {
    /// Handler for the TransactionAdded event from the MemoryPool.
    ///
    /// This method is triggered when a transaction is added to the MemoryPool.
    ///
    /// # Arguments
    ///
    /// * `sender` - A reference to the source of the event.
    /// * `tx` - The transaction added to the memory pool.
    fn memory_pool_transaction_added_handler(&self, sender: &dyn Any, tx: &Transaction);
}
