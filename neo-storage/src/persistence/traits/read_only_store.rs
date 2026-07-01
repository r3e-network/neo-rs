use super::seek_direction::SeekDirection;
use crate::types::{StorageItem, StorageKey};

/// This interface provides methods to read from the database.
pub trait ReadOnlyStore: ReadOnlyStoreGeneric<StorageKey, StorageItem> {}

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

/// Borrowed raw byte-key lookup for backends whose consensus/storage hot paths
/// already hold keys as byte slices.
pub trait RawReadOnlyStore {
    /// Reads a raw byte-key entry without forcing callers to allocate a
    /// temporary `Vec<u8>` just to satisfy the generic store trait.
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>>;
}
