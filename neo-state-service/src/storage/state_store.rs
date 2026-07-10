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

use crate::StateRootApplyMetrics;
use crate::mpt_store::{MptBlockChanges, MptChange, MptStore};
use crate::state_root::StateRoot;
use neo_crypto::mpt_trie::{MptError, MptResult};
use neo_primitives::UInt256;
use neo_storage::persistence::Store;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::{DataCache, TrackState};
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// C# `NativeContract.Ledger.Id`. Ledger native-contract bookkeeping is
/// excluded from the StateService MPT just like the C# plugin's
/// `Blockchain_Committing_Handler` filter.
const LEDGER_CONTRACT_ID: i32 = -4;

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
#[derive(Debug)]
pub struct StateStore<S: Store = MemoryStore> {
    inner: Arc<RwLock<StateStoreInner>>,
    /// Optional persisted MPT backend (trie nodes + local-root
    /// records). `None` reproduces the verification-cache-only
    /// behaviour; composition roots that persist the trie construct
    /// the store via [`StateStore::with_mpt`].
    mpt: Option<Arc<MptStore<S>>>,
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

pub(crate) struct ProjectedMptBlock<'a> {
    pub(crate) block_index: u32,
    pub(crate) changes: &'a [MptChange],
    pub(crate) project_us: u64,
    pub(crate) total_start: std::time::Instant,
}

/// Transactional, read-committed view of the [`StateStore`].
///
/// Holds a shared handle to the underlying store and a snapshot of the
/// candidate set captured at the time the transaction was opened.
pub struct StateStoreTransaction<S: Store = MemoryStore> {
    store: StateStore<S>,
    candidates_at_open: HashSet<UInt256>,
}

impl<S> StateStoreTransaction<S>
where
    S: Store,
{
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

impl<S> Default for StateStore<S>
where
    S: Store,
{
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::default()),
            mpt: None,
        }
    }
}

impl StateStore<MemoryStore> {
    /// Constructs a new, empty state store without an MPT backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs a state store with an in-memory persisted MPT backend.
    ///
    /// `full_state` mirrors the C# `StateServiceSettings.FullState`
    /// flag: `true` retains every historical trie version (so
    /// `getstate` / `getproof` / `findstates` can serve old roots),
    /// `false` prunes superseded nodes on each applied block, leaving
    /// only the current root resolvable.
    pub fn with_mpt(full_state: bool) -> Self {
        Self {
            inner: Arc::new(RwLock::default()),
            mpt: Some(Arc::new(MptStore::new(full_state))),
        }
    }
}

