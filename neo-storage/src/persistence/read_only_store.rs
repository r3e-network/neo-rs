use super::seek_direction::SeekDirection;
use crate::types::{StorageItem, StorageKey};

/// This interface provides methods to read from the database.
pub trait ReadOnlyStore: ReadOnlyStoreGeneric<StorageKey, StorageItem> {
    /// Scans at most `limit` entries inside one key namespace.
    ///
    /// `key_prefix` bounds the namespace: iteration stops as soon as a key stops
    /// matching it, so a scan can never cross a contract boundary. `start` is an
    /// exclusive cursor: a [`SeekDirection::Forward`] scan returns only keys
    /// greater than `start`, a [`SeekDirection::Backward`] scan only keys less
    /// than it.
    ///
    /// This is the bounded counterpart of [`ReadOnlyStoreGeneric::find`].
    /// Speculative readers must never force a backend to materialise an
    /// unbounded prefix range that an outer `take` then discards, so backends
    /// that build their scan results eagerly override this method and stop at
    /// the source. The default implementation walks [`ReadOnlyStoreGeneric::find`]
    /// and stops at `limit`.
    fn seek_limited(
        &self,
        key_prefix: Option<&StorageKey>,
        start: Option<&StorageKey>,
        direction: SeekDirection,
        limit: usize,
    ) -> Vec<(StorageKey, StorageItem)> {
        if limit == 0 {
            return Vec::new();
        }

        let prefix_bytes = key_prefix.map(|key| key.as_bytes().into_owned());
        let mut entries = Vec::new();
        for (key, item) in self.find(key_prefix, direction) {
            if let Some(prefix) = prefix_bytes.as_deref()
                && !key.as_bytes().starts_with(prefix)
            {
                break;
            }
            if let Some(start) = start {
                let ordered = match direction {
                    SeekDirection::Forward => key <= *start,
                    SeekDirection::Backward => key >= *start,
                };
                if ordered {
                    continue;
                }
            }
            entries.push((key, item));
            if entries.len() == limit {
                break;
            }
        }
        entries
    }
}

/// This interface provides methods to read from the database (generic version).
pub trait ReadOnlyStoreGeneric<TKey, TValue>
where
    TKey: Clone,
    TValue: Clone,
{
    /// Reads a specified entry from the database.
    /// Returns the data of the entry, or None if it doesn't exist.
    fn try_get(&self, key: &TKey) -> Option<TValue>;

    /// Determines whether the database contains the specified entry.
    fn contains(&self, key: &TKey) -> bool {
        self.try_get(key).is_some()
    }

    /// Finds the entries starting with the specified prefix.
    fn find(
        &self,
        key_prefix: Option<&TKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (TKey, TValue)> + '_>;

    /// Gets the entry with the specified key, returning `None` if absent.
    fn get(&self, key: &TKey) -> Option<TValue> {
        self.try_get(key)
    }
}
