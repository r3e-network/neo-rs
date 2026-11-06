//! Legacy provider facade over the storage-owned RocksDB provider.

use crate::persistence::{
    read_cache::ReadCacheConfig, storage::StorageConfig, store::Store,
    store_provider::StoreProvider,
};
use neo_storage::StorageResult;
use neo_storage::persistence::providers::rocksdb::provider as raw;
use std::sync::Arc;

use super::store::RocksDbStore;

pub use raw::{
    BatchCommitConfig, BatchCommitStats, BatchCommitStatsSnapshot, BatchCommitter, ReadAheadConfig,
};

/// Preserves core store downcasts without owning backend configuration or state.
#[derive(Debug, Clone)]
pub struct RocksDBStoreProvider {
    inner: raw::RocksDBStoreProvider,
}

impl RocksDBStoreProvider {
    /// Creates the storage-owned provider with its original defaults.
    pub fn new(base_config: StorageConfig) -> Self {
        Self {
            inner: raw::RocksDBStoreProvider::new(base_config),
        }
    }

    /// Overrides the write-batching configuration.
    pub fn with_batch_config(mut self, config: BatchCommitConfig) -> Self {
        self.inner = self.inner.with_batch_config(config);
        self
    }

    /// Installs a point read cache with the given configuration.
    pub fn with_read_cache(mut self, config: ReadCacheConfig) -> Self {
        self.inner = self.inner.with_read_cache(config);
        self
    }

    /// Disables the point read cache.
    pub fn without_read_cache(mut self) -> Self {
        self.inner = self.inner.without_read_cache();
        self
    }

    /// Enables or disables SST bloom filters.
    pub fn with_bloom_filters(mut self, enable: bool) -> Self {
        self.inner = self.inner.with_bloom_filters(enable);
        self
    }

    /// Enables or disables read-ahead for sequential scans.
    pub fn with_read_ahead(mut self, enable: bool) -> Self {
        self.inner = self.inner.with_read_ahead(enable);
        self
    }

    /// Returns the original provider's shared statistics handle.
    pub fn batch_stats(&self) -> Arc<BatchCommitStats> {
        self.inner.batch_stats()
    }
}

impl StoreProvider for RocksDBStoreProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn get_store(&self, path: &str) -> StorageResult<Arc<dyn Store>> {
        let store = self.inner.get_rocksdb_store(path)?;
        Ok(Arc::new(RocksDbStore::from_raw(store)))
    }
}
