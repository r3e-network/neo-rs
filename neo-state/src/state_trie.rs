// Copyright (C) 2015-2025 The Neo Project.
//
// state_trie.rs is free software: you can redistribute it and/or modify
// it under the terms of the MIT License.

//! State Trie Manager for Neo N3 State Root Calculation
//!
//! This module provides the integration between world state changes and
//! the Merkle Patricia Trie (MPT) for calculating state roots.
//!
//! ## State Root Calculation
//!
//! In Neo N3, the state root is calculated from all storage changes in a block:
//! - Contract storage (key-value pairs)
//! - Account states (NEO/GAS balances)
//!
//! The MPT root hash represents the cryptographic commitment to the entire
//! world state at a given block height.

use crate::{StateChanges, StateError, StateResult, StorageKey};
use neo_crypto::mpt_trie::{MptResult, MptStoreSnapshot, Trie};
use neo_primitives::UInt256;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

/// In-memory store for MPT trie nodes.
#[derive(Default)]
pub struct MemoryMptStore {
    data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
}

impl MemoryMptStore {
    /// Creates a new in-memory MPT store.
    #[must_use] 
    pub fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the number of nodes stored.
    pub fn len(&self) -> usize {
        self.data.lock().len()
    }

    /// Returns true if the store is empty.
    pub fn is_empty(&self) -> bool {
        self.data.lock().is_empty()
    }
}

impl MptStoreSnapshot for MemoryMptStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self.data.lock().get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.data.lock().insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.data.lock().remove(&key);
        Ok(())
    }
}

/// Manages the state trie for calculating state roots.
///
/// The `StateTrieManager` maintains an MPT trie that tracks all state changes
/// and provides the root hash for state root calculation.
pub struct StateTrieManager {
    /// The underlying MPT trie.
    trie: Trie<MemoryMptStore>,
    /// Current block index.
    current_index: u32,
    /// Whether to track full state history.
    full_state: bool,
}

impl StateTrieManager {
    /// Creates a new `StateTrieManager` with an empty trie.
    #[must_use] 
    pub fn new(full_state: bool) -> Self {
        let store = Arc::new(MemoryMptStore::new());
        let trie = Trie::new(store, None, full_state);
        Self {
            trie,
            current_index: 0,
            full_state,
        }
    }

    /// Creates a `StateTrieManager` with an existing root hash.
    #[must_use] 
    pub fn with_root(root_hash: UInt256, full_state: bool) -> Self {
        let store = Arc::new(MemoryMptStore::new());
        let trie = Trie::new(store, Some(root_hash), full_state);
        Self {
            trie,
            current_index: 0,
            full_state,
        }
    }

    /// Returns the current state root hash.
    ///
    /// Returns `None` if the trie is empty (no state changes).
    pub fn root_hash(&self) -> Option<UInt256> {
        self.trie.root_hash()
    }

    /// Returns the current block index.
    pub const fn current_index(&self) -> u32 {
        self.current_index
    }

    /// Applies state changes to the trie and returns the new root hash.
    ///
    /// This method:
    /// 1. Converts storage changes to trie key-value pairs
    /// 2. Updates the MPT trie with all changes
    /// 3. Commits the changes
    /// 4. Returns the new root hash
    pub fn apply_changes(
        &mut self,
        block_index: u32,
        changes: &StateChanges,
    ) -> StateResult<UInt256> {
        self.current_index = block_index;

        // Apply storage changes to trie
        for (key, value) in &changes.storage {
            let trie_key = Self::storage_key_to_trie_key(key);

            if let Some(item) = value {
                // Insert or update
                self.trie
                    .put(&trie_key, item.as_bytes())
                    .map_err(|e| StateError::TrieError(e.to_string()))?;
                debug!(
                    target: "neo::state",
                    key_len = trie_key.len(),
                    value_len = item.as_bytes().len(),
                    "trie put"
                );
            } else {
                // Delete
                self.trie
                    .delete(&trie_key)
                    .map_err(|e| StateError::TrieError(e.to_string()))?;
                debug!(
                    target: "neo::state",
                    key_len = trie_key.len(),
                    "trie delete"
                );
            }
        }

        // Apply account changes to trie (accounts are stored with a special prefix)
        for (hash, account) in &changes.accounts {
            let trie_key = Self::account_key_to_trie_key(hash);

            if let Some(acc) = account {
                // Serialize account state as simple binary format
                let value = Self::serialize_account(acc);
                self.trie
                    .put(&trie_key, &value)
                    .map_err(|e| StateError::TrieError(e.to_string()))?;
                debug!(
                    target: "neo::state",
                    account = %hash,
                    "trie put account"
                );
            } else {
                self.trie
                    .delete(&trie_key)
                    .map_err(|e| StateError::TrieError(e.to_string()))?;
                debug!(
                    target: "neo::state",
                    account = %hash,
                    "trie delete account"
                );
            }
        }

        // Commit changes to get the new root hash
        self.trie
            .commit()
            .map_err(|e| StateError::TrieError(e.to_string()))?;

        // Get the root hash (or zero hash if empty)
        let root_hash = self.trie.root_hash().unwrap_or_else(UInt256::zero);

        info!(
            target: "neo::state",
            block_index,
            root_hash = %root_hash,
            storage_changes = changes.storage.len(),
            account_changes = changes.accounts.len(),
            "state root calculated"
        );

        Ok(root_hash)
    }

