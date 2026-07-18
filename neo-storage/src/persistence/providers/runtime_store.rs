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
use crate::persistence::store::{MdbxEnvironmentInfo, RawOverlaySource, StoreBackendKind};
use crate::persistence::{
    RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric, Store, StoreMaintenanceBatch,
    StoreSnapshot, TransactionalStore, WriteStore,
};
use crate::types::{SeekDirection, StorageItem, StorageKey};

use super::{memory_snapshot::MemorySnapshot, memory_store::MemoryStore};

/// Concrete runtime-selected storage backend.
#[derive(Clone, Debug)]
pub enum RuntimeStore {
    /// Ephemeral in-memory backend.
    Memory(MemoryStore),
    /// MDBX backend.
    Mdbx(MdbxStore),
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
}

/// Concrete raw iterator for a runtime-selected store.
pub enum RuntimeRawFindIterator<'a> {
    /// Iterator over the in-memory backend.
    Memory(<MemoryStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
    /// Iterator over the MDBX backend.
    Mdbx(<MdbxStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
}

impl Iterator for RuntimeRawFindIterator<'_> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Memory(iter) => iter.next(),
            Self::Mdbx(iter) => iter.next(),
        }
    }
}

/// Concrete typed-storage iterator for a runtime-selected store.
pub enum RuntimeStorageFindIterator<'a> {
    /// Iterator over the in-memory backend.
    Memory(<MemoryStore as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::FindIterator<'a>),
    /// Iterator over the MDBX backend.
    Mdbx(<MdbxStore as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::FindIterator<'a>),
}

impl Iterator for RuntimeStorageFindIterator<'_> {
    type Item = (StorageKey, StorageItem);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Memory(iter) => iter.next(),
            Self::Mdbx(iter) => iter.next(),
        }
    }
}

/// Concrete raw iterator for a runtime-selected snapshot.
pub enum RuntimeSnapshotRawFindIterator<'a> {
    /// Iterator over an in-memory snapshot.
    Memory(<MemorySnapshot as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
    /// Iterator over an MDBX snapshot.
    Mdbx(<MdbxSnapshot as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>),
}

