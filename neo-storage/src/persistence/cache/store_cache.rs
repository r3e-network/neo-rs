//! Cache facade that fronts a `Store` or snapshot for smart-contract storage.

use super::{
    data_cache::{CacheRead, DataCache, DataCacheConfig, DataCacheError, DataCacheResult},
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::Store,
    store_snapshot::StoreSnapshot,
    track_state::TrackState,
};
use crate::error::StorageResult;
use crate::persistence::providers::memory_store::MemoryStore;
use crate::types::{StorageItem, StorageKey};
use std::sync::Arc;
use tracing::warn;

/// Concrete read source carried by [`StoreCache`].
#[derive(Debug)]
pub enum StoreCacheBacking<S: Store> {
    /// Reads directly from a shared store.
    Store(Arc<S>),
    /// Reads from a point-in-time backend snapshot.
    Snapshot(Arc<S::Snapshot>),
}

impl<S: Store> Clone for StoreCacheBacking<S> {
    fn clone(&self) -> Self {
        match self {
            Self::Store(store) => Self::Store(Arc::clone(store)),
            Self::Snapshot(snapshot) => Self::Snapshot(Arc::clone(snapshot)),
        }
    }
}

impl<S: Store> CacheRead for StoreCacheBacking<S> {
    fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        match self {
            Self::Store(store) => {
                <S as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::try_get(store.as_ref(), key)
            }
            Self::Snapshot(snapshot) => {
                let key = key.to_array();
                <S::Snapshot as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::try_get(
                    snapshot.as_ref(),
                    &key,
                )
                .map(StorageItem::from_bytes)
            }
        }
    }

    fn find(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Option<Vec<(StorageKey, StorageItem)>> {
        let entries = match self {
            Self::Store(store) => <S as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::find(
                store.as_ref(),
                prefix,
                direction,
            )
            .collect(),
            Self::Snapshot(snapshot) => {
                let prefix = prefix.map(StorageKey::to_array);
                <S::Snapshot as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::find(
                    snapshot.as_ref(),
                    prefix.as_ref(),
                    direction,
                )
                .map(|(key, value)| (StorageKey::from_bytes(&key), StorageItem::from_bytes(value)))
                .collect()
            }
        };
        Some(entries)
    }
}

/// Read cache type produced by a concrete storage backend.
pub type StoreDataCache<S> = DataCache<StoreCacheBacking<S>>;

/// Read-through contract storage cache with Neo-style change tracking.
pub struct StoreCache<S: Store = MemoryStore> {
    data_cache: StoreDataCache<S>,
    backing: StoreCacheBacking<S>,
}

