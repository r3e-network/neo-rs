//! RocksDB storage backend for the Neo blockchain node.
//!
//! Implements this crate's `Store`/`ReadOnlyStore`/`WriteStore`/`StoreProvider`
//! traits over RocksDB. Lifted out of `neo-core` so the heavyweight `rocksdb`
//! dependency stays optional and confined to nodes that select this backend.

/// RocksDB store provider and tuning options.
pub mod provider;
/// Concrete RocksDB store and snapshot implementations.
pub mod store;
pub mod write_batch_buffer;

#[cfg(test)]
#[path = "../tests/rocksdb.rs"]
mod tests;

pub use provider::{
    BatchCommitConfig, BatchCommitStats, BatchCommitStatsSnapshot, BatchCommitter, ReadAheadConfig,
    RocksDBStoreProvider,
};
pub use store::{RocksDbSnapshot, RocksDbStore};
pub use write_batch_buffer::{
    WriteBatchBuffer, WriteBatchConfig, WriteBatchStats, WriteBatchStatsSnapshot,
};