impl Iterator for RuntimeSnapshotRawFindIterator<'_> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Memory(iter) => iter.next(),
            Self::Mdbx(iter) => iter.next(),
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

    /// Visits raw keys in prefix order without materializing a persistent
    /// backend's complete prefix domain.
    pub fn visit_raw_keys_with_prefix<F>(
        &self,
        key_prefix: &[u8],
        maximum: Option<u64>,
        mut visitor: F,
    ) -> StorageResult<u64>
    where
        F: FnMut(&[u8]),
    {
        match self {
            Self::Mdbx(store) => store.visit_raw_keys_with_prefix(key_prefix, maximum, visitor),
            Self::Memory(store) => {
                if maximum == Some(0) {
                    return Ok(0);
                }
                let prefix = key_prefix.to_vec();
                let mut visited = 0u64;
                for (key, _) in store.find(Some(&prefix), SeekDirection::Forward) {
                    visitor(&key);
                    visited = visited.saturating_add(1);
                    if maximum.is_some_and(|maximum| visited >= maximum) {
                        break;
                    }
                }
                Ok(visited)
            }
        }
    }

    /// Visits raw prefix rows through one backend-native frozen scan.
    ///
    /// The callback borrows each key/value pair and may fail, which lets
    /// bounded migration tooling flush a durable output frame or abort without
    /// materializing the remaining namespace.
    pub fn visit_raw_entries_with_prefix<F>(
        &self,
        key_prefix: &[u8],
        maximum: Option<u64>,
        mut visitor: F,
    ) -> StorageResult<u64>
    where
        F: FnMut(&[u8], &[u8]) -> StorageResult<()>,
    {
        match self {
            Self::Mdbx(store) => store.visit_raw_entries_with_prefix(key_prefix, maximum, visitor),
            Self::Memory(store) => {
                if maximum == Some(0) {
                    return Ok(0);
                }
                let prefix = key_prefix.to_vec();
                let mut visited = 0u64;
                for (key, value) in store.find(Some(&prefix), SeekDirection::Forward) {
                    visitor(&key, &value)?;
                    visited = visited.saturating_add(1);
                    if maximum.is_some_and(|maximum| visited >= maximum) {
                        break;
                    }
                }
                Ok(visited)
            }
        }
    }

    /// Creates an isolated store namespace when the selected runtime backend
    /// can keep it in the same atomic commit domain.
    pub fn open_coordinated_namespace(&self, name: &str) -> StorageResult<Self> {
        match self {
            Self::Mdbx(store) => store.open_named_table(name).map(Self::Mdbx),
            Self::Memory(_) => Err(StorageError::invalid_operation(format!(
                "{} does not provide coordinated store namespaces",
                self.backend_kind().as_str()
            ))),
        }
    }

    /// Atomically publishes overlays from two runtime-selected store views.
    pub fn commit_coordinated_overlays<P, S>(
        &self,
        primary: &mut P,
        secondary_store: &Self,
        secondary: &mut S,
    ) -> StorageResult<()>
    where
        P: RawOverlaySource + ?Sized,
        S: RawOverlaySource + ?Sized,
    {
        match (self, secondary_store) {
            (Self::Mdbx(primary_store), Self::Mdbx(secondary_store)) => {
                primary_store.commit_coordinated_overlays(primary, secondary_store, secondary)
            }
            _ => Err(StorageError::invalid_operation(
                "coordinated runtime commit requires two MDBX views from one environment",
            )),
        }
    }

    /// Atomically publishes overlays from two runtime-selected store views,
    /// feeding the secondary overlay's entries to an optional shadow
    /// dual-writer. See [`MdbxStore::commit_coordinated_overlays_with_shadow`]
    /// for the marker and failure discipline.
    pub fn commit_coordinated_overlays_with_shadow<P, S>(
        &self,
        primary: &mut P,
        secondary_store: &Self,
        secondary: &mut S,
        shadow: Option<&mut crate::persistence::ShadowCommitHook<'_>>,
    ) -> StorageResult<()>
    where
        P: RawOverlaySource + ?Sized,
        S: RawOverlaySource + ?Sized,
    {
        match (self, secondary_store) {
            (Self::Mdbx(primary_store), Self::Mdbx(secondary_store)) => primary_store
                .commit_coordinated_overlays_with_shadow(
                    primary,
                    secondary_store,
                    secondary,
                    shadow,
                ),
            _ => Err(StorageError::invalid_operation(
                "coordinated runtime commit requires two MDBX views from one environment",
            )),
        }
    }

    /// Atomically publishes two MDBX overlays and a mandatory maintenance
    /// marker. Marker failure aborts the complete transaction.
    pub fn commit_coordinated_overlays_with_required_marker<P, S>(
        &self,
        primary: &mut P,
        secondary_store: &Self,
        secondary: &mut S,
        marker: &crate::persistence::CoordinatedCommitMarker,
    ) -> StorageResult<()>
    where
        P: RawOverlaySource + ?Sized,
        S: RawOverlaySource + ?Sized,
    {
        match (self, secondary_store) {
            (Self::Mdbx(primary_store), Self::Mdbx(secondary_store)) => primary_store
                .commit_coordinated_overlays_with_required_marker(
                    primary,
                    secondary_store,
                    secondary,
                    marker,
                ),
            _ => Err(StorageError::invalid_operation(
                "coordinated runtime commit requires two MDBX views from one environment",
            )),
        }
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RuntimeSnapshot {
    type FindIterator<'a> = RuntimeSnapshotRawFindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => snapshot.try_get(key),
            RuntimeSnapshotInner::Mdbx(snapshot) => snapshot.try_get(key),
        }
    }

    fn try_get_result(&self, key: &Vec<u8>) -> StorageResult<Option<Vec<u8>>> {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => snapshot.try_get_result(key),
            RuntimeSnapshotInner::Mdbx(snapshot) => snapshot.try_get_result(key),
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
        }
    }
}

