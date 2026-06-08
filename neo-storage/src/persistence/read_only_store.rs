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
