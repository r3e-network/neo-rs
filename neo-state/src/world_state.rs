// Copyright (C) 2015-2025 The Neo Project.
//
// world_state.rs is free software: you can redistribute it and/or modify
// it under the terms of the MIT License.

//! World state abstraction for Neo N3.
//!
//! The world state represents the complete state of the Neo blockchain at a
//! given point in time, including:
//! - Account balances (NEO, GAS)
//! - Contract code and storage
//! - Validator votes
//!
//! This module provides traits for abstracting over different storage backends.

use crate::account::AccountState;
use crate::contract_storage::{ContractStorage, StorageItem, StorageKey};
use crate::error::StateResult;
use hashbrown::HashMap;
use neo_primitives::UInt160;
use parking_lot::RwLock;

/// Trait for world state operations.
///
/// This trait abstracts over the underlying storage backend, allowing
/// different implementations (in-memory, RocksDB, etc.) to be used
/// interchangeably.
pub trait WorldState: Send + Sync {
    /// Gets an account by script hash.
    fn get_account(&self, hash: &UInt160) -> StateResult<Option<AccountState>>;

    /// Gets a storage item by key.
    fn get_storage(&self, key: &StorageKey) -> StateResult<Option<StorageItem>>;

    /// Creates a new snapshot for isolated state changes.
    fn snapshot(&self) -> StateResult<Box<dyn StateView>>;

    /// Commits changes from a snapshot.
    fn commit(&mut self, changes: StateChanges) -> StateResult<()>;

    /// Returns the current state root hash.
    fn state_root(&self) -> StateResult<[u8; 32]>;

    /// Returns the current block height.
    fn height(&self) -> u32;
}

/// Read-only view of the world state.
pub trait StateView: Send + Sync {
    /// Gets an account by script hash.
    fn get_account(&self, hash: &UInt160) -> StateResult<Option<AccountState>>;

    /// Gets a storage item by key.
    fn get_storage(&self, key: &StorageKey) -> StateResult<Option<StorageItem>>;

    /// Checks if an account exists.
    fn account_exists(&self, hash: &UInt160) -> StateResult<bool> {
        Ok(self.get_account(hash)?.is_some())
    }

    /// Checks if a storage key exists.
    fn storage_exists(&self, key: &StorageKey) -> StateResult<bool> {
        Ok(self.get_storage(key)?.is_some())
    }
}

/// Mutable view of the world state for transaction execution.
pub trait StateMut: StateView {
    /// Sets an account state.
    fn set_account(&mut self, hash: UInt160, account: AccountState) -> StateResult<()>;

    /// Deletes an account.
    fn delete_account(&mut self, hash: &UInt160) -> StateResult<()>;

    /// Sets a storage item.
    fn set_storage(&mut self, key: StorageKey, item: StorageItem) -> StateResult<()>;

    /// Deletes a storage item.
    fn delete_storage(&mut self, key: &StorageKey) -> StateResult<()>;

    /// Commits all changes and returns the change set.
    fn commit(self: Box<Self>) -> StateResult<StateChanges>;

    /// Rolls back all changes.
    fn rollback(self: Box<Self>) -> StateResult<()>;
}

/// Represents a set of state changes.
#[derive(Debug, Default, Clone)]
pub struct StateChanges {
    /// Account changes (hash -> new state or None for deletion).
    pub accounts: HashMap<UInt160, Option<AccountState>>,
    /// Storage changes (key -> new item or None for deletion).
    pub storage: HashMap<StorageKey, Option<StorageItem>>,
}

impl StateChanges {
    /// Creates an empty change set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if there are no changes.
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty() && self.storage.is_empty()
    }

    /// Merges another change set into this one.
    pub fn merge(&mut self, other: StateChanges) {
        self.accounts.extend(other.accounts);
        self.storage.extend(other.storage);
    }

    /// Returns the number of account changes.
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Returns the number of storage changes.
    pub fn storage_count(&self) -> usize {
        self.storage.len()
    }
}

/// In-memory implementation of WorldState for testing.
#[derive(Debug, Default)]
pub struct MemoryWorldState {
    /// Account states.
    accounts: RwLock<HashMap<UInt160, AccountState>>,
    /// Contract storage.
    storage: RwLock<ContractStorage>,
    /// Current block height.
    height: u32,
}

