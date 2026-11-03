use super::data_cache::{DataCache, DataCacheConfig, DataCacheResult, StoreFindFn, StoreGetFn};
use super::seek_direction::SeekDirection;
use crate::types::{StorageItem, StorageKey};
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

/// Immutable storage state copied away from a live store.
#[derive(Clone, Debug)]
pub struct DetachedSnapshot {
    entries: Arc<BTreeMap<Vec<u8>, DetachedValue>>,
    encoded_bytes: usize,
}

#[derive(Clone, Debug)]
struct DetachedValue {
    bytes: Vec<u8>,
    constant: bool,
}

/// One bounded entry used to construct a detached snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetachedSnapshotEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
    pub constant: bool,
}

/// Snapshot export limits enforced before any detached state is allocated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetachedSnapshotLimits {
    pub max_entries: usize,
    pub max_bytes: usize,
}

/// Errors returned while constructing detached state.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DetachedSnapshotError {
    #[error("detached snapshot limits must be non-zero")]
    InvalidLimits,
    #[error("detached snapshot contains an invalid storage key")]
    InvalidKey,
    #[error("detached snapshot contains a duplicate storage key")]
    DuplicateKey,
    #[error("detached snapshot exceeds its entry or byte limit")]
    LimitExceeded,
}

impl DetachedSnapshot {
    /// Copies entries into an immutable snapshot with no live-store handle or commit API.
    pub fn try_from_entries(
        entries: impl IntoIterator<Item = DetachedSnapshotEntry>,
        limits: DetachedSnapshotLimits,
    ) -> Result<Self, DetachedSnapshotError> {
        if limits.max_entries == 0 || limits.max_bytes == 0 {
            return Err(DetachedSnapshotError::InvalidLimits);
        }
        let mut detached = BTreeMap::new();
        let mut encoded_bytes = 0usize;
        for entry in entries {
            if entry.key.len() < 4 {
                return Err(DetachedSnapshotError::InvalidKey);
            }
            if detached.len() >= limits.max_entries {
                return Err(DetachedSnapshotError::LimitExceeded);
            }
            encoded_bytes = encoded_bytes
                .checked_add(entry.key.len())
                .and_then(|size| size.checked_add(entry.value.len()))
                .and_then(|size| size.checked_add(1))
                .ok_or(DetachedSnapshotError::LimitExceeded)?;
            if encoded_bytes > limits.max_bytes {
                return Err(DetachedSnapshotError::LimitExceeded);
            }
            if detached
                .insert(
                    entry.key,
                    DetachedValue {
                        bytes: entry.value,
                        constant: entry.constant,
                    },
                )
                .is_some()
            {
                return Err(DetachedSnapshotError::DuplicateKey);
            }
        }
        Ok(Self {
            entries: Arc::new(detached),
            encoded_bytes,
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub const fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }

    /// Returns a detached value without exposing mutable state.
    #[must_use]
    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.entries.get(key).map(|value| value.bytes.clone())
    }

    /// Creates a disposable writable overlay whose backing data is detached memory only.
    #[must_use]
    pub fn disposable_overlay(&self) -> DisposableOverlay {
        let get_entries = Arc::clone(&self.entries);
        let find_entries = Arc::clone(&self.entries);
        let store_get: Arc<StoreGetFn> = Arc::new(move |key: &StorageKey| {
            get_entries
                .get(key.as_bytes().as_ref())
                .map(detached_storage_item)
        });
        let store_find: Arc<StoreFindFn> = Arc::new(move |prefix, direction| {
            let prefix = prefix.map(|key| key.as_bytes().into_owned());
            let mut values = find_entries
                .iter()
                .filter(|(key, _)| prefix.as_ref().is_none_or(|prefix| key.starts_with(prefix)))
                .map(|(key, value)| (StorageKey::from_bytes(key), detached_storage_item(value)))
                .collect::<Vec<_>>();
            if direction == SeekDirection::Backward {
                values.reverse();
            }
            values
        });
        let config = DataCacheConfig {
            enable_read_cache: false,
            enable_prefetching: false,
            track_reads_in_write_cache: false,
            ..DataCacheConfig::default()
        };
        DisposableOverlay {
            cache: Arc::new(DataCache::new_with_config(
                false,
                Some(store_get),
                Some(store_find),
                config,
            )),
        }
    }
}

fn detached_storage_item(value: &DetachedValue) -> StorageItem {
    if value.constant {
        StorageItem::constant(value.bytes.clone())
    } else {
        StorageItem::from_bytes(value.bytes.clone())
    }
}

/// Writable in-memory overlay that cannot reach or commit a live store.
pub struct DisposableOverlay {
    cache: Arc<DataCache>,
}

impl DisposableOverlay {
    /// Reads through the detached backing state and this disposable overlay.
    #[must_use]
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.cache.get(key)
    }

