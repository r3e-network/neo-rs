
/// Trait for handling TransactionRemoved events from the MemoryPool
pub trait ITransactionRemovedHandler {
    /// Handler of TransactionRemoved event from MemoryPool
    /// Triggered when a transaction is removed from the MemoryPool.
    ///
    /// # Arguments
    ///
    /// * `sender` - The source of the event
    /// * `tx` - The arguments of event that removes a transaction from the MemoryPool
    fn memory_pool_transaction_removed_handler(&self, sender: &dyn std::any::Any, tx: &TransactionRemovedEventArgs);
}