impl MemoryWorldState {
    /// Creates a new empty in-memory world state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a world state with initial accounts.
    pub fn with_accounts(accounts: HashMap<UInt160, AccountState>) -> Self {
        Self {
            accounts: RwLock::new(accounts),
            storage: RwLock::new(ContractStorage::new()),
            height: 0,
        }
    }

    /// Sets the current block height.
    pub fn set_height(&mut self, height: u32) {
        self.height = height;
    }
}

impl WorldState for MemoryWorldState {
    fn get_account(&self, hash: &UInt160) -> StateResult<Option<AccountState>> {
        Ok(self.accounts.read().get(hash).cloned())
    }

    fn get_storage(&self, key: &StorageKey) -> StateResult<Option<StorageItem>> {
        Ok(self.storage.read().get(key).cloned())
    }

    fn snapshot(&self) -> StateResult<Box<dyn StateView>> {
        let accounts = self.accounts.read().clone();
        let storage_cache: HashMap<StorageKey, StorageItem> = self
            .storage
            .read()
            .iter_contract(&UInt160::default())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        Ok(Box::new(MemoryStateSnapshot {
            accounts,
            storage: storage_cache,
        }))
    }

    fn commit(&mut self, changes: StateChanges) -> StateResult<()> {
        let mut accounts = self.accounts.write();
        let mut storage = self.storage.write();

        for (hash, account_opt) in changes.accounts {
            match account_opt {
                Some(account) => {
                    accounts.insert(hash, account);
                }
                None => {
                    accounts.remove(&hash);
                }
            }
        }

        for (key, item_opt) in changes.storage {
            match item_opt {
                Some(item) => {
                    storage.put(key, item)?;
                }
                None => {
                    storage.delete(&key)?;
                }
            }
        }

        Ok(())
    }

    fn state_root(&self) -> StateResult<[u8; 32]> {
        // In-memory state does not maintain MPT - use StateTrieManager for state root calculation
        Ok([0u8; 32])
    }

    fn height(&self) -> u32 {
        self.height
    }
}

/// In-memory snapshot for testing.
#[derive(Debug)]
struct MemoryStateSnapshot {
    accounts: HashMap<UInt160, AccountState>,
    storage: HashMap<StorageKey, StorageItem>,
}

impl StateView for MemoryStateSnapshot {
    fn get_account(&self, hash: &UInt160) -> StateResult<Option<AccountState>> {
        Ok(self.accounts.get(hash).cloned())
    }

    fn get_storage(&self, key: &StorageKey) -> StateResult<Option<StorageItem>> {
        Ok(self.storage.get(key).cloned())
    }
}

/// Mutable state view for transaction execution.
#[derive(Debug)]
pub struct MutableStateView {
    /// Base state for reads.
    base_accounts: HashMap<UInt160, AccountState>,
    base_storage: HashMap<StorageKey, StorageItem>,
    /// Pending changes.
    changes: StateChanges,
}

impl MutableStateView {
    /// Creates a new mutable state view.
    pub fn new(
        accounts: HashMap<UInt160, AccountState>,
        storage: HashMap<StorageKey, StorageItem>,
    ) -> Self {
        Self {
            base_accounts: accounts,
            base_storage: storage,
            changes: StateChanges::new(),
        }
    }
}

impl StateView for MutableStateView {
    fn get_account(&self, hash: &UInt160) -> StateResult<Option<AccountState>> {
        // Check pending changes first
        if let Some(account_opt) = self.changes.accounts.get(hash) {
            return Ok(account_opt.clone());
        }
        // Fall back to base state
        Ok(self.base_accounts.get(hash).cloned())
    }

    fn get_storage(&self, key: &StorageKey) -> StateResult<Option<StorageItem>> {
        // Check pending changes first
        if let Some(item_opt) = self.changes.storage.get(key) {
            return Ok(item_opt.clone());
        }
        // Fall back to base state
        Ok(self.base_storage.get(key).cloned())
    }
}

impl StateMut for MutableStateView {
    fn set_account(&mut self, hash: UInt160, account: AccountState) -> StateResult<()> {
        self.changes.accounts.insert(hash, Some(account));
        Ok(())
    }

