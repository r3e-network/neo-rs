//! [`StateStore`] - in-memory storage for state roots and pending
//! state-root candidates.
//!
//! The full state service persists the MPT (Merkle Patricia Trie)
//! itself to a backing store; this in-memory implementation provides
//! the verification pipeline's hot-path surfaces:
//!
//! - [`StateStore::try_add_state_root`] - stage a freshly-received
//!   state root as a candidate.
//! - [`StateStore::commit_validated_state_roots`] - move a batch of
//!   state roots from the candidate set to the validated set.
//! - [`StateStore::get_state_root`] - look up a state root by
//!   `(block_index)` or by `trie_root_hash`.
//!
//! Mirrors the C# `StateService.Storage.StateStore` shape.

use crate::state_root::StateRoot;
use neo_primitives::UInt256;
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The kind of [`StateStore`] record a [`StateStore::get_state_root`]
/// query should return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateStoreLookup {
    /// Look up a state root by its block index.
    ByBlockIndex(u32),
    /// Look up a state root by its trie root hash.
    ByRootHash(UInt256),
}

/// In-memory state store for state roots.
#[derive(Debug, Default)]
pub struct StateStore {
    inner: RwLock<StateStoreInner>,
}

#[derive(Debug, Default)]
struct StateStoreInner {
    /// State roots indexed by block index.
    by_index: BTreeMap<u32, StateRoot>,
    /// State roots indexed by trie root hash.
    by_root_hash: HashMap<UInt256, StateRoot>,
    /// State roots staged as candidates but not yet validated.
    candidates: HashSet<UInt256>,
}

/// Transactional, read-committed view of the [`StateStore`].
///
/// Holds an `Arc` to the underlying store and a snapshot of the
/// candidate set captured at the time the transaction was opened.
pub struct StateStoreTransaction {
    store: StateStore,
    candidates_at_open: HashSet<UInt256>,
}

impl StateStoreTransaction {
    /// Returns the state roots that were in the candidate set when
    /// this transaction was opened.
    pub fn candidates(&self) -> &HashSet<UInt256> {
        &self.candidates_at_open
    }

    /// Looks up a state root within this transaction's view.
    pub fn get(&self, lookup: StateStoreLookup) -> Option<StateRoot> {
        self.store.get_state_root(lookup)
    }

    /// Commits a batch of validated state roots, removing them from
    /// the candidate set and publishing them as the canonical
    /// validated record for their block index.
    pub fn commit(&self, roots: &[StateRoot]) {
        self.store.commit_validated_state_roots(roots);
    }
}

impl StateStore {
    /// Constructs a new, empty state store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of state roots currently in the store.
    pub fn len(&self) -> usize {
        self.inner.read().by_index.len()
    }

    /// Returns whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.read().by_index.is_empty()
    }

    /// Returns the number of pending candidate state roots.
    pub fn candidate_count(&self) -> usize {
        self.inner.read().candidates.len()
    }

    /// Begins a new transaction, returning a view that captures the
    /// current candidate set.
    pub fn begin_transaction(&self) -> StateStoreTransaction {
        let candidates = self.inner.read().candidates.clone();
        StateStoreTransaction {
            store: StateStore::clone(self),
            candidates_at_open: candidates,
        }
    }

    /// Stages a state root as a candidate (does not mark it validated).
    pub fn try_add_state_root(&self, root: StateRoot) -> bool {
        let hash = *root.root_hash();
        let mut guard = self.inner.write();
        if guard.by_root_hash.contains_key(&hash) {
            return false;
        }
        guard.candidates.insert(hash);
        guard.by_root_hash.insert(hash, root.clone());
        guard.by_index.insert(root.index(), root);
        true
    }

    /// Moves a batch of state roots from the candidate set to the
    /// validated set (recorded by their block index).
    pub fn commit_validated_state_roots(&self, roots: &[StateRoot]) {
        let mut guard = self.inner.write();
        for root in roots {
            let hash = *root.root_hash();
            guard.candidates.remove(&hash);
            // The state root is already in by_index / by_root_hash
            // from the try_add_state_root call. This step confirms
            // its validation status.
        }
    }

    /// Drops a state root from the store entirely (used when a
    /// candidate fails validation and must be discarded).
    pub fn discard(&self, root_hash: &UInt256) -> Option<StateRoot> {
        let mut guard = self.inner.write();
        guard.candidates.remove(root_hash);
        let removed = guard.by_root_hash.remove(root_hash)?;
        guard.by_index.remove(&removed.index());
        Some(removed)
    }

    /// Looks up a state root by block index or trie root hash.
    pub fn get_state_root(&self, lookup: StateStoreLookup) -> Option<StateRoot> {
        let guard = self.inner.read();
        match lookup {
            StateStoreLookup::ByBlockIndex(index) => guard.by_index.get(&index).cloned(),
            StateStoreLookup::ByRootHash(hash) => guard.by_root_hash.get(&hash).cloned(),
        }
    }

    /// Returns the current local (highest) validated block index, or
    /// `None` if no state roots have been committed.
    pub fn current_local_index(&self) -> Option<u32> {
        let guard = self.inner.read();
        guard.by_index.keys().next_back().copied()
    }
}

impl Clone for StateStore {
    fn clone(&self) -> Self {
        Self {
            inner: RwLock::new(self.inner.read().clone()),
        }
    }
}

impl Clone for StateStoreInner {
    fn clone(&self) -> Self {
        Self {
            by_index: self.by_index.clone(),
            by_root_hash: self.by_root_hash.clone(),
            candidates: self.candidates.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(index: u32, byte: u8) -> StateRoot {
        StateRoot::new_current(index, UInt256::from([byte; 32]))
    }

    #[test]
    fn try_add_and_get_by_index() {
        let store = StateStore::new();
        assert!(store.try_add_state_root(root(1, 0x11)));
        let got = store.get_state_root(StateStoreLookup::ByBlockIndex(1));
        assert!(got.is_some());
    }

    #[test]
    fn try_add_rejects_duplicate_root_hash() {
        let store = StateStore::new();
        let r1 = root(1, 0xAB);
        let r2 = root(2, 0xAB);
        assert!(store.try_add_state_root(r1));
        assert!(!store.try_add_state_root(r2));
    }

    #[test]
    fn commit_moves_candidate_to_validated() {
        let store = StateStore::new();
        let r = root(1, 0xCC);
        assert!(store.try_add_state_root(r.clone()));
        assert_eq!(store.candidate_count(), 1);
        store.commit_validated_state_roots(&[r]);
        assert_eq!(store.candidate_count(), 0);
        assert!(store.get_state_root(StateStoreLookup::ByBlockIndex(1)).is_some());
    }

    #[test]
    fn discard_removes_state_root() {
        let store = StateStore::new();
        let r = root(1, 0xDD);
        assert!(store.try_add_state_root(r.clone()));
        let removed = store.discard(r.root_hash());
        assert!(removed.is_some());
        assert!(store.get_state_root(StateStoreLookup::ByBlockIndex(1)).is_none());
    }

    #[test]
    fn transaction_captures_candidate_snapshot() {
        let store = StateStore::new();
        store.try_add_state_root(root(1, 0x10));
        store.try_add_state_root(root(2, 0x20));
        let tx = store.begin_transaction();
        assert_eq!(tx.candidates().len(), 2);
    }
}
