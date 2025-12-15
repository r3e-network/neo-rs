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

    #[test]
    fn test_get_returns_error_on_missing_key() {
        let store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let result = IReadOnlyStore::get(&store, &key);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_returns_ok_on_existing_key() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![0xAA]);
        store.put(key.clone(), value.clone());

        let result = IReadOnlyStore::get(&store, &key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().value(), value.value());
    }

    #[test]
    fn test_multiple_keys() {
        let mut store = MemoryStore::new();
        let key1 = StorageKey::new(-1, vec![0x01]);
        let key2 = StorageKey::new(-1, vec![0x02]);
        let key3 = StorageKey::new(-2, vec![0x01]);

        let value1 = StorageItem::new(vec![0xAA]);
        let value2 = StorageItem::new(vec![0xBB]);
        let value3 = StorageItem::new(vec![0xCC]);

        store.put(key1.clone(), value1.clone());
        store.put(key2.clone(), value2.clone());
        store.put(key3.clone(), value3.clone());

        assert_eq!(IReadOnlyStore::try_get(&store, &key1).unwrap().value(), value1.value());
        assert_eq!(IReadOnlyStore::try_get(&store, &key2).unwrap().value(), value2.value());
        assert_eq!(IReadOnlyStore::try_get(&store, &key3).unwrap().value(), value3.value());
    }

    #[test]
    fn test_overwrite_existing_key() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value1 = StorageItem::new(vec![0xAA]);
        let value2 = StorageItem::new(vec![0xBB]);

        store.put(key.clone(), value1);
        store.put(key.clone(), value2.clone());

        let retrieved = IReadOnlyStore::try_get(&store, &key).unwrap();
        assert_eq!(retrieved.value(), value2.value());
    }

    #[test]
    fn test_delete_nonexistent_key() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        // Should not panic
        store.delete(&key);
        assert!(!IReadOnlyStore::contains(&store, &key));
    }

    #[test]
    fn test_delete_then_reinsert() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value1 = StorageItem::new(vec![0xAA]);
        let value2 = StorageItem::new(vec![0xBB]);

        store.put(key.clone(), value1);
        store.delete(&key);
        assert!(!IReadOnlyStore::contains(&store, &key));

        store.put(key.clone(), value2.clone());
        assert!(IReadOnlyStore::contains(&store, &key));
        assert_eq!(IReadOnlyStore::try_get(&store, &key).unwrap().value(), value2.value());
    }

    #[test]
    fn test_empty_value() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![]);

        store.put(key.clone(), value.clone());
        assert!(IReadOnlyStore::contains(&store, &key));
        let empty: &[u8] = &[];
        assert_eq!(IReadOnlyStore::try_get(&store, &key).unwrap().value(), empty);
    }

    #[test]
    fn test_large_value() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let large_data = vec![0xAA; 10000];
        let value = StorageItem::new(large_data.clone());

        store.put(key.clone(), value);
        let retrieved = IReadOnlyStore::try_get(&store, &key).unwrap();
        assert_eq!(retrieved.value(), &large_data[..]);
    }

    #[test]
    fn test_generic_trait_implementation() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![0xAA]);

        store.put(key.clone(), value.clone());

        // Test generic trait
        assert!(IReadOnlyStoreGeneric::contains(&store, &key));
        assert_eq!(
            IReadOnlyStoreGeneric::try_get(&store, &key).unwrap().value(),
            value.value()
        );
    }

    #[test]
    fn test_constant_storage_item() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::constant(vec![0xAA]);

        store.put(key.clone(), value.clone());
        let retrieved = IReadOnlyStore::try_get(&store, &key).unwrap();
        assert!(retrieved.is_constant());
    }

    #[test]
    fn test_negative_contract_ids() {
        let mut store = MemoryStore::new();
        // Native contracts use negative IDs
        let key1 = StorageKey::new(-1, vec![0x01]); // NeoToken
        let key2 = StorageKey::new(-4, vec![0x01]); // GasToken
        let key3 = StorageKey::new(-6, vec![0x01]); // PolicyContract

        let value = StorageItem::new(vec![0xAA]);

        store.put(key1.clone(), value.clone());
        store.put(key2.clone(), value.clone());
        store.put(key3.clone(), value.clone());

        assert!(IReadOnlyStore::contains(&store, &key1));
        assert!(IReadOnlyStore::contains(&store, &key2));
        assert!(IReadOnlyStore::contains(&store, &key3));
    }

    #[test]
    fn test_storage_key_with_empty_suffix() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![]);
        let value = StorageItem::new(vec![0xAA]);

        store.put(key.clone(), value.clone());
        assert!(IReadOnlyStore::contains(&store, &key));
    }

    #[test]
    fn test_try_get_returns_none() {
        let store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        assert!(IReadOnlyStore::try_get(&store, &key).is_none());
    }

    #[test]
    fn test_try_get_returns_some() {
        let mut store = MemoryStore::new();
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![0xAA]);

        store.put(key.clone(), value);
        assert!(IReadOnlyStore::try_get(&store, &key).is_some());
    }
}