impl<S> StoreCache<S>
where
    S: Store + 'static,
{
    /// Initializes a new instance of the StoreCache class with a store.
    pub fn new_from_store(store: Arc<S>, read_only: bool) -> Self {
        Self::new_from_store_with_config(store, read_only, DataCacheConfig::default())
    }

    /// Initializes a new instance with a store and custom configuration.
    pub fn new_from_store_with_config(
        store: Arc<S>,
        read_only: bool,
        config: DataCacheConfig,
    ) -> Self {
        let backing = StoreCacheBacking::Store(store);
        Self {
            data_cache: DataCache::with_backing(read_only, backing.clone(), config),
            backing,
        }
    }

    /// Provides read-only access to the underlying in-memory data cache.
    pub fn data_cache(&self) -> &StoreDataCache<S> {
        &self.data_cache
    }

    /// Commits all changes.
    pub fn commit(&mut self) {
        if let Err(err) = self.try_commit() {
            warn!(target: "neo", error = ?err, "store cache commit failed");
        }
    }

    /// Commits all changes, returning an error if read-only.
    pub fn try_commit(&mut self) -> DataCacheResult {
        if self.data_cache.is_read_only() {
            return Err(DataCacheError::ReadOnly);
        }

        if self.data_cache.pending_change_count() == 0 {
            self.data_cache.commit();
            return Ok(());
        }

        if self.try_commit_store_overlay()? {
            return Ok(());
        }

        let mut writer_snapshot = match &self.backing {
            StoreCacheBacking::Store(store) => store.snapshot(),
            StoreCacheBacking::Snapshot(snapshot) => snapshot.store().snapshot(),
        };

        if let Some(snapshot) = Arc::get_mut(&mut writer_snapshot) {
            let mut apply_result = Ok(());
            self.data_cache.visit_tracked_items(|key, trackable| {
                if apply_result.is_ok() {
                    apply_result = apply_tracked_item(key, trackable, snapshot);
                }
            });
            apply_result.map_err(|e| {
                DataCacheError::CommitFailed(format!("storage write failed: {}", e))
            })?;
            snapshot
                .try_commit()
                .map_err(|e| DataCacheError::CommitFailed(e.to_string()))?;
            self.data_cache.commit();
        } else {
            let msg = "unable to obtain mutable snapshot for commit; changes not persisted";
            warn!(target: "neo", "{msg}");
            return Err(DataCacheError::CommitFailed(msg.to_string()));
        }
        Ok(())
    }

    /// Commits the canonical overlay as one backend durability boundary.
    ///
    /// The backing store must implement atomic durable-overlay commit whenever
    /// this cache has pending canonical changes. MDBX uses one write
    /// transaction. RocksDB first persists any earlier fast-sync prefix and
    /// then writes this overlay with synchronous WAL. Callers that publish a
    /// canonical tip must use this method rather than treating
    /// [`Self::try_commit`] or commit-then-flush as a durability fence.
    pub fn try_commit_durable(&mut self) -> DataCacheResult {
        if self.data_cache.is_read_only() {
            return Err(DataCacheError::ReadOnly);
        }

        if self.data_cache.pending_change_count() > 0 {
            match self.try_commit_durable_store_overlay() {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    self.discard_pending_changes();
                    return Err(DataCacheError::CommitFailed(
                        "storage backend does not support atomic durable overlay commits"
                            .to_string(),
                    ));
                }
                Err(error) => {
                    self.discard_pending_changes();
                    return Err(error);
                }
            }
        }

        let flush_result = match &self.backing {
            StoreCacheBacking::Store(store) => store.flush(),
            StoreCacheBacking::Snapshot(snapshot) => snapshot.store().flush(),
        };
        if let Err(error) = flush_result {
            self.discard_pending_changes();
            return Err(DataCacheError::CommitFailed(format!(
                "storage flush failed: {error}"
            )));
        }
        Ok(())
    }

    fn try_commit_durable_store_overlay(&self) -> DataCacheResult<bool> {
        let StoreCacheBacking::Store(store) = &self.backing else {
            return Ok(false);
        };

        let mut source = &self.data_cache;
        let committed = store
            .try_commit_durable_borrowed_raw_overlay(&mut source)
            .map_err(|error| {
                DataCacheError::CommitFailed(format!("durable storage write failed: {error}"))
            })?;
        if committed {
            self.data_cache.commit();
        }
        Ok(committed)
    }

    /// Discards changes that have not reached a durable backend commit.
    ///
    /// This is the failure-side counterpart to [`Self::try_commit_durable`]. It clears
    /// the canonical overlay and any backend-specific fast-sync buffer so the
    /// next read observes the last successfully committed snapshot.
    pub fn discard_pending_changes(&self) {
        self.data_cache.reset();
        match &self.backing {
            StoreCacheBacking::Store(store) => store.discard_pending_fast_sync_writes(),
            StoreCacheBacking::Snapshot(snapshot) => {
                snapshot.store().discard_pending_fast_sync_writes();
            }
        }
    }

    fn try_commit_store_overlay(&self) -> DataCacheResult<bool> {
        let StoreCacheBacking::Store(store) = &self.backing else {
            return Ok(false);
        };

        let mut source = &self.data_cache;
        let committed = store
            .try_commit_borrowed_raw_overlay(&mut source)
            .map_err(|e| DataCacheError::CommitFailed(format!("storage write failed: {e}")))?;
        if committed {
            self.data_cache.commit();
            return Ok(true);
        }

        let overlay = self.data_cache.extract_raw_changes();
        let committed = store
            .try_commit_raw_overlay(&overlay)
            .map_err(|e| DataCacheError::CommitFailed(format!("storage write failed: {e}")))?;
        if committed {
            self.data_cache.commit();
        }
        Ok(committed)
    }

    /// Gets an item from the cache or underlying store.
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.data_cache.get(key)
    }

    /// Adds an item to the cache.
    pub fn add(&mut self, key: StorageKey, value: StorageItem) {
        let _ = self.try_add(key, value);
    }

    /// Adds an item to the cache, returning an error if the cache is read-only.
    pub fn try_add(&mut self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.data_cache.try_add(key, value)
    }

    /// Updates an item in the cache.
    pub fn update(&mut self, key: StorageKey, value: StorageItem) {
        let _ = self.try_update(key, value);
    }

    /// Updates an item in the cache, returning an error if the cache is read-only.
    pub fn try_update(&mut self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.data_cache.try_update(key, value)
    }

    /// Deletes an item from the cache.
    pub fn delete(&mut self, key: StorageKey) {
        let _ = self.try_delete(key);
    }

    /// Deletes an item from the cache, returning an error if the cache is read-only.
    pub fn try_delete(&mut self, key: StorageKey) -> DataCacheResult {
        self.data_cache.try_delete(&key)
    }

    /// Finds items by key prefix.
    pub fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> std::vec::IntoIter<(StorageKey, StorageItem)> {
        self.data_cache.find(key_prefix, direction)
    }

    /// Applies a tracked change set onto this cache.
    pub fn apply_tracked_items(
        &mut self,
        tracked: Vec<(StorageKey, super::data_cache::Trackable)>,
    ) {
        for (key, trackable) in tracked {
            match trackable.state {
                TrackState::Added => self.add(key, trackable.item),
                TrackState::Changed => self.update(key, trackable.item),
                TrackState::Deleted => self.delete(key),
                TrackState::None | TrackState::NotFound => {}
            }
        }
    }
}

