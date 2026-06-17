use super::memory_snapshot::MemorySnapshot;
use crate::persistence::{
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::{OnNewSnapshotDelegate, Store},
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use crate::types::{StorageItem, storage_key::StorageKey};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

/// An in-memory Store implementation that uses BTreeMap as the underlying storage.
pub struct MemoryStore {
    inner_data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
    on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
}

impl MemoryStore {
    /// Creates a new MemoryStore.
    pub fn new() -> Self {
        Self {
            inner_data: Arc::new(RwLock::new(BTreeMap::new())),
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Resets the store, clearing all data.
    pub fn reset(&self) {
        self.inner_data.write().clear();
    }
}

neo_io::impl_default_via_new!(MemoryStore);

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MemoryStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner_data.read().get(key).cloned()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let data = self.inner_data.read();
        let iter: Vec<_> = data
            .iter()
            .filter(|(key, _)| {
                key_prefix
                    .map(|prefix| key.starts_with(prefix))
                    .unwrap_or(true)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if direction == SeekDirection::Backward {
            Box::new(iter.into_iter().rev()) as Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>
        } else {
            Box::new(iter.into_iter()) as Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>
        }
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for MemoryStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw_key = key.to_array();
        self.inner_data
            .read()
            .get(&raw_key)
            .cloned()
            .map(StorageItem::from_bytes)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let data = self.inner_data.read();
        let prefix_bytes = key_prefix.map(|k| k.to_array());

        let mut entries: Vec<_> = data
            .iter()
            .filter(|(key, _)| {
                if let Some(prefix) = prefix_bytes.as_ref() {
                    key.starts_with(prefix)
                } else {
                    true
                }
            })
            .map(|(key, value)| {
                (
                    StorageKey::from_bytes(key),
                    StorageItem::from_bytes(value.clone()),
                )
            })
            .collect();

        if direction == SeekDirection::Backward {
            entries.reverse();
        }

        Box::new(entries.into_iter())
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for MemoryStore {
    fn delete(&mut self, key: Vec<u8>) -> crate::error::StorageResult<()> {
        self.inner_data.write().remove(&key);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> crate::error::StorageResult<()> {
        self.inner_data.write().insert(key, value);
        Ok(())
    }
}

impl ReadOnlyStore for MemoryStore {}

impl Store for MemoryStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        let snapshot = Arc::new(MemorySnapshot::new(
            Arc::new(self.clone()),
            self.inner_data.clone(),
        ));

        // Trigger event
        let handlers = self.on_new_snapshot.read();
        for handler in handlers.iter() {
            handler(self, snapshot.clone());
        }

        snapshot
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.on_new_snapshot.write().push(handler);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        Self {
            inner_data: self.inner_data.clone(),
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl MemoryStore {
    /// Applies a batch of write operations to the underlying store.
    pub fn apply_batch(&self, batch: &std::collections::BTreeMap<Vec<u8>, Option<Vec<u8>>>) {
        let mut guard = self.inner_data.write();
        for (key, value) in batch.iter() {
            match value {
                Some(v) => {
                    guard.insert(key.clone(), v.clone());
                }
                None => {
                    guard.remove(key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::store_cache::StoreCache;

    #[test]
    fn raw_prefix_find_returns_only_matching_rows_in_both_directions() {
        let mut store = MemoryStore::new();
        for (key, value) in [
            (b"a\x00".to_vec(), vec![0x01]),
            (b"a\xff".to_vec(), vec![0x02]),
            (b"b".to_vec(), vec![0x03]),
        ] {
            store.put(key, value).expect("put raw row");
        }

        let prefix = b"a".to_vec();
        let forward_expected = vec![b"a\x00".to_vec(), b"a\xff".to_vec()];
        let backward_expected = vec![b"a\xff".to_vec(), b"a\x00".to_vec()];

        let store_forward_keys: Vec<_> = store
            .find(Some(&prefix), SeekDirection::Forward)
            .map(|(key, _)| key)
            .collect();
        assert_eq!(store_forward_keys, forward_expected);

        let store_keys: Vec<_> = store
            .find(Some(&prefix), SeekDirection::Backward)
            .map(|(key, _)| key)
            .collect();
        assert_eq!(store_keys, backward_expected);

        let snapshot = store.snapshot();
        let snapshot_forward_keys: Vec<_> = snapshot
            .find(Some(&prefix), SeekDirection::Forward)
            .map(|(key, _)| key)
            .collect();
        assert_eq!(snapshot_forward_keys, forward_expected);

        let snapshot_keys: Vec<_> = snapshot
            .find(Some(&prefix), SeekDirection::Backward)
            .map(|(key, _)| key)
            .collect();
        assert_eq!(snapshot_keys, backward_expected);
    }

    #[test]
    fn snapshot_reads_ignore_pending_writes_until_reopened_after_commit() {
        let mut store = MemoryStore::new();
        let existing_key = b"k1".to_vec();
        let added_key = b"k2".to_vec();

        store
            .put(existing_key.clone(), vec![0xAA])
            .expect("put existing row");

        let mut snapshot = store.snapshot();
        {
            let snapshot_mut = Arc::get_mut(&mut snapshot).expect("exclusive snapshot");
            snapshot_mut.delete(existing_key.clone()).unwrap();
            snapshot_mut.put(added_key.clone(), vec![0xBB]).unwrap();
        }

        assert_eq!(snapshot.try_get(&existing_key), Some(vec![0xAA]));
        assert_eq!(snapshot.try_get(&added_key), None);
        let entries: Vec<_> = snapshot.find(None, SeekDirection::Forward).collect();
        assert_eq!(entries, vec![(existing_key.clone(), vec![0xAA])]);

        Arc::get_mut(&mut snapshot)
            .expect("exclusive snapshot")
            .try_commit()
            .expect("snapshot commit");

        assert_eq!(snapshot.try_get(&existing_key), Some(vec![0xAA]));
        assert_eq!(snapshot.try_get(&added_key), None);

        let reopened = store.snapshot();
        assert_eq!(reopened.try_get(&existing_key), None);
        assert_eq!(reopened.try_get(&added_key), Some(vec![0xBB]));
    }

    #[test]
    fn snapshot_backed_store_cache_backward_find_matches_prefix_rows() {
        let mut store = MemoryStore::new();
        let key_a = StorageKey::new(-5, vec![0x1d, 0x00]);
        let key_b = StorageKey::new(-5, vec![0x1d, 0xff]);
        let key_other = StorageKey::new(-5, vec![0x1e, 0x00]);

        for (key, value) in [
            (key_a.to_array(), vec![0x01]),
            (key_b.to_array(), vec![0x02]),
            (key_other.to_array(), vec![0x03]),
        ] {
            store.put(key, value).expect("put storage row");
        }

        let prefix = StorageKey::create(-5, 0x1d);
        let cache = StoreCache::new_from_snapshot(store.snapshot());
        let keys: Vec<_> = cache
            .data_cache()
            .find(Some(&prefix), SeekDirection::Backward)
            .map(|(key, _)| key.to_array())
            .collect();

        assert_eq!(keys, vec![key_b.to_array(), key_a.to_array()]);
    }
}