    fn delete_account(&mut self, hash: &UInt160) -> StateResult<()> {
        self.changes.accounts.insert(*hash, None);
        Ok(())
    }

    fn set_storage(&mut self, key: StorageKey, item: StorageItem) -> StateResult<()> {
        self.changes.storage.insert(key, Some(item));
        Ok(())
    }

    fn delete_storage(&mut self, key: &StorageKey) -> StateResult<()> {
        self.changes.storage.insert(key.clone(), None);
        Ok(())
    }

    fn commit(self: Box<Self>) -> StateResult<StateChanges> {
        Ok(self.changes)
    }

    fn rollback(self: Box<Self>) -> StateResult<()> {
        // Simply drop the changes
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_world_state_accounts() {
        let mut state = MemoryWorldState::new();

        let hash = UInt160::default();
        let account = AccountState::with_balances(hash, 100, 50_000_000);

        // Initially no account
        assert!(state.get_account(&hash).unwrap().is_none());

        // Commit account
        let mut changes = StateChanges::new();
        changes.accounts.insert(hash, Some(account.clone()));
        state.commit(changes).unwrap();

        // Now account exists
        let retrieved = state.get_account(&hash).unwrap().unwrap();
        assert_eq!(retrieved.neo_balance(), 100);
        assert_eq!(retrieved.gas_balance(), 50_000_000);
    }

    #[test]
    fn test_memory_world_state_storage() {
        let mut state = MemoryWorldState::new();

        let key = StorageKey::new(UInt160::default(), vec![1, 2, 3]);
        let item = StorageItem::new(vec![4, 5, 6]);

        // Initially no storage
        assert!(state.get_storage(&key).unwrap().is_none());

        // Commit storage
        let mut changes = StateChanges::new();
        changes.storage.insert(key.clone(), Some(item.clone()));
        state.commit(changes).unwrap();

        // Now storage exists
        let retrieved = state.get_storage(&key).unwrap().unwrap();
        assert_eq!(retrieved.as_bytes(), &[4, 5, 6]);
    }

    #[test]
    fn test_mutable_state_view() {
        let accounts = HashMap::new();
        let storage = HashMap::new();
        let mut view = MutableStateView::new(accounts, storage);

        let hash = UInt160::default();
        let account = AccountState::with_balances(hash, 100, 50_000_000);

        // Set account
        view.set_account(hash, account.clone()).unwrap();

        // Read back
        let retrieved = view.get_account(&hash).unwrap().unwrap();
        assert_eq!(retrieved.neo_balance(), 100);

        // Commit and get changes
        let changes = Box::new(view).commit().unwrap();
        assert_eq!(changes.account_count(), 1);
    }

    #[test]
    fn test_state_changes_merge() {
        let mut changes1 = StateChanges::new();
        changes1
            .accounts
            .insert(UInt160::default(), Some(AccountState::default()));

        let mut changes2 = StateChanges::new();
        let key = StorageKey::new(UInt160::default(), vec![1]);
        changes2
            .storage
            .insert(key, Some(StorageItem::new(vec![1])));

        changes1.merge(changes2);

        assert_eq!(changes1.account_count(), 1);
        assert_eq!(changes1.storage_count(), 1);
    }

    #[test]
    fn test_snapshot_isolation() {
        let mut state = MemoryWorldState::new();

        let hash = UInt160::default();
        let account = AccountState::with_balances(hash, 100, 50_000_000);

        // Commit initial state
        let mut changes = StateChanges::new();
        changes.accounts.insert(hash, Some(account));
        state.commit(changes).unwrap();

        // Take snapshot
        let snapshot = state.snapshot().unwrap();

        // Modify state
        let mut changes = StateChanges::new();
        changes.accounts.insert(
            hash,
            Some(AccountState::with_balances(hash, 200, 100_000_000)),
        );
        state.commit(changes).unwrap();

        // Snapshot should still see old value
        let snapshot_account = snapshot.get_account(&hash).unwrap().unwrap();
        assert_eq!(snapshot_account.neo_balance(), 100);

        // Current state should see new value
        let current_account = state.get_account(&hash).unwrap().unwrap();
        assert_eq!(current_account.neo_balance(), 200);
    }
}
