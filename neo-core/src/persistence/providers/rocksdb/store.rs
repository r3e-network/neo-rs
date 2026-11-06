//! Core compatibility facade; all raw RocksDB state and snapshots belong to storage.

use crate::error::CoreResult;
use crate::persistence::{
    read_cache::{ReadCacheConfig, ReadCacheStatsSnapshot},
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::{OnNewSnapshotDelegate, Store},
    store_snapshot::StoreSnapshot,
    write_batch_buffer::{WriteBatchStatsSnapshot, map_storage_flush_error},
    write_store::WriteStore,
};
use neo_storage::StorageResult;
use neo_storage::persistence::providers::rocksdb::store as raw;
use neo_storage::{StorageItem, StorageKey};
use std::sync::Arc;

pub use raw::RocksDbSnapshot;

/// Preserves the legacy core downcast and inherent error signatures.
pub struct RocksDbStore {
    inner: Arc<raw::RocksDbStore>,
}

impl RocksDbStore {
    pub(crate) fn from_raw(inner: raw::RocksDbStore) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Enables fast sync mode using the storage-owned configuration.
    pub fn enable_fast_sync_mode(&self) {
        self.inner.enable_fast_sync_mode();
    }

    /// Restores the storage-owned durable configuration.
    pub fn disable_fast_sync_mode(&self) {
        self.inner.disable_fast_sync_mode();
    }

    /// Flushes memtables and WAL through the storage owner.
    pub fn flush_memtables(&self) {
        self.inner.flush_memtables();
    }

    /// Returns backend memory usage statistics.
    pub fn memory_usage(&self) -> Option<(u64, u64)> {
        self.inner.memory_usage()
    }

    /// Enables the original point read cache.
    pub fn enable_read_cache(&mut self, config: ReadCacheConfig) {
        // Mutable facades are exclusive: a snapshot keeps its exposed facade
        // in an Arc, preventing a mutable borrow until its raw handle is released.
        Arc::get_mut(&mut self.inner)
            .expect("mutable RocksDB facade must have an exclusive backend handle")
            .enable_read_cache(config);
    }

    /// Disables the original point read cache.
    pub fn disable_read_cache(&mut self) {
        Arc::get_mut(&mut self.inner)
            .expect("mutable RocksDB facade must have an exclusive backend handle")
            .disable_read_cache();
    }

    /// Returns read cache statistics.
    pub fn read_cache_stats(&self) -> Option<ReadCacheStatsSnapshot> {
        self.inner.read_cache_stats()
    }

    /// Clears the storage-owned read cache.
    pub fn clear_read_cache(&self) {
        self.inner.clear_read_cache();
    }

    /// Returns storage-owned batch statistics.
    pub fn batch_commit_stats(&self) -> WriteBatchStatsSnapshot {
        self.inner.batch_commit_stats()
    }

    /// Preserves the legacy inherent CoreResult and I/O retryability.
    pub fn flush_batch_writes(&self) -> CoreResult<()> {
        self.inner
            .flush_batch_writes()
            .map_err(map_storage_flush_error)
    }
}

impl Clone for RocksDbStore {
    fn clone(&self) -> Self {
        // Delegate the original clone semantics; the storage owner shares the
        // DB, batch buffer, and read cache and retains its per-store settings.
        Self::from_raw(self.inner.as_ref().clone())
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        ReadOnlyStoreGeneric::try_get(self.inner.as_ref(), key)
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        ReadOnlyStoreGeneric::find(self.inner.as_ref(), key_prefix, direction)
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        ReadOnlyStoreGeneric::try_get(self.inner.as_ref(), key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        ReadOnlyStoreGeneric::find(self.inner.as_ref(), key_prefix, direction)
    }
}

impl ReadOnlyStore for RocksDbStore {}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.inner.delete_raw(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.inner.put_raw(key, value)
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.inner.put_sync_raw(key, value)
    }
}

impl Store for RocksDbStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        self.inner
            .snapshot_with_store(self, |inner| Arc::new(Self { inner }))
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.inner.on_new_snapshot(handler);
    }

    fn enable_fast_sync_mode(&self) {
        self.inner.enable_fast_sync_mode();
    }

    fn disable_fast_sync_mode(&self) {
        self.inner.disable_fast_sync_mode();
    }

    fn flush(&self) {
        self.inner.flush();
    }

    fn supports_durable_writes(&self) -> bool {
        self.inner.supports_durable_writes()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
