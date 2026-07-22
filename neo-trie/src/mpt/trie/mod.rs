//! # Neo MPT Trie
//!
//! ## Boundary
//!
//! This module composes backend-independent Neo MPT root state with lookup,
//! mutation, proof, and commit behavior. It depends only on the snapshot
//! capability exposed by the cache layer.
//!
//! ## Contents
//!
//! Trie construction and root lifecycle, plus focused lookup, mutation, and
//! proof submodules.

use super::cache::MptStoreSnapshot;
use super::error::MptResult;
use super::node::Node;
use super::{MPT_NODE_PREFIX, MptCache, MptMutationStats};
use neo_primitives::UInt256;
use std::sync::Arc;

mod lookup;
mod mutation;
mod proof;

/// Merkle Patricia Trie backed by an [`MptStoreSnapshot`].
pub struct Trie<S>
where
    S: MptStoreSnapshot + 'static,
{
    cache: MptCache<S>,
    root: Node,
    full_state: bool,
    defer_node_finalization: bool,
}

impl<S> Trie<S>
where
    S: MptStoreSnapshot + 'static,
{
    /// Creates a new trie instance using the supplied store snapshot and optional root hash.
    pub fn new(store: Arc<S>, root: Option<UInt256>, full_state: bool) -> Self {
        Self::new_with_modes(store, root, full_state, !full_state, false, false)
    }

    /// Creates a trie optimized for ordered block checkpoints.
    ///
    /// Pruning mode defers both node finalization and reference resolution
    /// across checkpoints. Full-state mode keeps the C#-compatible eager
    /// finalization path by default; callers that explicitly want batched
    /// lookup work while preserving every serialized node can use
    /// [`Trie::new_batch_deferred_full_state`].
    pub fn new_batch(store: Arc<S>, root: Option<UInt256>, full_state: bool) -> Self {
        Self::new_with_modes(store, root, full_state, !full_state, !full_state, false)
    }

    /// Creates an ordered batch trie that defers full-state node finalization.
    ///
    /// Every mutation is recorded as a deferred reference operation, so the
    /// complete C#-compatible raw namespace and reference counts are preserved
    /// while backing lookups are batched at the final commit.
    pub fn new_batch_deferred_full_state(
        store: Arc<S>,
        root: Option<UInt256>,
        full_state: bool,
    ) -> Self {
        if !full_state {
            // The deferred intermediate-node journal is a full-state policy;
            // keep a false flag on the existing pruning batch semantics.
            return Self::new_batch(store, root, false);
        }
        Self::new_with_modes(store, root, full_state, true, true, true)
    }

    fn new_with_modes(
        store: Arc<S>,
        root: Option<UInt256>,
        full_state: bool,
        defer_node_finalization: bool,
        defer_reference_resolution: bool,
        defer_intermediate_nodes: bool,
    ) -> Self {
        let cache = if defer_reference_resolution {
            if defer_intermediate_nodes {
                MptCache::new_deferred_with_intermediate_nodes(store, MPT_NODE_PREFIX)
            } else {
                MptCache::new_deferred(store, MPT_NODE_PREFIX)
            }
        } else {
            MptCache::new(store, MPT_NODE_PREFIX)
        };
        let root_node = root.map_or_else(Node::new, Node::new_hash);
        Self {
            cache,
            root: root_node,
            full_state,
            defer_node_finalization,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_eager(store: Arc<S>, root: Option<UInt256>, full_state: bool) -> Self {
        Self::new_with_modes(store, root, full_state, false, false, false)
    }

    /// Returns a reference to the current root node.
    pub const fn root(&self) -> &Node {
        &self.root
    }

    /// Computes the current root while attributing real hash work to this trie.
    pub fn try_root_hash(&mut self) -> MptResult<UInt256> {
        let before = super::metrics::hash_computations();
        let result = self.root.try_hash();
        self.cache
            .record_hash_computations(super::metrics::hash_computations().saturating_sub(before));
        result
    }

    /// Returns mutation work accumulated since the previous reset.
    pub const fn mutation_stats(&self) -> MptMutationStats {
        self.cache.mutation_stats()
    }

    /// Returns and resets mutation work accumulated by this trie.
    pub fn take_mutation_stats(&mut self) -> MptMutationStats {
        self.cache.take_mutation_stats()
    }

    /// Returns the current root hash if the trie is not empty.
    pub fn root_hash(&self) -> Option<UInt256> {
        if self.root.is_empty() {
            None
        } else {
            Some(self.root.hash())
        }
    }

    /// Commits pending cache changes to the underlying store.
    pub fn commit(&mut self) -> MptResult<()> {
        self.finalize_dirty_nodes()?;
        self.cache.commit()
    }

    /// Enables or disables unresolved deferred-journal export for this trie's
    /// cache commits.
    ///
    /// Only meaningful for deferred full-state batch tries
    /// ([`Trie::new_batch_deferred_full_state`]) whose store carries the
    /// journal to a cursor-resolving backing commit; every other
    /// configuration keeps the classic resolve-then-write flow.
    pub fn set_deferred_journal_export(&mut self, enabled: bool) {
        self.cache.set_deferred_journal_export(enabled);
    }

    /// Finalizes the current root without publishing the accumulated cache overlay.
    pub fn checkpoint(&mut self) -> MptResult<()> {
        self.finalize_dirty_nodes()?;
        self.cache.checkpoint();
        Ok(())
    }

    fn finalize_dirty_nodes(&mut self) -> MptResult<()> {
        if self.defer_node_finalization {
            if self.cache.defers_intermediate_nodes() {
                Self::mark_dirty_subtree_finalized(&mut self.root);
                return Ok(());
            }
            let before = super::metrics::hash_computations();
            let mut pending = Vec::new();
            let result = Self::collect_dirty_subtree(&mut self.cache, &mut self.root, &mut pending)
                .and_then(|()| self.cache.finalize_prepared_nodes(pending));
            self.cache.record_hash_computations(
                super::metrics::hash_computations().saturating_sub(before),
            );
            result?;
            Self::mark_dirty_subtree_finalized(&mut self.root);
        }
        Ok(())
    }
}

/// Key/value entry returned by trie enumeration helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrieEntry {
    /// The nibble-encoded key for this entry.
    pub key: Vec<u8>,
    /// The value stored at this key.
    pub value: Vec<u8>,
}
