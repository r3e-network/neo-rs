pub use neo_storage::persistence::providers::memory_snapshot;
pub use neo_storage::persistence::providers::memory_store;
pub use neo_storage::persistence::providers::memory_store_provider;

pub use memory_snapshot::MemorySnapshot;
pub use memory_store::MemoryStore;
pub use memory_store_provider::MemoryStoreProvider;

// The RocksDB backend now lives in the standalone `neo-storage-rocksdb` crate;
// re-export it here (under the same `providers::rocksdb` path) so existing
// `neo_core::persistence::providers::{RocksDBStoreProvider, rocksdb::*}` callers
// are unaffected.
#[cfg(feature = "rocksdb")]
pub use neo_storage_rocksdb as rocksdb;

#[cfg(feature = "rocksdb")]
pub use neo_storage_rocksdb::RocksDBStoreProvider;
