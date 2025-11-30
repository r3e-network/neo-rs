//! Storage trait definitions for Neo blockchain.
//!
//! These traits define the interface for storage operations, allowing
//! different backends to be used interchangeably.

use crate::error::StorageResult;
use crate::types::{SeekDirection, StorageItem, StorageKey};

/// Read-only storage interface.
///
/// Provides methods for reading data from storage without modification.
pub trait IReadOnlyStore {
    /// Tries to get a value by key.
    ///
    /// Returns `Some(item)` if the key exists, `None` otherwise.
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem>;

    /// Gets a value by key, returning an error if not found.
    fn get(&self, key: &StorageKey) -> StorageResult<StorageItem> {
        self.try_get(key)
            .ok_or_else(|| crate::error::StorageError::key_not_found(format!("{:?}", key)))
    }

    /// Checks if a key exists in storage.
    fn contains(&self, key: &StorageKey) -> bool {
        self.try_get(key).is_some()
    }
}

/// Write storage interface.
///
/// Provides methods for modifying data in storage.
pub trait IWriteStore {
    /// Puts a value into storage.
    fn put(&mut self, key: StorageKey, value: StorageItem);

    /// Deletes a value from storage.
    fn delete(&mut self, key: &StorageKey);
}

/// Combined read/write storage interface.
///
/// Combines [`IReadOnlyStore`] and [`IWriteStore`] for full storage access.
pub trait IStore: IReadOnlyStore + IWriteStore {}

// Blanket implementation for any type that implements both traits
impl<T: IReadOnlyStore + IWriteStore> IStore for T {}

/// Snapshot interface for point-in-time storage views.
///
/// Provides methods for creating and working with storage snapshots.
pub trait ISnapshot: IReadOnlyStore {
    /// Creates a new snapshot of the current storage state.
    fn snapshot(&self) -> Box<dyn ISnapshot>;

    /// Seeks to entries matching a prefix.
    ///
    /// Returns an iterator over (key, value) pairs.
    fn seek(
        &self,
        prefix: &StorageKey,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_>;

    /// Finds all entries matching a prefix.
    fn find(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_>;
}

/// Generic read-only store trait with type parameters.
///
/// This allows for more flexible storage implementations with custom key/value types.
pub trait IReadOnlyStoreGeneric<K, V> {
    /// Tries to get a value by key.
    fn try_get(&self, key: &K) -> Option<V>;

    /// Checks if a key exists.
    fn contains(&self, key: &K) -> bool {
        self.try_get(key).is_some()
    }
}

// Implement generic trait for concrete types
impl<T: IReadOnlyStore> IReadOnlyStoreGeneric<StorageKey, StorageItem> for T {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        IReadOnlyStore::try_get(self, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // Simple in-memory store for testing
    struct MemoryStore {
        data: HashMap<Vec<u8>, StorageItem>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                data: HashMap::new(),
            }
        }
    }

    impl IReadOnlyStore for MemoryStore {
        fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
            self.data.get(&key.to_array()).cloned()
        }
    }

    impl IWriteStore for MemoryStore {
        fn put(&mut self, key: StorageKey, value: StorageItem) {
            self.data.insert(key.to_array(), value);
        }

        fn delete(&mut self, key: &StorageKey) {
            self.data.remove(&key.to_array());
        }
    }

    #[test]
    fn test_memory_store_put_get() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![0xAA, 0xBB]);

        store.put(key.clone(), value.clone());

        let retrieved = IReadOnlyStore::try_get(&store, &key);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().value(), value.value());
    }

    #[test]
    fn test_memory_store_delete() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![0xAA]);

        store.put(key.clone(), value);
        assert!(IReadOnlyStore::contains(&store, &key));

        store.delete(&key);
        assert!(!IReadOnlyStore::contains(&store, &key));
    }

    #[test]
    fn test_memory_store_contains() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);

        assert!(!IReadOnlyStore::contains(&store, &key));

        store.put(key.clone(), StorageItem::new(vec![0x00]));
        assert!(IReadOnlyStore::contains(&store, &key));
    }

    #[test]
    fn test_istore_blanket_impl() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![0xAA]);

        // This tests that MemoryStore implements IStore via blanket impl
        fn use_store<S: IStore>(store: &mut S, key: StorageKey, value: StorageItem) {
            store.put(key.clone(), value);
            assert!(IReadOnlyStore::contains(store, &key));
        }

        use_store(&mut store, key, value);
    }
}