    /// Adds or replaces an overlay value without exposing a commit-capable cache handle.
    pub fn upsert(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        if self.cache.get(&key).is_some() {
            self.cache.try_update(key, value)
        } else {
            self.cache.try_add(key, value)
        }
    }

    /// Returns detached and overlaid values in deterministic seek order.
    #[must_use]
    pub fn find(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Vec<(StorageKey, StorageItem)> {
        self.cache.find(prefix, direction).collect()
    }

    #[must_use]
    pub fn modified_count(&self) -> usize {
        self.cache.pending_change_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(max_entries: usize, max_bytes: usize) -> DetachedSnapshotLimits {
        DetachedSnapshotLimits {
            max_entries,
            max_bytes,
        }
    }

    fn entry(key: Vec<u8>, value: Vec<u8>) -> DetachedSnapshotEntry {
        DetachedSnapshotEntry {
            key,
            value,
            constant: false,
        }
    }

    #[test]
    fn overlay_mutation_never_changes_detached_snapshot() {
        let key = StorageKey::new(7, vec![1, 2, 3]).to_array();
        let snapshot =
            DetachedSnapshot::try_from_entries([entry(key.clone(), vec![4, 5])], limits(2, 64))
                .unwrap();
        let overlay = snapshot.disposable_overlay();
        overlay
            .upsert(
                StorageKey::from_bytes(&key),
                StorageItem::from_bytes(vec![9]),
            )
            .unwrap();
        assert_eq!(snapshot.get(&key), Some(vec![4, 5]));
        assert_eq!(
            overlay.get(&StorageKey::from_bytes(&key)).unwrap().value(),
            &[9]
        );
        assert_eq!(overlay.modified_count(), 1);
        assert_eq!(snapshot.get(&key), Some(vec![4, 5]));
    }

    #[test]
    fn snapshot_limits_and_duplicates_fail_closed() {
        let duplicate = entry(vec![0, 0, 0, 0], vec![1]);
        assert_eq!(
            DetachedSnapshot::try_from_entries([duplicate.clone(), duplicate], limits(2, 32),)
                .unwrap_err(),
            DetachedSnapshotError::DuplicateKey
        );
        assert_eq!(
            DetachedSnapshot::try_from_entries([entry(vec![0, 0, 0, 0], vec![1])], limits(0, 32),)
                .unwrap_err(),
            DetachedSnapshotError::InvalidLimits
        );
        assert_eq!(
            DetachedSnapshot::try_from_entries([entry(vec![0, 0, 0], vec![1])], limits(1, 32),)
                .unwrap_err(),
            DetachedSnapshotError::InvalidKey
        );
        assert_eq!(
            DetachedSnapshot::try_from_entries(
                [
                    entry(vec![0, 0, 0, 0], vec![1]),
                    entry(vec![0, 0, 0, 1], vec![2]),
                ],
                limits(1, 32),
            )
            .unwrap_err(),
            DetachedSnapshotError::LimitExceeded
        );
        assert_eq!(
            DetachedSnapshot::try_from_entries([entry(vec![0, 0, 0, 0], vec![1])], limits(1, 5),)
                .unwrap_err(),
            DetachedSnapshotError::LimitExceeded
        );
    }

    #[test]
    fn detached_values_preserve_constants_and_seek_order() {
        let first = StorageKey::new(9, vec![1]).to_array();
        let second = StorageKey::new(9, vec![2]).to_array();
        let snapshot = DetachedSnapshot::try_from_entries(
            [
                DetachedSnapshotEntry {
                    key: second.clone(),
                    value: vec![20],
                    constant: true,
                },
                entry(first.clone(), vec![10]),
            ],
            limits(2, 64),
        )
        .unwrap();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot.encoded_bytes(), 14);

        let overlay = snapshot.disposable_overlay();
        let constant = overlay.get(&StorageKey::from_bytes(&second)).unwrap();
        assert!(constant.is_constant());
        let backward = overlay
            .find(None, SeekDirection::Backward)
            .into_iter()
            .map(|(key, _)| key.to_array())
            .collect::<Vec<_>>();
        assert_eq!(backward, vec![second, first]);
    }
}
