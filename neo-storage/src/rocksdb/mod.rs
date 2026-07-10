//! # neo-storage::rocksdb
//!
//! RocksDB provider, store, snapshot, and write-batch adapter.
//!
//! ## Boundary
//!
//! This module belongs to `neo-storage`. This infrastructure crate owns store
//! mechanics and must not execute contracts, import blocks, or make RPC/network
//! policy decisions.
//!
//! ## Contents
//!
//! - `provider`: Provider adapter for the surrounding trait boundary.
//! - `store`: Store implementation for the surrounding backend or domain.
//! - `write_batch_buffer`: RocksDB write-batch staging buffer.
//! - `tests`: Module-local tests and regression coverage.

/// Concrete RocksDB prefix-scan iterators.
pub mod find_iterator;
/// RocksDB store provider and tuning options.
pub mod provider;
/// Concrete RocksDB store and snapshot implementations.
pub mod store;
/// RocksDB write-batch staging buffer.
pub mod write_batch_buffer;

#[cfg(test)]
#[path = "../tests/rocksdb/mod.rs"]
mod tests;

pub use find_iterator::{RocksDbRawFindIterator, RocksDbStorageFindIterator};
pub use provider::{
    BatchCommitConfig, BatchCommitStats, BatchCommitStatsSnapshot, BatchCommitter, ReadAheadConfig,
    RocksDBStoreProvider,
};
pub use store::{RocksDbSnapshot, RocksDbStore};
pub use write_batch_buffer::{
    WriteBatchBuffer, WriteBatchConfig, WriteBatchStats, WriteBatchStatsSnapshot,
};
