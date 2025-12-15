// Copyright (C) 2015-2025 The Neo Project.
//
// contract_storage.rs is free software: you can redistribute it and/or modify
// it under the terms of the MIT License.

//! Contract storage abstraction for Neo N3.

use crate::error::{StateError, StateResult};
use hashbrown::HashMap;
use neo_primitives::UInt160;
use serde::{Deserialize, Serialize};

/// Represents a storage key for a contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey {
    /// The contract script hash.
    pub contract_hash: UInt160,
    /// The storage key bytes.
    pub key: Vec<u8>,
}

impl StorageKey {
    /// Creates a new storage key.
    pub fn new(contract_hash: UInt160, key: Vec<u8>) -> Self {
        Self { contract_hash, key }
    }

    /// Creates a storage key from raw bytes.
    pub fn from_bytes(contract_hash: UInt160, key: &[u8]) -> Self {
        Self {
            contract_hash,
            key: key.to_vec(),
        }
    }

    /// Returns the serialized form of this key.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(20 + self.key.len());
        result.extend_from_slice(&self.contract_hash.to_array());
        result.extend_from_slice(&self.key);
        result
    }
}

/// Represents a storage item value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageItem {
    /// The stored value.
    pub value: Vec<u8>,
    /// Whether this item is constant (cannot be modified).
    pub is_constant: bool,
}

impl StorageItem {
    /// Creates a new storage item.
    pub fn new(value: Vec<u8>) -> Self {
        Self {
            value,
            is_constant: false,
        }
    }

    /// Creates a constant storage item.
    pub fn constant(value: Vec<u8>) -> Self {
        Self {
            value,
            is_constant: true,
        }
    }

    /// Returns the value as bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.value
    }

    /// Returns true if this item is constant.
    pub fn is_constant(&self) -> bool {
        self.is_constant
    }
}