    /// Converts a `StorageKey` to a trie key.
    ///
    /// Format: `contract_hash` (20 bytes) + `key_bytes`
    fn storage_key_to_trie_key(key: &StorageKey) -> Vec<u8> {
        let mut trie_key = Vec::with_capacity(20 + key.key.len());
        trie_key.extend_from_slice(&key.contract_hash.to_array());
        trie_key.extend_from_slice(&key.key);
        trie_key
    }

    /// Converts an account hash to a trie key.
    ///
    /// Format: 0x14 (account prefix) + `account_hash` (20 bytes)
    fn account_key_to_trie_key(hash: &neo_primitives::UInt160) -> Vec<u8> {
        let mut trie_key = Vec::with_capacity(21);
        trie_key.push(0x14); // Account prefix
        trie_key.extend_from_slice(&hash.to_array());
        trie_key
    }

    /// Serializes an `AccountState` to bytes for trie storage.
    fn serialize_account(acc: &crate::AccountState) -> Vec<u8> {
        // Simple binary format: neo_balance (8) + gas_balance (8) + balance_height (4)
        let mut value = Vec::with_capacity(20);
        value.extend_from_slice(&acc.neo_balance.to_le_bytes());
        value.extend_from_slice(&acc.gas_balance.to_le_bytes());
        value.extend_from_slice(&acc.balance_height.to_le_bytes());
        value
    }

    /// Resets the trie to an empty state.
    pub fn reset(&mut self) {
        let store = Arc::new(MemoryMptStore::new());
        self.trie = Trie::new(store, None, self.full_state);
        self.current_index = 0;
    }

    /// Resets the trie to a specific root hash and block index.
    /// Used for state rollback during chain reorganization.
    ///
    /// Note: This creates a new trie with the given root hash. For full state
    /// mode, the historical trie nodes must still be available in the store.
    /// For non-full state mode, this effectively creates a new trie that will
    /// diverge from the original.
    pub fn reset_to_root(&mut self, root_hash: UInt256, block_index: u32) {
        let store = Arc::new(MemoryMptStore::new());
        self.trie = Trie::new(store, Some(root_hash), self.full_state);
        self.current_index = block_index;
    }
}

impl Default for StateTrieManager {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountState, StorageItem};
    use neo_primitives::UInt160;

    #[test]
    fn test_empty_trie_root() {
        let manager = StateTrieManager::new(false);
        assert!(manager.root_hash().is_none());
    }

    #[test]
    fn test_apply_storage_changes() {
        let mut manager = StateTrieManager::new(false);

        let contract_hash = UInt160::from([1u8; 20]);
        let key = StorageKey::new(contract_hash, vec![0x01, 0x02]);
        let item = StorageItem::new(vec![0x03, 0x04, 0x05]);

        let mut changes = StateChanges::new();
        changes.storage.insert(key, Some(item));

        let root = manager.apply_changes(1, &changes).unwrap();
        assert_ne!(root, UInt256::zero());

        // Same changes should produce same root
        let mut manager2 = StateTrieManager::new(false);
        let root2 = manager2.apply_changes(1, &changes).unwrap();
        assert_eq!(root, root2);
    }

    #[test]
    fn test_apply_account_changes() {
        let mut manager = StateTrieManager::new(false);

        let hash = UInt160::from([2u8; 20]);
        let account = AccountState::with_balances(hash, 1000, 50_000_000);

        let mut changes = StateChanges::new();
        changes.accounts.insert(hash, Some(account));

        let root = manager.apply_changes(1, &changes).unwrap();
        assert_ne!(root, UInt256::zero());
    }

    #[test]
    fn test_different_changes_different_roots() {
        let mut manager1 = StateTrieManager::new(false);
        let mut manager2 = StateTrieManager::new(false);

        let contract_hash = UInt160::from([1u8; 20]);

        // Different values should produce different roots
        let key = StorageKey::new(contract_hash, vec![0x01]);

        let mut changes1 = StateChanges::new();
        changes1
            .storage
            .insert(key.clone(), Some(StorageItem::new(vec![0x01])));

        let mut changes2 = StateChanges::new();
        changes2
            .storage
            .insert(key, Some(StorageItem::new(vec![0x02])));

        let root1 = manager1.apply_changes(1, &changes1).unwrap();
        let root2 = manager2.apply_changes(1, &changes2).unwrap();

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_incremental_changes() {
        let mut manager = StateTrieManager::new(false);

        let contract_hash = UInt160::from([1u8; 20]);

        // First block
        let key1 = StorageKey::new(contract_hash, vec![0x01]);
        let mut changes1 = StateChanges::new();
        changes1
            .storage
            .insert(key1, Some(StorageItem::new(vec![0xAA])));
        let root1 = manager.apply_changes(1, &changes1).unwrap();

        // Second block adds more data
        let key2 = StorageKey::new(contract_hash, vec![0x02]);
        let mut changes2 = StateChanges::new();
        changes2
            .storage
            .insert(key2, Some(StorageItem::new(vec![0xBB])));
        let root2 = manager.apply_changes(2, &changes2).unwrap();

        // Roots should be different
        assert_ne!(root1, root2);
        assert_eq!(manager.current_index(), 2);
    }
}
