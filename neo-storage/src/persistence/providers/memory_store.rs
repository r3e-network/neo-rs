use super::memory_snapshot::MemorySnapshot;
use crate::impl_default_via_new;
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
use std::ops::Bound;
use std::sync::Arc;

/// Shared immutable map used by snapshots. Writes take a copy-on-write path
/// only when a snapshot still holds the previous map.
type SharedMap = Arc<BTreeMap<Vec<u8>, Vec<u8>>>;

/// Bounds for a byte-prefix range in a `BTreeMap`.
pub(crate) fn prefix_bounds(prefix: &[u8]) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    let start = Bound::Included(prefix.to_vec());
    let end = prefix
        .iter()
        .rposition(|byte| *byte != u8::MAX)
        .map_or(Bound::Unbounded, |index| {
            let mut upper = prefix[..=index].to_vec();
            upper[index] = upper[index].saturating_add(1);
            Bound::Excluded(upper)
        });
    (start, end)
}

/// An in-memory Store implementation that uses BTreeMap as the underlying storage.
pub struct MemoryStore {
    inner_data: Arc<RwLock<SharedMap>>,
    on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
}

impl MemoryStore {
    /// Creates a new MemoryStore.
    pub fn new() -> Self {
        Self {
            inner_data: Arc::new(RwLock::new(Arc::new(BTreeMap::new()))),
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Resets the store, clearing all data.
    pub fn reset(&self) {
        *self.inner_data.write() = Arc::new(BTreeMap::new());
    }
}

impl_default_via_new!(MemoryStore);

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

        // Use the BTreeMap range for the prefix instead of scanning the whole
        // store. This keeps prefix scans proportional to the matching range.
        let mut entries: Vec<_> = match key_prefix {
            Some(prefix) => {
                let (start, end) = prefix_bounds(prefix);
                data.range((start, end))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            }
            None => data.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        };

        if direction == SeekDirection::Backward {
            entries.reverse();
        }

        Box::new(entries.into_iter())
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
        let mut guard = self.inner_data.write();
        Arc::make_mut(&mut guard).remove(&key);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> crate::error::StorageResult<()> {
        let mut guard = self.inner_data.write();
        Arc::make_mut(&mut guard).insert(key, value);
        Ok(())
    }
}

impl ReadOnlyStore for MemoryStore {}

impl Store for MemoryStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        let snapshot = Arc::new(MemorySnapshot::new(
            Arc::new(self.clone()),
            Arc::clone(&self.inner_data.read()),
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
        let data = Arc::make_mut(&mut guard);
        for (key, value) in batch.iter() {
            match value {
                Some(v) => {
                    data.insert(key.clone(), v.clone());
                }
                None => {
                    data.remove(key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{ReadOnlyStoreGeneric, Store, StoreSnapshot};

    #[test]
    fn prefix_find_uses_range_and_preserves_order() {
        let mut store = MemoryStore::new();
        for key in [vec![1u8], vec![1, 2], vec![2]] {
            store.put(key.clone(), key).unwrap();
        }

        let forward: Vec<_> = store.find(Some(&vec![1]), SeekDirection::Forward).collect();
        assert_eq!(forward, vec![(vec![1], vec![1]), (vec![1, 2], vec![1, 2])]);

        let backward: Vec<_> = store
            .find(Some(&vec![1]), SeekDirection::Backward)
            .collect();
        assert_eq!(backward, vec![(vec![1, 2], vec![1, 2]), (vec![1], vec![1])]);
    }

    #[test]
    fn snapshot_isolated_from_later_store_writes() {
        let mut store = MemoryStore::new();
        store.put(vec![1], vec![1]).unwrap();
        let snapshot: Arc<dyn StoreSnapshot> = store.snapshot();
        store.put(vec![2], vec![2]).unwrap();

        assert_eq!(snapshot.try_get(&vec![1]), Some(vec![1]));
        assert_eq!(snapshot.try_get(&vec![2]), None);
    }
}