impl RawReadOnlyStore for RuntimeSnapshot {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => snapshot.try_get_bytes(key),
            RuntimeSnapshotInner::Mdbx(snapshot) => snapshot.try_get_bytes(key),
        }
    }

    fn try_get_bytes_result(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => snapshot.try_get_bytes_result(key),
            RuntimeSnapshotInner::Mdbx(snapshot) => snapshot.try_get_bytes_result(key),
        }
    }

    fn try_get_many_bytes<K>(&self, keys: &[K]) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => snapshot.try_get_many_bytes(keys),
            RuntimeSnapshotInner::Mdbx(snapshot) => snapshot.try_get_many_bytes(keys),
        }
    }

    fn try_get_many_bytes_sorted<K>(&self, keys: &[K]) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => snapshot.try_get_many_bytes_sorted(keys),
            RuntimeSnapshotInner::Mdbx(snapshot) => snapshot.try_get_many_bytes_sorted(keys),
        }
    }

    fn try_get_many_bytes_sorted_for_write<K>(
        &self,
        keys: &[K],
    ) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        match &self.inner {
            RuntimeSnapshotInner::Memory(snapshot) => {
                snapshot.try_get_many_bytes_sorted_for_write(keys)
            }
            RuntimeSnapshotInner::Mdbx(snapshot) => {
                snapshot.try_get_many_bytes_sorted_for_write(keys)
            }
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

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RuntimeStore {
    type FindIterator<'a> = RuntimeRawFindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        match self {
            Self::Memory(store) => store.try_get(key),
            Self::Mdbx(store) => store.try_get(key),
        }
    }

    fn try_get_result(&self, key: &Vec<u8>) -> StorageResult<Option<Vec<u8>>> {
        match self {
            Self::Memory(store) => store.try_get_result(key),
            Self::Mdbx(store) => store.try_get_result(key),
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
        }
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RuntimeStore {
    type FindIterator<'a> = RuntimeStorageFindIterator<'a>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        match self {
            Self::Memory(store) => store.try_get(key),
            Self::Mdbx(store) => store.try_get(key),
        }
    }

    fn try_get_result(&self, key: &StorageKey) -> StorageResult<Option<StorageItem>> {
        match self {
            Self::Memory(store) => store.try_get_result(key),
            Self::Mdbx(store) => store.try_get_result(key),
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
        }
    }
}

impl RawReadOnlyStore for RuntimeStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self {
            Self::Memory(store) => store.try_get_bytes(key),
            Self::Mdbx(store) => store.try_get_bytes(key),
        }
    }

    fn try_get_bytes_result(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        match self {
            Self::Memory(store) => store.try_get_bytes_result(key),
            Self::Mdbx(store) => store.try_get_bytes_result(key),
        }
    }

    fn try_get_many_bytes<K>(&self, keys: &[K]) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        match self {
            Self::Memory(store) => store.try_get_many_bytes(keys),
            Self::Mdbx(store) => store.try_get_many_bytes(keys),
        }
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for RuntimeStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.delete(key),
            Self::Mdbx(store) => store.delete(key),
        }
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.put(key, value),
            Self::Mdbx(store) => store.put(key, value),
        }
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.put_sync(key, value),
            Self::Mdbx(store) => store.put_sync(key, value),
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
        }
    }

    fn flush(&self) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.flush(),
            Self::Mdbx(store) => store.flush(),
        }
    }

    fn backend_kind(&self) -> StoreBackendKind {
        match self {
            Self::Memory(store) => store.backend_kind(),
            Self::Mdbx(store) => store.backend_kind(),
        }
    }

    fn mdbx_environment_info(&self) -> Option<StorageResult<MdbxEnvironmentInfo>> {
        match self {
            Self::Memory(store) => store.mdbx_environment_info(),
            Self::Mdbx(store) => store.mdbx_environment_info(),
        }
    }

    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> StorageResult<bool> {
        match self {
            Self::Memory(store) => store.try_commit_raw_overlay(overlay),
            Self::Mdbx(store) => store.try_commit_raw_overlay(overlay),
        }
    }

    fn try_commit_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        match self {
            Self::Memory(store) => store.try_commit_borrowed_raw_overlay(overlay),
            Self::Mdbx(store) => store.try_commit_borrowed_raw_overlay(overlay),
        }
    }

    fn supports_raw_overlay_cursor(&self) -> bool {
        match self {
            Self::Memory(store) => store.supports_raw_overlay_cursor(),
            Self::Mdbx(store) => store.supports_raw_overlay_cursor(),
        }
    }
}

impl TransactionalStore for RuntimeStore {
    fn commit_canonical_overlay<O>(&self, overlay: &mut O) -> StorageResult<()>
    where
        O: RawOverlaySource + ?Sized,
    {
        match self {
            Self::Memory(store) => store.commit_canonical_overlay(overlay),
            Self::Mdbx(store) => store.commit_canonical_overlay(overlay),
        }
    }

    fn maintenance_metadata(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        match self {
            Self::Memory(store) => store.maintenance_metadata(key),
            Self::Mdbx(store) => store.maintenance_metadata(key),
        }
    }

    fn commit_maintenance(&self, batch: &StoreMaintenanceBatch) -> StorageResult<()> {
        match self {
            Self::Memory(store) => store.commit_maintenance(batch),
            Self::Mdbx(store) => store.commit_maintenance(batch),
        }
    }
}
