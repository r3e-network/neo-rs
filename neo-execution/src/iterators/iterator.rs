//! StorageIterator - matches C# Neo.SmartContract.Iterators.IIterator exactly

use crate::interoperable::SmartContractStackItem;

/// Represents iterators in smart contract (matches C# StorageIterator)
pub trait StorageIterator: std::fmt::Debug {
    /// Advances the iterator to the next element of the collection
    fn next(&mut self) -> bool;

    /// Gets the element in the collection at the current position of the iterator
    fn value(&self) -> SmartContractStackItem;

    /// Dispose/cleanup resources if needed
    fn dispose(&mut self) {
        // Default implementation does nothing
    }
}