impl<S> StoreCache<S>
where
    S: Store + 'static,
{
    /// Initializes a new instance of the StoreCache class with a snapshot.
    pub fn new_from_snapshot(snapshot: Arc<S::Snapshot>) -> Self {
        Self::new_from_snapshot_with_config(snapshot, DataCacheConfig::default())
    }

    /// Initializes a new instance with a snapshot and custom cache configuration.
    pub fn new_from_snapshot_with_config(
        snapshot: Arc<S::Snapshot>,
        config: DataCacheConfig,
    ) -> Self {
        let backing = StoreCacheBacking::Snapshot(snapshot);
        Self {
            data_cache: DataCache::with_backing(false, backing.clone(), config),
            backing,
        }
    }
}

/// Applies tracked cache entries to a raw byte-oriented writer.
pub fn apply_tracked<T>(
    tracked: &[(StorageKey, super::data_cache::Trackable)],
    writer: &mut T,
) -> StorageResult<()>
where
    T: super::write_store::WriteStore<Vec<u8>, Vec<u8>> + ?Sized,
{
    for (key, trackable) in tracked {
        apply_tracked_item(key, trackable, writer)?;
    }
    Ok(())
}

fn apply_tracked_item<T>(
    key: &StorageKey,
    trackable: &super::data_cache::Trackable,
    writer: &mut T,
) -> StorageResult<()>
where
    T: super::write_store::WriteStore<Vec<u8>, Vec<u8>> + ?Sized,
{
    match trackable.state {
        TrackState::Added | TrackState::Changed => {
            writer.put(key.to_array(), trackable.item.to_value())?;
        }
        TrackState::Deleted => writer.delete(key.to_array())?,
        TrackState::None | TrackState::NotFound => {}
    }
    Ok(())
}

impl<S> ReadOnlyStore for StoreCache<S> where S: Store + 'static {}

impl<S> ReadOnlyStoreGeneric<StorageKey, StorageItem> for StoreCache<S>
where
    S: Store + 'static,
{
    type FindIterator<'a>
        = std::vec::IntoIter<(StorageKey, StorageItem)>
    where
        Self: 'a;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        self.data_cache.find(key_prefix, direction)
    }
}

#[cfg(test)]
#[path = "../../tests/persistence/store_cache.rs"]
mod tests;
