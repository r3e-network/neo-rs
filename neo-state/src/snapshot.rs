// Copyright (C) 2015-2025 The Neo Project.
//
// snapshot.rs is free software: you can redistribute it and/or modify
// it under the terms of the MIT License.

//! State snapshot management for Neo N3.

use crate::account::AccountState;
use crate::contract_storage::{StorageChange, StorageKey};
use crate::error::{StateError, StateResult};
use hashbrown::HashMap;
use neo_primitives::UInt160;

/// Maximum allowed snapshot depth.
pub const MAX_SNAPSHOT_DEPTH: usize = 16;

/// Represents the state of a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotState {
    /// Snapshot is active and can be modified.
    Active,
    /// Snapshot has been committed.
    Committed,
    /// Snapshot has been rolled back.
    RolledBack,
}

/// A point-in-time snapshot of the world state.
///
/// Snapshots provide isolation for state changes during transaction
/// execution. Changes can be committed to the parent state or rolled
/// back without affecting the parent.
#[derive(Debug)]
pub struct StateSnapshot {
    /// Unique identifier for this snapshot.
    id: u64,
    /// Parent snapshot (if any).
    parent_id: Option<u64>,
    /// Current state of this snapshot.
    state: SnapshotState,
    /// Account state changes.
    account_changes: HashMap<UInt160, Option<AccountState>>,
    /// Contract storage changes.
    storage_changes: HashMap<StorageKey, StorageChange>,
    /// Depth of this snapshot in the hierarchy.
    depth: usize,
}

impl StateSnapshot {
    /// Creates a new root snapshot.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            parent_id: None,
            state: SnapshotState::Active,
            account_changes: HashMap::new(),
            storage_changes: HashMap::new(),
            depth: 0,
        }
    }

    /// Creates a child snapshot.
    pub fn child(id: u64, parent_id: u64, parent_depth: usize) -> StateResult<Self> {
        let depth = parent_depth + 1;
        if depth > MAX_SNAPSHOT_DEPTH {
            return Err(StateError::MaxDepthExceeded(depth));
        }

        Ok(Self {
            id,
            parent_id: Some(parent_id),
            state: SnapshotState::Active,
            account_changes: HashMap::new(),
            storage_changes: HashMap::new(),
            depth,
        })
    }

    /// Returns the snapshot ID.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the parent snapshot ID.
    pub fn parent_id(&self) -> Option<u64> {
        self.parent_id
    }

    /// Returns the current state.
    pub fn state(&self) -> SnapshotState {
        self.state
    }

    /// Returns the depth of this snapshot.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Returns true if this snapshot is active.
    pub fn is_active(&self) -> bool {
        self.state == SnapshotState::Active
    }

    /// Records an account change.
    pub fn record_account_change(&mut self, hash: UInt160, account: Option<AccountState>) -> StateResult<()> {
        if !self.is_active() {
            return Err(StateError::InvalidSnapshotState(
                "snapshot is not active".to_string(),
            ));
        }
        self.account_changes.insert(hash, account);
        Ok(())
    }

    /// Records a storage change.
    pub fn record_storage_change(&mut self, key: StorageKey, change: StorageChange) -> StateResult<()> {
        if !self.is_active() {
            return Err(StateError::InvalidSnapshotState(
                "snapshot is not active".to_string(),
            ));
        }
        self.storage_changes.insert(key, change);
        Ok(())
    }

    /// Returns all account changes.
    pub fn account_changes(&self) -> &HashMap<UInt160, Option<AccountState>> {
        &self.account_changes
    }

    /// Returns all storage changes.
    pub fn storage_changes(&self) -> &HashMap<StorageKey, StorageChange> {
        &self.storage_changes
    }

    /// Marks this snapshot as committed.
    pub fn mark_committed(&mut self) -> StateResult<()> {
        if self.state != SnapshotState::Active {
            return Err(StateError::SnapshotAlreadyCommitted);
        }
        self.state = SnapshotState::Committed;
        Ok(())
    }

    /// Marks this snapshot as rolled back.
    pub fn mark_rolled_back(&mut self) -> StateResult<()> {
        if self.state != SnapshotState::Active {
            return Err(StateError::SnapshotAlreadyRolledBack);
        }
        self.state = SnapshotState::RolledBack;
        Ok(())
    }

    /// Returns true if this snapshot has changes.
    pub fn has_changes(&self) -> bool {
        !self.account_changes.is_empty() || !self.storage_changes.is_empty()
    }

    /// Clears all changes.
    pub fn clear(&mut self) {
        self.account_changes.clear();
        self.storage_changes.clear();
    }
}

/// Manages a stack of state snapshots.
#[derive(Debug)]
pub struct SnapshotManager {
    /// Next snapshot ID.
    next_id: u64,
    /// Active snapshots by ID.
    snapshots: HashMap<u64, StateSnapshot>,
    /// Current active snapshot ID.
    current_id: Option<u64>,
}

impl SnapshotManager {
    /// Creates a new snapshot manager.
    pub fn new() -> Self {
        Self {
            next_id: 1,
            snapshots: HashMap::new(),
            current_id: None,
        }
    }