impl<S> StateStore<S>
where
    S: Store,
{
    /// Constructs a state store with an MPT backend loaded from a backing store.
    ///
    /// Composition roots should pass the concrete backend type they already
    /// own when it is known (`MdbxStore`, `RocksDbStore`, `MemoryStore`, ...).
    /// Runtime-selected startup code should pass the concrete [`RuntimeStore`]
    /// enum, so the state service stays provider-neutral through the generic
    /// `S` parameter rather than depending on RocksDB/MDBX/memory directly.
    pub fn with_mpt_store(full_state: bool, backing: Arc<S>) -> MptResult<Self> {
        Ok(Self {
            inner: Arc::new(RwLock::default()),
            mpt: Some(Arc::new(MptStore::from_store(backing, full_state)?)),
        })
    }

    /// Returns the persisted MPT backend, if this store maintains one.
    pub fn mpt(&self) -> Option<Arc<MptStore<S>>> {
        self.mpt.clone()
    }

    /// Applies the storage changes tracked in `snapshot` to the persisted MPT
    /// backend and records the local state root for `block_index`.
    ///
    /// Returns `Ok(None)` when this store has no MPT backend. The conversion
    /// mirrors C# `StateService.StatePlugin.Blockchain_Committing_Handler`:
    /// skip Ledger native-contract records, ignore `TrackState.None`, write
    /// added/changed items, and delete removed items.
    pub fn apply_snapshot_changes<B: neo_storage::CacheRead>(
        &self,
        block_index: u32,
        snapshot: &DataCache<B>,
    ) -> MptResult<Option<UInt256>> {
        let total_start = std::time::Instant::now();
        let Some(mpt) = self.mpt.as_ref() else {
            return Ok(None);
        };
        let project_start = std::time::Instant::now();
        let root_before = match Self::contiguous_root_before(mpt, block_index) {
            Ok(root_before) => root_before,
            Err(err) => {
                StateRootApplyMetrics::record_apply(
                    block_index,
                    0,
                    elapsed_us(project_start),
                    0,
                    elapsed_us(total_start),
                    false,
                );
                return Err(err);
            }
        };
        let project_us = elapsed_us(project_start);
        self.apply_snapshot_mpt_changes_with_root(
            block_index,
            root_before,
            snapshot,
            project_us,
            total_start,
        )
    }

    pub(crate) fn project_mpt_changes_into<B: neo_storage::CacheRead>(
        snapshot: &DataCache<B>,
        changes: &mut Vec<MptChange>,
    ) {
        changes.clear();
        changes.reserve(snapshot.pending_change_count());
        Self::push_mpt_changes_from_snapshot(snapshot, changes);
    }

    pub(crate) fn apply_projected_mpt_changes(
        &self,
        block_index: u32,
        changes: &[MptChange],
        project_us: u64,
        total_start: std::time::Instant,
    ) -> MptResult<Option<UInt256>> {
        let Some(mpt) = self.mpt.as_ref() else {
            return Ok(None);
        };
        let root_before = match Self::contiguous_root_before(mpt, block_index) {
            Ok(root_before) => root_before,
            Err(err) => {
                StateRootApplyMetrics::record_apply(
                    block_index,
                    changes.len(),
                    project_us,
                    0,
                    elapsed_us(total_start),
                    false,
                );
                return Err(err);
            }
        };
        self.apply_projected_mpt_changes_with_root(
            block_index,
            root_before,
            changes,
            project_us,
            total_start,
        )
    }

    pub(crate) fn apply_projected_mpt_change_batch(
        &self,
        requests: &[ProjectedMptBlock<'_>],
    ) -> MptResult<Vec<UInt256>> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        if requests.len() == 1 {
            return Ok(self
                .apply_projected_mpt_changes(
                    requests[0].block_index,
                    requests[0].changes,
                    requests[0].project_us,
                    requests[0].total_start,
                )?
                .map(|root| vec![root])
                .unwrap_or_default());
        }

        let Some(mpt) = self.mpt.as_ref() else {
            return Ok(Vec::new());
        };
        let first = &requests[0];
        for pair in requests.windows(2) {
            if pair[0].block_index.checked_add(1) != Some(pair[1].block_index) {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: block {} followed by {}",
                    pair[0].block_index, pair[1].block_index
                )));
            }
        }
        let root_before = match Self::contiguous_root_before(mpt, first.block_index) {
            Ok(root_before) => root_before,
            Err(err) => {
                StateRootApplyMetrics::record_apply(
                    first.block_index,
                    first.changes.len(),
                    first.project_us,
                    0,
                    elapsed_us(first.total_start),
                    false,
                );
                return Err(err);
            }
        };

        let blocks = requests
            .iter()
            .map(|request| MptBlockChanges {
                block_index: request.block_index,
                changes: request.changes,
            })
            .collect::<Vec<_>>();
        let apply_start = std::time::Instant::now();
        match mpt.apply_block_changes_batch(root_before, &blocks) {
            Ok(roots) => {
                let apply_us = elapsed_us(apply_start) / (requests.len() as u64).max(1);
                for request in requests {
                    StateRootApplyMetrics::record_apply(
                        request.block_index,
                        request.changes.len(),
                        request.project_us,
                        apply_us,
                        elapsed_us(request.total_start),
                        true,
                    );
                }
                Ok(roots)
            }
            Err(err) => {
                let apply_us = elapsed_us(apply_start) / (requests.len() as u64).max(1);
                for request in requests {
                    StateRootApplyMetrics::record_apply(
                        request.block_index,
                        request.changes.len(),
                        request.project_us,
                        apply_us,
                        elapsed_us(request.total_start),
                        false,
                    );
                }
                Err(err)
            }
        }
    }

    fn apply_projected_mpt_changes_with_root(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        changes: &[MptChange],
        project_us: u64,
        total_start: std::time::Instant,
    ) -> MptResult<Option<UInt256>> {
        let Some(mpt) = self.mpt.as_ref() else {
            return Ok(None);
        };
        let apply_start = std::time::Instant::now();
        match mpt.apply_block_changes(block_index, root_before, changes) {
            Ok(root_hash) => {
                StateRootApplyMetrics::record_apply(
                    block_index,
                    changes.len(),
                    project_us,
                    elapsed_us(apply_start),
                    elapsed_us(total_start),
                    true,
                );
                Ok(Some(root_hash))
            }
            Err(err) => {
                StateRootApplyMetrics::record_apply(
                    block_index,
                    changes.len(),
                    project_us,
                    elapsed_us(apply_start),
                    elapsed_us(total_start),
                    false,
                );
                Err(err)
            }
        }
    }

    fn contiguous_root_before(mpt: &MptStore<S>, block_index: u32) -> MptResult<Option<UInt256>> {
        match mpt.current_local_root() {
            None if block_index == 0 => Ok(None),
            None => Err(MptError::invalid(format!(
                "non-contiguous state-service MPT update: no local root exists before block {block_index}"
            ))),
            Some((previous_index, root_hash))
                if previous_index.checked_add(1) == Some(block_index) =>
            {
                Ok(Some(root_hash))
            }
            Some((previous_index, _root_hash)) => Err(MptError::invalid(format!(
                "non-contiguous state-service MPT update: current local root index is {previous_index}, next block is {block_index}"
            ))),
        }
    }

    fn apply_snapshot_mpt_changes_with_root<B: neo_storage::CacheRead>(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        snapshot: &DataCache<B>,
        project_us: u64,
        total_start: std::time::Instant,
    ) -> MptResult<Option<UInt256>> {
        let Some(mpt) = self.mpt.as_ref() else {
            return Ok(None);
        };
        let apply_start = std::time::Instant::now();
        let mut change_count = 0usize;
        match mpt.apply_block_changes_lazy(
            block_index,
            root_before,
            snapshot.pending_change_count(),
            |changes| {
                let mut result = Ok(());
                let mut path_scratch = Vec::new();
                snapshot.visit_tracked_items(|key, trackable| {
                    if result.is_err() || key.id() == LEDGER_CONTRACT_ID {
                        return;
                    }
                    result = match trackable.state {
                        TrackState::Added | TrackState::Changed => {
                            let key_bytes = key.as_bytes();
                            let value = trackable.item.value_bytes();
                            change_count += 1;
                            changes.put_with_scratch(
                                key_bytes.as_ref(),
                                value.as_ref(),
                                &mut path_scratch,
                            )
                        }
                        TrackState::Deleted => {
                            let key_bytes = key.as_bytes();
                            change_count += 1;
                            changes
                                .delete_with_scratch(key_bytes.as_ref(), &mut path_scratch)
                                .map(|_| ())
                        }
                        TrackState::None | TrackState::NotFound => Ok(()),
                    };
                });
                result.map(|()| change_count)
            },
        ) {
            Ok(root_hash) => {
                StateRootApplyMetrics::record_apply(
                    block_index,
                    change_count,
                    project_us,
                    elapsed_us(apply_start),
                    elapsed_us(total_start),
                    true,
                );
                Ok(Some(root_hash))
            }
            Err(err) => {
                StateRootApplyMetrics::record_apply(
                    block_index,
                    change_count,
                    project_us,
                    elapsed_us(apply_start),
                    elapsed_us(total_start),
                    false,
                );
                Err(err)
            }
        }
    }

    #[cfg(test)]
    fn mpt_changes_from_snapshot<B: neo_storage::CacheRead>(
        snapshot: &DataCache<B>,
    ) -> Vec<MptChange> {
        let mut changes = Vec::with_capacity(snapshot.pending_change_count());
        Self::push_mpt_changes_from_snapshot(snapshot, &mut changes);
        changes
    }

    fn push_mpt_changes_from_snapshot<B: neo_storage::CacheRead>(
        snapshot: &DataCache<B>,
        changes: &mut Vec<MptChange>,
    ) {
        snapshot.visit_tracked_items(|key, trackable| {
            if key.id() == LEDGER_CONTRACT_ID {
                return;
            }
            match trackable.state {
                TrackState::Added | TrackState::Changed => changes.push(MptChange::Put {
                    key: key.to_array(),
                    value: trackable.item.value_bytes().into_owned(),
                }),
                TrackState::Deleted => changes.push(MptChange::Delete {
                    key: key.to_array(),
                }),
                TrackState::None | TrackState::NotFound => {}
            }
        });
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
    pub fn begin_transaction(&self) -> StateStoreTransaction<S> {
        let candidates = self.inner.read().candidates.clone();
        StateStoreTransaction {
            store: self.clone(),
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
        let removed_index = removed.index();
        let removed_owned_index = guard
            .by_index
            .get(&removed_index)
            .is_some_and(|indexed| indexed.root_hash() == root_hash);
        if removed_owned_index {
            let replacement = guard
                .by_root_hash
                .values()
                .find(|root| root.index() == removed_index)
                .cloned();
            match replacement {
                Some(replacement) => {
                    guard.by_index.insert(removed_index, replacement);
                }
                None => {
                    guard.by_index.remove(&removed_index);
                }
            }
        }
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

fn elapsed_us(start: std::time::Instant) -> u64 {
    start.elapsed().as_micros().min(u64::MAX as u128) as u64
}

impl<S> Clone for StateStore<S>
where
    S: Store,
{
    fn clone(&self) -> Self {
        Self {
            // Clones share the same in-memory root indexes/candidate set:
            // transactions commit against the live store, matching the C#
            // snapshot contract where Commit publishes to the underlying store.
            inner: Arc::clone(&self.inner),
            // The MPT backend is shared, not deep-copied, for the same reason.
            mpt: self.mpt.clone(),
        }
    }
}

#[cfg(test)]
#[path = "../tests/storage/state_store.rs"]
mod tests;
