//! Runtime-selected store backend.
//!
//! Node startup chooses a storage backend from configuration, but after that
//! choice the rest of the node should still carry a concrete store type instead
//! of spreading an erased `Store` handle through composition. [`RuntimeStore`] is that
//! concrete sum type: it uses enum dispatch at the startup boundary and then
//! implements the normal [`Store`](crate::persistence::Store) surface.

use std::sync::Arc;

use crate::error::{StorageError, StorageResult};
use crate::mdbx::{MdbxSnapshot, MdbxStore};
use crate::persistence::store::{
    MdbxEnvironmentInfo, RawOverlaySource, RocksDbBatchMetrics, StoreBackendKind,
};
use crate::persistence::{
    RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric, Store, StoreSnapshot, WriteStore,
};
use crate::rocksdb::{RocksDbSnapshot, RocksDbStore};
use crate::types::{SeekDirection, StorageItem, StorageKey};

use super::{memory_snapshot::MemorySnapshot, memory_store::MemoryStore};

/// Concrete runtime-selected storage backend.
#[derive(Clone, Debug)]
pub enum RuntimeStore {
    /// Ephemeral in-memory backend.
    Memory(MemoryStore),
    /// MDBX backend.
    Mdbx(MdbxStore),
    /// RocksDB backend.
    RocksDb(RocksDbStore),
}

/// Concrete snapshot for a runtime-selected storage backend.
#[derive(Debug)]
pub struct RuntimeSnapshot {
    store: Arc<RuntimeStore>,
    inner: RuntimeSnapshotInner,
}

#[derive(Debug)]
enum RuntimeSnapshotInner {
    Memory(Arc<MemorySnapshot>),
    Mdbx(Arc<MdbxSnapshot>),
    RocksDb(Arc<RocksDbSnapshot>),
}

/// Concrete raw iterator for a runtime-selected store.
pub enum RuntimeRawFindIterator<'a> {
    /// Iterator over the in-memory backend.
    Memory(<MemoryStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
    /// Iterator over the MDBX backend.
    Mdbx(<MdbxStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
    /// Iterator over the RocksDB backend.
    RocksDb(<RocksDbStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
}

impl Iterator for RuntimeRawFindIterator<'_> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Memory(iter) => iter.next(),
            Self::Mdbx(iter) => iter.next(),
            Self::RocksDb(iter) => iter.next(),
        }
    }
}

/// Concrete typed-storage iterator for a runtime-selected store.
pub enum RuntimeStorageFindIterator<'a> {
    /// Iterator over the in-memory backend.
    Memory(<MemoryStore as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::FindIterator<'a>),
    /// Iterator over the MDBX backend.
    Mdbx(<MdbxStore as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::FindIterator<'a>),
    /// Iterator over the RocksDB backend.
    RocksDb(<RocksDbStore as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::FindIterator<'a>),
}

impl Iterator for RuntimeStorageFindIterator<'_> {
    type Item = (StorageKey, StorageItem);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Memory(iter) => iter.next(),
            Self::Mdbx(iter) => iter.next(),
            Self::RocksDb(iter) => iter.next(),
        }
    }
}

/// Concrete raw iterator for a runtime-selected snapshot.
pub enum RuntimeSnapshotRawFindIterator<'a> {
    /// Iterator over an in-memory snapshot.
    Memory(<MemorySnapshot as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
    /// Iterator over an MDBX snapshot.
    Mdbx(<MdbxSnapshot as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
    /// Iterator over a RocksDB snapshot.
    RocksDb(<RocksDbSnapshot as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
}

impl Iterator for RuntimeSnapshotRawFindIterator<'_> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Memory(iter) => iter.next(),
            Self::Mdbx(iter) => iter.next(),
            Self::RocksDb(iter) => iter.next(),
        }
    }
}

impl RuntimeSnapshot {
    fn memory(store: RuntimeStore, snapshot: Arc<MemorySnapshot>) -> Self {
        Self {
            store: Arc::new(store),
            inner: RuntimeSnapshotInner::Memory(snapshot),
        }
    }

    fn mdbx(store: RuntimeStore, snapshot: Arc<MdbxSnapshot>) -> Self {
        Self {
            store: Arc::new(store),
            inner: RuntimeSnapshotInner::Mdbx(snapshot),
        }
    }

    fn rocksdb(store: RuntimeStore, snapshot: Arc<RocksDbSnapshot>) -> Self {
        Self {
            store: Arc::new(store),
            inner: RuntimeSnapshotInner::RocksDb(snapshot),
        }
    }

    fn shared_snapshot_error() -> StorageError {
        StorageError::invalid_operation("runtime snapshot is still shared")
    }
}

