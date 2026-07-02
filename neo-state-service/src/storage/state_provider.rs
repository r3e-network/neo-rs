//! Provider-style read views over [`MptStore`] snapshots.
//!
//! State/proof readers need a root gate and trie walk to use the same frozen
//! generation. This module gives callers that boundary explicitly instead of
//! having each RPC or service path coordinate raw snapshots by hand.

use crate::{MptReadSnapshot, MptStore};
use neo_crypto::mpt_trie::{MptResult, Trie};
use neo_primitives::UInt256;
use std::sync::Arc;

/// Opens immutable state views over a state backend.
pub trait StateProviderFactory {
    /// Concrete view type returned by this factory.
    type View: StateView;

    /// Opens the state view recorded for `height`, if the snapshot has that
    /// state-root record.
    fn state_view_at_height(&self, height: u32) -> MptResult<Option<Self::View>>;

    /// Opens a state view rooted at `root_hash`.
    fn state_view_at_root(&self, root_hash: UInt256) -> MptResult<Self::View>;

    /// Opens the current local state view, if the snapshot has a current root.
    fn current_state_view(&self) -> MptResult<Option<Self::View>>;
}

/// Immutable view of a state root and its trie over one frozen generation.
pub trait StateView {
    /// Block height associated with the view when known.
    fn height(&self) -> Option<u32>;

    /// Root hash this view reads.
    fn root_hash(&self) -> UInt256;

    /// Current local root visible to this view's frozen snapshot.
    fn current_local_root_hash(&self) -> Option<UInt256>;

    /// Whether this view's backing store retains historical trie versions.
    fn full_state(&self) -> bool;

    /// Opens a trie over this view's frozen snapshot and root.
    fn open_trie(&self) -> Trie<MptReadSnapshot>;

    /// Reads a storage value directly from this view's trie.
    fn storage_value(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>>;
}

/// [`MptStore`]-backed [`StateProviderFactory`].
pub struct MptStateProviderFactory<'a> {
    store: &'a MptStore,
}

impl<'a> MptStateProviderFactory<'a> {
    /// Creates a provider over `store`.
    pub fn new(store: &'a MptStore) -> Self {
        Self { store }
    }
}

impl StateProviderFactory for MptStateProviderFactory<'_> {
    type View = MptStateView;

    fn state_view_at_height(&self, height: u32) -> MptResult<Option<Self::View>> {
        let snapshot = self.store.snapshot();
        let Some(root) = snapshot.get_state_root(height) else {
            return Ok(None);
        };
        Ok(Some(MptStateView::new(
            snapshot,
            Some(height),
            *root.root_hash(),
        )))
    }

    fn state_view_at_root(&self, root_hash: UInt256) -> MptResult<Self::View> {
        let snapshot = self.store.snapshot();
        let height = match snapshot.current_local_root_hash() {
            Some(current) if current == root_hash => snapshot.current_local_root_index(),
            _ => None,
        };
        Ok(MptStateView::new(snapshot, height, root_hash))
    }

    fn current_state_view(&self) -> MptResult<Option<Self::View>> {
        let snapshot = self.store.snapshot();
        let Some(height) = snapshot.current_local_root_index() else {
            return Ok(None);
        };
        let Some(root) = snapshot.get_state_root(height) else {
            return Ok(None);
        };
        Ok(Some(MptStateView::new(
            snapshot,
            Some(height),
            *root.root_hash(),
        )))
    }
}

/// [`MptReadSnapshot`]-backed immutable state view.
pub struct MptStateView {
    snapshot: Arc<MptReadSnapshot>,
    height: Option<u32>,
    root_hash: UInt256,
}

impl MptStateView {
    fn new(snapshot: Arc<MptReadSnapshot>, height: Option<u32>, root_hash: UInt256) -> Self {
        Self {
            snapshot,
            height,
            root_hash,
        }
    }
}

impl StateView for MptStateView {
    fn height(&self) -> Option<u32> {
        self.height
    }

    fn root_hash(&self) -> UInt256 {
        self.root_hash
    }

    fn current_local_root_hash(&self) -> Option<UInt256> {
        self.snapshot.current_local_root_hash()
    }

    fn full_state(&self) -> bool {
        self.snapshot.full_state()
    }

    fn open_trie(&self) -> Trie<MptReadSnapshot> {
        self.snapshot.open_trie(Some(self.root_hash))
    }

    fn storage_value(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        self.open_trie().try_get_value(key)
    }
}

#[cfg(test)]
#[path = "../tests/storage/state_provider.rs"]
mod tests;
