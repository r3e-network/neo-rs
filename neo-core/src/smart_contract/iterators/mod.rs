//! Iterators module - matches C# Neo.SmartContract.Iterators exactly

/// Iterator trait definition.
pub mod i_iterator;
/// Iterator interop wrapper.
pub mod iterator_interop;
/// Storage iterator implementation.
pub mod storage_iterator;

pub use self::i_iterator::IIterator;
pub use self::iterator_interop::IteratorInterop;
pub use self::storage_iterator::StorageIterator;