impl RuntimeStore {
    /// Returns the in-memory backend when this runtime store uses memory.
    pub fn as_memory(&self) -> Option<&MemoryStore> {
        match self {
            Self::Memory(store) => Some(store),
            _ => None,
        }
    }

    /// Returns the MDBX backend when this runtime store uses MDBX.
    pub fn as_mdbx(&self) -> Option<&MdbxStore> {
        match self {
            Self::Mdbx(store) => Some(store),
            _ => None,
        }
    }

    /// Returns the RocksDB backend when this runtime store uses RocksDB.
    pub fn as_rocksdb(&self) -> Option<&RocksDbStore> {
        match self {
            Self::RocksDb(store) => Some(store),
            _ => None,
        }
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RuntimeSnapshot {
    type FindIterator<'a> = RuntimeSnapshotRawFindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => snapshot.try_get(key),
            RuntimeSnapshotInner::Mdbx(snapshot) => snapshot.try_get(key),
            RuntimeSnapshotInner::RocksDb(snapshot) => snapshot.try_get(key),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => {
                RuntimeSnapshotRawFindIterator::Memory(snapshot.find(key_prefix, direction))
            }
            RuntimeSnapshotInner::Mdbx(snapshot) => {
                RuntimeSnapshotRawFindIterator::Mdbx(snapshot.find(key_prefix, direction))
            }
            RuntimeSnapshotInner::RocksDb(snapshot) => {
                RuntimeSnapshotRawFindIterator::RocksDb(snapshot.find(key_prefix, direction))
            }
        }
    }
}

impl RawReadOnlyStore for RuntimeSnapshot {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => snapshot.try_get_bytes(key),
            RuntimeSnapshotInner::Mdbx(snapshot) => snapshot.try_get_bytes(key),
            RuntimeSnapshotInner::RocksDb(snapshot) => snapshot.try_get_bytes(key),
        }
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for RuntimeSnapshot {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        match &mut self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .delete(key),
            RuntimeSnapshotInner::Mdbx(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .delete(key),
            RuntimeSnapshotInner::RocksDb(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .delete(key),
        }
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        match &mut self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .put(key, value),
            RuntimeSnapshotInner::Mdbx(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .put(key, value),
            RuntimeSnapshotInner::RocksDb(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .put(key, value),
        }
    }
}

impl StoreSnapshot for RuntimeSnapshot {
    type Store = RuntimeStore;

    fn store(&self) -> Arc<Self::Store> {
        self.store.clone()
    }

    fn try_commit(&mut self) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        match &mut self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .try_commit(),
            RuntimeSnapshotInner::Mdbx(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .try_commit(),
            RuntimeSnapshotInner::RocksDb(snapshot) => Arc::get_mut(snapshot)
                .ok_or_else(RuntimeSnapshot::shared_snapshot_error)?
                .try_commit(),
        }
    }
}

impl From<MemoryStore> for RuntimeStore {
    fn from(store: MemoryStore) -> Self {
        Self::Memory(store)
    }
}

impl From<MdbxStore> for RuntimeStore {
    fn from(store: MdbxStore) -> Self {
        Self::Mdbx(store)
    }
}

impl From<RocksDbStore> for RuntimeStore {
    fn from(store: RocksDbStore) -> Self {
        Self::RocksDb(store)
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RuntimeStore {
    type FindIterator<'a> = RuntimeRawFindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        match self {
            Self::Memory(store) => store.try_get(key),
            Self::Mdbx(store) => store.try_get(key),
            Self::RocksDb(store) => store.try_get(key),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        match self {
            Self::Memory(store) => {
                RuntimeRawFindIterator::Memory(store.find(key_prefix, direction))
            }
            Self::Mdbx(store) => RuntimeRawFindIterator::Mdbx(store.find(key_prefix, direction)),
            Self::RocksDb(store) => {
                RuntimeRawFindIterator::RocksDb(store.find(key_prefix, direction))
            }
        }
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RuntimeStore {
    type FindIterator<'a> = RuntimeStorageFindIterator<'a>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        match self {
            Self::Memory(store) => store.try_get(key),
            Self::Mdbx(store) => store.try_get(key),
            Self::RocksDb(store) => store.try_get(key),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        match self {
            Self::Memory(store) => {
                RuntimeStorageFindIterator::Memory(store.find(key_prefix, direction))
            }
            Self::Mdbx(store) => {
                RuntimeStorageFindIterator::Mdbx(store.find(key_prefix, direction))
            }
            Self::RocksDb(store) => {
                RuntimeStorageFindIterator::RocksDb(store.find(key_prefix, direction))
            }
        }
    }
}

impl RawReadOnlyStore for RuntimeStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self {
            Self::Memory(store) => store.try_get_bytes(key),
            Self::Mdbx(store) => store.try_get_bytes(key),
            Self::RocksDb(store) => store.try_get_bytes(key),
        }
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for RuntimeStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.delete(key),
            Self::Mdbx(store) => store.delete(key),
            Self::RocksDb(store) => store.delete(key),
        }
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.put(key, value),
            Self::Mdbx(store) => store.put(key, value),
            Self::RocksDb(store) => store.put(key, value),
        }
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.put_sync(key, value),
            Self::Mdbx(store) => store.put_sync(key, value),
            Self::RocksDb(store) => store.put_sync(key, value),
        }
    }
}