    /// Creates a new snapshot.
    pub fn create_snapshot(&mut self) -> StateResult<u64> {
        let id = self.next_id;
        self.next_id += 1;

        let snapshot = if let Some(parent_id) = self.current_id {
            let parent_depth = self.snapshots
                .get(&parent_id)
                .map(|s| s.depth())
                .unwrap_or(0);
            StateSnapshot::child(id, parent_id, parent_depth)?
        } else {
            StateSnapshot::new(id)
        };

        self.snapshots.insert(id, snapshot);
        self.current_id = Some(id);
        Ok(id)
    }

    /// Gets a snapshot by ID.
    pub fn get(&self, id: u64) -> Option<&StateSnapshot> {
        self.snapshots.get(&id)
    }

    /// Gets a mutable snapshot by ID.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut StateSnapshot> {
        self.snapshots.get_mut(&id)
    }

    /// Returns the current snapshot ID.
    pub fn current_id(&self) -> Option<u64> {
        self.current_id
    }

    /// Returns the current snapshot.
    pub fn current(&self) -> Option<&StateSnapshot> {
        self.current_id.and_then(|id| self.snapshots.get(&id))
    }

    /// Returns the current snapshot mutably.
    pub fn current_mut(&mut self) -> Option<&mut StateSnapshot> {
        self.current_id.and_then(|id| self.snapshots.get_mut(&id))
    }

    /// Commits the current snapshot.
    pub fn commit(&mut self) -> StateResult<StateSnapshot> {
        let current_id = self.current_id
            .ok_or_else(|| StateError::InvalidSnapshotState("no active snapshot".to_string()))?;

        let mut snapshot = self.snapshots
            .remove(&current_id)
            .ok_or_else(|| StateError::InvalidSnapshotState("snapshot not found".to_string()))?;

        snapshot.mark_committed()?;
        self.current_id = snapshot.parent_id();

        Ok(snapshot)
    }

    /// Rolls back the current snapshot.
    pub fn rollback(&mut self) -> StateResult<StateSnapshot> {
        let current_id = self.current_id
            .ok_or_else(|| StateError::InvalidSnapshotState("no active snapshot".to_string()))?;

        let mut snapshot = self.snapshots
            .remove(&current_id)
            .ok_or_else(|| StateError::InvalidSnapshotState("snapshot not found".to_string()))?;

        snapshot.mark_rolled_back()?;
        self.current_id = snapshot.parent_id();

        Ok(snapshot)
    }

    /// Returns the number of active snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }
}

impl Default for SnapshotManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_creation() {
        let snapshot = StateSnapshot::new(1);
        assert_eq!(snapshot.id(), 1);
        assert!(snapshot.parent_id().is_none());
        assert!(snapshot.is_active());
        assert_eq!(snapshot.depth(), 0);
    }

    #[test]
    fn test_child_snapshot() {
        let child = StateSnapshot::child(2, 1, 0).unwrap();
        assert_eq!(child.id(), 2);
        assert_eq!(child.parent_id(), Some(1));
        assert_eq!(child.depth(), 1);
    }

    #[test]
    fn test_max_depth_exceeded() {
        let result = StateSnapshot::child(100, 99, MAX_SNAPSHOT_DEPTH);
        assert!(matches!(result, Err(StateError::MaxDepthExceeded(_))));
    }

    #[test]
    fn test_snapshot_state_transitions() {
        let mut snapshot = StateSnapshot::new(1);

        assert!(snapshot.is_active());

        snapshot.mark_committed().unwrap();
        assert_eq!(snapshot.state(), SnapshotState::Committed);

        // Cannot commit again
        assert!(snapshot.mark_committed().is_err());
    }

    #[test]
    fn test_record_changes() {
        let mut snapshot = StateSnapshot::new(1);

        let account = AccountState::new(UInt160::default());
        snapshot.record_account_change(UInt160::default(), Some(account)).unwrap();

        assert!(snapshot.has_changes());
        assert_eq!(snapshot.account_changes().len(), 1);
    }

    #[test]
    fn test_snapshot_manager_create() {
        let mut manager = SnapshotManager::new();

        let id1 = manager.create_snapshot().unwrap();
        assert_eq!(id1, 1);
        assert_eq!(manager.current_id(), Some(1));

        let id2 = manager.create_snapshot().unwrap();
        assert_eq!(id2, 2);
        assert_eq!(manager.current_id(), Some(2));

        // Check parent relationship
        let snapshot2 = manager.get(2).unwrap();
        assert_eq!(snapshot2.parent_id(), Some(1));
    }

    #[test]
    fn test_snapshot_manager_commit() {
        let mut manager = SnapshotManager::new();

        manager.create_snapshot().unwrap();
        manager.create_snapshot().unwrap();

        assert_eq!(manager.snapshot_count(), 2);

        let committed = manager.commit().unwrap();
        assert_eq!(committed.state(), SnapshotState::Committed);
        assert_eq!(manager.current_id(), Some(1));
        assert_eq!(manager.snapshot_count(), 1);
    }

    #[test]
    fn test_snapshot_manager_rollback() {
        let mut manager = SnapshotManager::new();

        manager.create_snapshot().unwrap();
        manager.create_snapshot().unwrap();

        let rolled_back = manager.rollback().unwrap();
        assert_eq!(rolled_back.state(), SnapshotState::RolledBack);
        assert_eq!(manager.current_id(), Some(1));
    }
}