impl Default for StorageItem {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Tracks changes to contract storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageChange {
    /// Item was added.
    Added(StorageItem),
    /// Item was modified.
    Modified(StorageItem),
    /// Item was deleted.
    Deleted,
}

/// Contract storage manager.
///
/// Provides a cache layer over the underlying storage backend,
/// tracking changes for commit/rollback operations.
#[derive(Debug, Default)]
pub struct ContractStorage {
    /// Cached storage items.
    cache: HashMap<StorageKey, Option<StorageItem>>,
    /// Tracked changes since last commit.
    changes: HashMap<StorageKey, StorageChange>,
}

impl ContractStorage {
    /// Creates a new empty contract storage.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            changes: HashMap::new(),
        }
    }

    /// Gets a storage item by key.
    pub fn get(&self, key: &StorageKey) -> Option<&StorageItem> {
        self.cache.get(key).and_then(|opt| opt.as_ref())
    }

    /// Puts a storage item.
    pub fn put(&mut self, key: StorageKey, item: StorageItem) -> StateResult<()> {
        // Check if modifying a constant
        if let Some(Some(existing)) = self.cache.get(&key) {
            if existing.is_constant {
                return Err(StateError::InvalidStateTransition(
                    "cannot modify constant storage item".to_string(),
                ));
            }
        }

        let change = if self.cache.contains_key(&key) {
            StorageChange::Modified(item.clone())
        } else {
            StorageChange::Added(item.clone())
        };

        self.cache.insert(key.clone(), Some(item));
        self.changes.insert(key, change);
        Ok(())
    }

    /// Deletes a storage item.
    pub fn delete(&mut self, key: &StorageKey) -> StateResult<()> {
        // Check if deleting a constant
        if let Some(Some(existing)) = self.cache.get(key) {
            if existing.is_constant {
                return Err(StateError::InvalidStateTransition(
                    "cannot delete constant storage item".to_string(),
                ));
            }
        }

        self.cache.insert(key.clone(), None);
        self.changes.insert(key.clone(), StorageChange::Deleted);
        Ok(())
    }

    /// Returns true if the key exists in storage.
    pub fn contains(&self, key: &StorageKey) -> bool {
        self.cache.get(key).map(|opt| opt.is_some()).unwrap_or(false)
    }

    /// Returns all changes since last commit.
    pub fn changes(&self) -> &HashMap<StorageKey, StorageChange> {
        &self.changes
    }

    /// Clears all tracked changes.
    pub fn clear_changes(&mut self) {
        self.changes.clear();
    }

    /// Returns the number of cached items.
    pub fn len(&self) -> usize {
        self.cache.values().filter(|v| v.is_some()).count()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates over all storage items for a contract.
    pub fn iter_contract<'a>(&'a self, contract_hash: &'a UInt160) -> impl Iterator<Item = (&'a StorageKey, &'a StorageItem)> + 'a {
        self.cache
            .iter()
            .filter(move |(k, v)| k.contract_hash == *contract_hash && v.is_some())
            .map(|(k, v)| (k, v.as_ref().unwrap()))
    }

    /// Finds storage items with a key prefix.
    pub fn find_by_prefix<'a>(
        &'a self,
        contract_hash: &'a UInt160,
        prefix: &'a [u8],
    ) -> impl Iterator<Item = (&'a StorageKey, &'a StorageItem)> + 'a {
        self.cache
            .iter()
            .filter(move |(k, v)| {
                k.contract_hash == *contract_hash
                    && k.key.starts_with(prefix)
                    && v.is_some()
            })
            .map(|(k, v)| (k, v.as_ref().unwrap()))
    }

    /// Loads initial state from a backend.
    pub fn load_from<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = (StorageKey, StorageItem)>,
    {
        for (key, item) in items {
            self.cache.insert(key, Some(item));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_contract_hash() -> UInt160 {
        UInt160::default()
    }

    #[test]
    fn test_storage_key_creation() {
        let hash = test_contract_hash();
        let key = StorageKey::new(hash, vec![1, 2, 3]);

        assert_eq!(key.contract_hash, hash);
        assert_eq!(key.key, vec![1, 2, 3]);
    }

    #[test]
    fn test_storage_key_to_bytes() {
        let hash = test_contract_hash();
        let key = StorageKey::new(hash, vec![1, 2, 3]);
        let bytes = key.to_bytes();

        assert_eq!(bytes.len(), 23); // 20 + 3
    }

    #[test]
    fn test_storage_item_creation() {
        let item = StorageItem::new(vec![1, 2, 3]);
        assert_eq!(item.as_bytes(), &[1, 2, 3]);
        assert!(!item.is_constant());

        let constant = StorageItem::constant(vec![4, 5, 6]);
        assert!(constant.is_constant());
    }

    #[test]
    fn test_contract_storage_put_get() {
        let mut storage = ContractStorage::new();
        let key = StorageKey::new(test_contract_hash(), vec![1, 2, 3]);
        let item = StorageItem::new(vec![4, 5, 6]);

        storage.put(key.clone(), item.clone()).unwrap();

        assert!(storage.contains(&key));
        assert_eq!(storage.get(&key), Some(&item));
        assert_eq!(storage.len(), 1);
    }

    #[test]
    fn test_contract_storage_delete() {
        let mut storage = ContractStorage::new();
        let key = StorageKey::new(test_contract_hash(), vec![1, 2, 3]);
        let item = StorageItem::new(vec![4, 5, 6]);

        storage.put(key.clone(), item).unwrap();
        assert!(storage.contains(&key));

        storage.delete(&key).unwrap();
        assert!(!storage.contains(&key));
    }

    #[test]
    fn test_constant_item_protection() {
        let mut storage = ContractStorage::new();
        let key = StorageKey::new(test_contract_hash(), vec![1, 2, 3]);
        let constant = StorageItem::constant(vec![4, 5, 6]);

        storage.put(key.clone(), constant).unwrap();

        // Should fail to modify constant
        let result = storage.put(key.clone(), StorageItem::new(vec![7, 8, 9]));
        assert!(result.is_err());

        // Should fail to delete constant
        let result = storage.delete(&key);
        assert!(result.is_err());
    }

    #[test]
    fn test_change_tracking() {
        let mut storage = ContractStorage::new();
        let key1 = StorageKey::new(test_contract_hash(), vec![1]);
        let key2 = StorageKey::new(test_contract_hash(), vec![2]);

        storage.put(key1.clone(), StorageItem::new(vec![1])).unwrap();
        storage.put(key2.clone(), StorageItem::new(vec![2])).unwrap();
        storage.put(key1.clone(), StorageItem::new(vec![3])).unwrap(); // Modify
        storage.delete(&key2).unwrap();

        let changes = storage.changes();
        assert_eq!(changes.len(), 2);
        assert!(matches!(changes.get(&key1), Some(StorageChange::Modified(_))));
        assert!(matches!(changes.get(&key2), Some(StorageChange::Deleted)));
    }

    #[test]
    fn test_find_by_prefix() {
        let mut storage = ContractStorage::new();
        let hash = test_contract_hash();

        storage.put(StorageKey::new(hash, vec![1, 0, 0]), StorageItem::new(vec![1])).unwrap();
        storage.put(StorageKey::new(hash, vec![1, 0, 1]), StorageItem::new(vec![2])).unwrap();
        storage.put(StorageKey::new(hash, vec![2, 0, 0]), StorageItem::new(vec![3])).unwrap();

        let results: Vec<_> = storage.find_by_prefix(&hash, &[1, 0]).collect();
        assert_eq!(results.len(), 2);
    }
}