impl ReadOnlyStore for RuntimeStore {}

impl Store for RuntimeStore {
    type Snapshot = RuntimeSnapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot> {
        match self {
            Self::Memory(store) => Arc::new(RuntimeSnapshot::memory(
                Self::Memory(store.clone()),
                store.snapshot(),
            )),
            Self::Mdbx(store) => Arc::new(RuntimeSnapshot::mdbx(
                Self::Mdbx(store.clone()),
                store.snapshot(),
            )),
            Self::RocksDb(store) => Arc::new(RuntimeSnapshot::rocksdb(
                Self::RocksDb(store.clone()),
                store.snapshot(),
            )),
        }
    }

    fn flush(&self) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.flush(),
            Self::Mdbx(store) => store.flush(),
            Self::RocksDb(store) => store.flush(),
        }
    }

    fn backend_kind(&self) -> StoreBackendKind {
        match self {
            Self::Memory(store) => store.backend_kind(),
            Self::Mdbx(store) => store.backend_kind(),
            Self::RocksDb(store) => store.backend_kind(),
        }
    }

    fn mdbx_environment_info(&self) -> Option<StorageResult<MdbxEnvironmentInfo>> {
        match self {
            Self::Memory(store) => store.mdbx_environment_info(),
            Self::Mdbx(store) => store.mdbx_environment_info(),
            Self::RocksDb(store) => store.mdbx_environment_info(),
        }
    }

    fn rocksdb_batch_metrics(&self) -> Option<RocksDbBatchMetrics> {
        match self {
            Self::Memory(store) => store.rocksdb_batch_metrics(),
            Self::Mdbx(store) => store.rocksdb_batch_metrics(),
            Self::RocksDb(store) => store.rocksdb_batch_metrics(),
        }
    }

    fn supports_fast_sync_mode(&self) -> bool {
        match self {
            Self::Memory(store) => store.supports_fast_sync_mode(),
            Self::Mdbx(store) => store.supports_fast_sync_mode(),
            Self::RocksDb(store) => store.supports_fast_sync_mode(),
        }
    }

    fn enable_fast_sync_mode(&self) {
        match self {
            Self::Memory(store) => store.enable_fast_sync_mode(),
            Self::Mdbx(store) => store.enable_fast_sync_mode(),
            Self::RocksDb(store) => store.enable_fast_sync_mode(),
        }
    }

    fn disable_fast_sync_mode(&self) {
        match self {
            Self::Memory(store) => store.disable_fast_sync_mode(),
            Self::Mdbx(store) => store.disable_fast_sync_mode(),
            Self::RocksDb(store) => store.disable_fast_sync_mode(),
        }
    }

    fn discard_pending_fast_sync_writes(&self) {
        match self {
            Self::Memory(store) => store.discard_pending_fast_sync_writes(),
            Self::Mdbx(store) => store.discard_pending_fast_sync_writes(),
            Self::RocksDb(store) => store.discard_pending_fast_sync_writes(),
        }
    }

    fn has_pending_fast_sync_writes(&self) -> bool {
        match self {
            Self::Memory(store) => store.has_pending_fast_sync_writes(),
            Self::Mdbx(store) => store.has_pending_fast_sync_writes(),
            Self::RocksDb(store) => store.has_pending_fast_sync_writes(),
        }
    }

    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> StorageResult<bool> {
        match self {
            Self::Memory(store) => store.try_commit_raw_overlay(overlay),
            Self::Mdbx(store) => store.try_commit_raw_overlay(overlay),
            Self::RocksDb(store) => store.try_commit_raw_overlay(overlay),
        }
    }

    fn try_commit_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        match self {
            Self::Memory(store) => store.try_commit_borrowed_raw_overlay(overlay),
            Self::Mdbx(store) => store.try_commit_borrowed_raw_overlay(overlay),
            Self::RocksDb(store) => store.try_commit_borrowed_raw_overlay(overlay),
        }
    }

    fn try_commit_durable_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        match self {
            Self::Memory(store) => store.try_commit_durable_borrowed_raw_overlay(overlay),
            Self::Mdbx(store) => store.try_commit_durable_borrowed_raw_overlay(overlay),
            Self::RocksDb(store) => store.try_commit_durable_borrowed_raw_overlay(overlay),
        }
    }
}
