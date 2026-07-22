use super::cache::MptStoreSnapshot;
use super::cache::PendingNodeFinalization;
use super::error::{MptError, MptResult};
use super::node::{BRANCH_CHILD_COUNT, BRANCH_VALUE_INDEX, MAX_KEY_LENGTH, MAX_VALUE_LENGTH, Node};
use super::node_type::NodeType;
use super::{MPT_NODE_PREFIX, MptCache, MptMutationStats};
use neo_primitives::UInt256;
use std::cmp::Ordering;
use std::sync::Arc;

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

    /// Retrieves the value associated with the supplied key (if present).
    pub fn get(&mut self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        let path = Self::ensure_lookup_key(key)?;
        self.get_with_nibble_path(&path)
    }

    fn get_with_nibble_path(&mut self, path: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Self::try_get_node(&mut self.cache, self.full_state, &mut self.root, path)
    }

    /// Retrieves the value associated with the key, returning an error if it does not exist.
    pub fn get_required(&mut self, key: &[u8]) -> MptResult<Vec<u8>> {
        match self.get(key)? {
            Some(value) => Ok(value),
            None => Err(MptError::key("requested key not present in trie")),
        }
    }

    /// Convenience alias matching the C# `TryGetValue` pattern.
    pub fn try_get_value(&mut self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        self.get(key)
    }

    /// Inserts or updates the value stored under `key`.
    pub fn put(&mut self, key: &[u8], value: &[u8]) -> MptResult<()> {
        let path = Self::ensure_lookup_key(key)?;
        self.put_with_nibble_path(&path, value)
    }

    /// Inserts or updates `value` under `key`, reusing `path_scratch` for
    /// byte-to-nibble expansion.
    ///
    /// This is the hot StateService path: callers that apply many changes to
    /// one trie can keep a single scratch buffer and avoid allocating a fresh
    /// path vector per item while preserving the same MPT node bytes and root.
    pub fn put_with_scratch(
        &mut self,
        key: &[u8],
        value: &[u8],
        path_scratch: &mut Vec<u8>,
    ) -> MptResult<()> {
        Self::fill_lookup_key(key, path_scratch)?;
        self.put_with_nibble_path(path_scratch, value)
    }

    fn put_with_nibble_path(&mut self, path: &[u8], value: &[u8]) -> MptResult<()> {
        Self::ensure_value_length(value)?;
        let leaf = Node::new_leaf(value.to_vec());
        let before = super::metrics::hash_computations();
        let result = Self::put_internal(
            &mut self.cache,
            self.full_state,
            self.defer_node_finalization,
            &mut self.root,
            path,
            leaf,
            1,
        );
        self.cache
            .record_hash_computations(super::metrics::hash_computations().saturating_sub(before));
        result
    }

    /// Deletes the entry stored under `key`. Returns `true` if an entry was removed.
    pub fn delete(&mut self, key: &[u8]) -> MptResult<bool> {
        let path = Self::ensure_lookup_key(key)?;
        self.delete_with_nibble_path(&path)
    }

    /// Deletes `key`, reusing `path_scratch` for byte-to-nibble expansion.
    ///
    /// See [`Trie::put_with_scratch`] for the motivation and parity boundary.
    pub fn delete_with_scratch(
        &mut self,
        key: &[u8],
        path_scratch: &mut Vec<u8>,
    ) -> MptResult<bool> {
        Self::fill_lookup_key(key, path_scratch)?;
        self.delete_with_nibble_path(path_scratch)
    }

    fn delete_with_nibble_path(&mut self, path: &[u8]) -> MptResult<bool> {
        let before = super::metrics::hash_computations();
        let result = Self::try_delete_node(
            &mut self.cache,
            self.full_state,
            self.defer_node_finalization,
            &mut self.root,
            path,
            1,
        );
        self.cache
            .record_hash_computations(super::metrics::hash_computations().saturating_sub(before));
        result
    }

    /// Enumerates key/value pairs under the supplied prefix, optionally resuming from `from`.
    pub fn find(&mut self, prefix: &[u8], from: Option<&[u8]>) -> MptResult<Vec<TrieEntry>> {
        self.find_limited(prefix, from, usize::MAX)
    }

    /// Bounded variant of [`Trie::find`]: traversal stops as soon as `limit`
    /// entries have been produced, without resolving or visiting any further
    /// subtree.
    ///
    /// The C# `Trie.Find` returns a lazy `IEnumerable` whose consumer breaks
    /// out of enumeration once it has seen enough entries; this method is the
    /// eager-Rust equivalent of that early break, so paged callers (e.g. the
    /// `findstates` RPC handler) never materialize an unbounded prefix range.
    pub fn find_limited(
        &mut self,
        prefix: &[u8],
        from: Option<&[u8]>,
        limit: usize,
    ) -> MptResult<Vec<TrieEntry>> {
        let mut results = Vec::new();
        if limit == 0 {
            return Ok(results);
        }
        self.find_visit(prefix, from, |entry| {
            results.push(entry);
            results.len() < limit
        })?;
        Ok(results)
    }

    /// Visitor seam underlying [`Trie::find`] / [`Trie::find_limited`].
    ///
    /// Invokes `visit` for each key/value pair under `prefix` (optionally
    /// resuming strictly after `from`), in the same key order the C#
    /// `Trie.Find` enumerator yields. Returning `false` from the visitor
    /// stops the traversal immediately: no further nodes are resolved from
    /// the backing store and no further entries are visited — the exact
    /// behaviour of breaking out of the C# lazy enumeration.
    pub fn find_visit<F>(&mut self, prefix: &[u8], from: Option<&[u8]>, visit: F) -> MptResult<()>
    where
        F: FnMut(TrieEntry) -> bool,
    {
        let mut visit = visit;
        if let Some(from_bytes) = from {
            if !from_bytes.starts_with(prefix) {
                return Err(MptError::invalid(
                    "`from` parameter must start with the supplied prefix",
                ));
            }
        }

        let path = Self::ensure_prefix(prefix)?;
        let from_path = from.map(Self::ensure_prefix).transpose()?;

        if let Some(ref from_vec) = from_path {
            if from_vec.len() > MAX_KEY_LENGTH {
                return Err(MptError::key(
                    "`from` key length exceeds maximum".to_string(),
                ));
            }
        }

        let (resolved_path, start) = Self::seek_node(&mut self.cache, &mut self.root, &path)?;

        let mut offset = 0;
        if let Some(ref from_vec) = from_path {
            if !from_vec.is_empty() {
                let limit = resolved_path.len().min(from_vec.len());
                for i in 0..limit {
                    if resolved_path[i] < from_vec[i] {
                        return Ok(());
                    }
                    if resolved_path[i] > from_vec[i] {
                        offset = from_vec.len();
                        break;
                    }
                }
                if offset == 0 {
                    offset = resolved_path.len().min(from_vec.len());
                }
            }
        }

        Self::traverse(
            &mut self.cache,
            start,
            resolved_path,
            from_path.as_deref().unwrap_or(&[]),
            offset,
            &mut visit,
        )?;
        Ok(())
    }

    fn try_get_node(
        cache: &mut MptCache<S>,
        _full_state: bool,
        node: &mut Node,
        path: &[u8],
    ) -> MptResult<Option<Vec<u8>>> {
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    Ok(Some(node.value.clone()))
                } else {
                    Ok(None)
                }
            }
            NodeType::Empty => Ok(None),
            NodeType::HashNode => {
                let resolved = cache
                    .resolve(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during trie get"))?;
                *node = resolved;
                Self::try_get_node(cache, _full_state, node, path)
            }
            NodeType::BranchNode => {
                if path.is_empty() {
                    let child = node
                        .get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::try_get_node(cache, _full_state, child, path)
                } else {
                    let index = path[0] as usize;
                    let child = node.get_child_mut(index).ok_or_else(|| {
                        MptError::invalid("branch node child index out of bounds")
                    })?;
                    Self::try_get_node(cache, _full_state, child, &path[1..])
                }
            }
            NodeType::ExtensionNode => {
                if path.starts_with(&node.key) {
                    let consumed = node.key.len();
                    let next = node
                        .get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    Self::try_get_node(cache, _full_state, next, &path[consumed..])
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn previous_hash(
        node: &Node,
        full_state: bool,
        defer_node_finalization: bool,
    ) -> MptResult<Option<UInt256>> {
        if full_state {
            Ok(None)
        } else if defer_node_finalization {
            Ok(node.accounted_hash())
        } else {
            node.try_hash().map(Some)
        }
    }

    fn stage_mutated_node(
        cache: &mut MptCache<S>,
        defer_node_finalization: bool,
        node: &mut Node,
    ) -> MptResult<()> {
        if defer_node_finalization {
            node.set_dirty();
            if cache.defers_intermediate_nodes() {
                cache.defer_intermediate_node(node)?;
            }
            Ok(())
        } else {
            cache.put_node_cached(node)
        }
    }

    fn collect_dirty_subtree(
        cache: &mut MptCache<S>,
        node: &mut Node,
        pending: &mut Vec<PendingNodeFinalization>,
    ) -> MptResult<()> {
        if !node.is_dirty() {
            return Ok(());
        }
        match node.node_type {
            NodeType::BranchNode => {
                for child in &mut node.children {
                    if child.is_dirty() {
                        Self::collect_dirty_subtree(cache, Arc::make_mut(child), pending)?;
                    }
                }
            }
            NodeType::ExtensionNode => {
                if let Some(next) = node.next.as_mut()
                    && next.is_dirty()
                {
                    Self::collect_dirty_subtree(cache, Arc::make_mut(next), pending)?;
                }
            }
            NodeType::LeafNode => {}
            NodeType::HashNode | NodeType::Empty => return Ok(()),
        }
        pending.push(cache.prepare_node_finalization(node)?);
        Ok(())
    }

    fn mark_dirty_subtree_finalized(node: &mut Node) {
        if !node.is_dirty() {
            return;
        }
        match node.node_type {
            NodeType::BranchNode => {
                for child in &mut node.children {
                    if child.is_dirty() {
                        Self::mark_dirty_subtree_finalized(Arc::make_mut(child));
                    }
                }
            }
            NodeType::ExtensionNode => {
                if let Some(next) = node.next.as_mut()
                    && next.is_dirty()
                {
                    Self::mark_dirty_subtree_finalized(Arc::make_mut(next));
                }
            }
            NodeType::LeafNode => {}
            NodeType::HashNode | NodeType::Empty => return,
        }
        if let Some(hash) = node.cached_hash() {
            node.set_finalized_hash(hash);
        }
    }

    fn put_internal(
        cache: &mut MptCache<S>,
        full_state: bool,
        defer_node_finalization: bool,
        node: &mut Node,
        path: &[u8],
        val: Node,
        depth: usize,
    ) -> MptResult<()> {
        cache.record_mutation_depth(depth);
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    if let Some(old_hash) =
                        Self::previous_hash(node, full_state, defer_node_finalization)?
                    {
                        cache.delete_node(old_hash)?;
                    }
                    *node = val;
                    Self::stage_mutated_node(cache, defer_node_finalization, node)?;
                    return Ok(());
                }
                let mut branch = Node::new_branch();
                let old_leaf = std::mem::replace(node, Node::new());
                branch.set_child(BRANCH_VALUE_INDEX, old_leaf);
                let index = path[0] as usize;

                // Use get_child_mut for copy-on-write semantics
                let child = branch
                    .get_child_mut(index)
                    .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                Self::put_internal(
                    cache,
                    full_state,
                    defer_node_finalization,
                    child,
                    &path[1..],
                    val,
                    depth + 1,
                )?;

                Self::stage_mutated_node(cache, defer_node_finalization, &mut branch)?;
                *node = branch;
            }
            NodeType::ExtensionNode => {
                if path.starts_with(&node.key) {
                    let consumed = node.key.len();
                    let old_hash = Self::previous_hash(node, full_state, defer_node_finalization)?;

                    // Use get_next_mut for copy-on-write semantics
                    let next = node
                        .get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    Self::put_internal(
                        cache,
                        full_state,
                        defer_node_finalization,
                        next,
                        &path[consumed..],
                        val,
                        depth + 1,
                    )?;

                    if let Some(old_hash) = old_hash {
                        cache.delete_node(old_hash)?;
                    }
                    Self::stage_mutated_node(cache, defer_node_finalization, node)?;
                    return Ok(());
                }

                let prefix_len = Self::common_prefix_len(&node.key, path);
                if let Some(old_hash) =
                    Self::previous_hash(node, full_state, defer_node_finalization)?
                {
                    cache.delete_node(old_hash)?;
                }

                let original_key = std::mem::take(&mut node.key);
                let prefix = original_key[..prefix_len].to_vec();
                let key_remain = original_key[prefix_len..].to_vec();
                let path_remain = path[prefix_len..].to_vec();

                let mut child_branch = Node::new_branch();
                let next_node = node
                    .take_next()
                    .ok_or_else(|| MptError::invalid("extension node missing child"))?;

                if key_remain.len() == 1 {
                    child_branch.set_child(key_remain[0] as usize, next_node);
                } else {
                    let mut ext_child = Node::new_extension(key_remain[1..].to_vec(), next_node)?;
                    Self::stage_mutated_node(cache, defer_node_finalization, &mut ext_child)?;
                    child_branch.set_child(key_remain[0] as usize, ext_child);
                }

                if path_remain.is_empty() {
                    let mut value_child = Node::new();
                    Self::put_internal(
                        cache,
                        full_state,
                        defer_node_finalization,
                        &mut value_child,
                        &[],
                        val,
                        depth + 1,
                    )?;
                    child_branch.set_child(BRANCH_VALUE_INDEX, value_child);
                } else {
                    let mut value_child = Node::new();
                    Self::put_internal(
                        cache,
                        full_state,
                        defer_node_finalization,
                        &mut value_child,
                        &path_remain[1..],
                        val,
                        depth + 1,
                    )?;
                    child_branch.set_child(path_remain[0] as usize, value_child);
                }

                Self::stage_mutated_node(cache, defer_node_finalization, &mut child_branch)?;

                if prefix.is_empty() {
                    *node = child_branch;
                } else {
                    let mut ext = Node::new_extension(prefix, child_branch)?;
                    Self::stage_mutated_node(cache, defer_node_finalization, &mut ext)?;
                    *node = ext;
                }
            }
            NodeType::BranchNode => {
                let old_hash = Self::previous_hash(node, full_state, defer_node_finalization)?;
                if path.is_empty() {
                    let child = node
                        .get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::put_internal(
                        cache,
                        full_state,
                        defer_node_finalization,
                        child,
                        path,
                        val,
                        depth + 1,
                    )?;
                } else {
                    let index = path[0] as usize;
                    let child = node
                        .get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    Self::put_internal(
                        cache,
                        full_state,
                        defer_node_finalization,
                        child,
                        &path[1..],
                        val,
                        depth + 1,
                    )?;
                }
                if let Some(old_hash) = old_hash {
                    cache.delete_node(old_hash)?;
                }
                Self::stage_mutated_node(cache, defer_node_finalization, node)?;
            }
            NodeType::Empty => {
                if path.is_empty() {
                    *node = val;
                } else {
                    // C# uses extension node to store the remaining path,
                    // pointing to the leaf which stores only the value.
                    let mut leaf = val;
                    Self::stage_mutated_node(cache, defer_node_finalization, &mut leaf)?;
                    let mut ext = Node::new_extension(path.to_vec(), leaf)?;
                    Self::stage_mutated_node(cache, defer_node_finalization, &mut ext)?;
                    *node = ext;
                }
                if node.node_type == NodeType::LeafNode {
                    Self::stage_mutated_node(cache, defer_node_finalization, node)?;
                }
            }
            NodeType::HashNode => {
                let resolved = cache
                    .resolve(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during trie put"))?;
                *node = resolved;
                Self::put_internal(
                    cache,
                    full_state,
                    defer_node_finalization,
                    node,
                    path,
                    val,
                    depth + 1,
                )?;
            }
        }
        Ok(())
    }

    fn try_delete_node(
        cache: &mut MptCache<S>,
        full_state: bool,
        defer_node_finalization: bool,
        node: &mut Node,
        path: &[u8],
        depth: usize,
    ) -> MptResult<bool> {
        cache.record_mutation_depth(depth);
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    if let Some(old_hash) =
                        Self::previous_hash(node, full_state, defer_node_finalization)?
                    {
                        cache.delete_node(old_hash)?;
                    }
                    *node = Node::new();
                    return Ok(true);
                }
                Ok(false)
            }
            NodeType::ExtensionNode => {
                if path.starts_with(&node.key) {
                    let consumed = node.key.len();
                    let old_hash = Self::previous_hash(node, full_state, defer_node_finalization)?;
                    let result = {
                        let next = node
                            .get_next_mut()
                            .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                        Self::try_delete_node(
                            cache,
                            full_state,
                            defer_node_finalization,
                            next,
                            &path[consumed..],
                            depth + 1,
                        )?
                    };
                    if !result {
                        return Ok(false);
                    }
                    if let Some(old_hash) = old_hash {
                        cache.delete_node(old_hash)?;
                    }

                    // Check if next is now empty
                    let next_is_empty = node.next.as_ref().is_none_or(|n| n.is_empty());
                    if next_is_empty {
                        let next = node.take_next().unwrap_or_default();
                        *node = next;
                        return Ok(true);
                    }

                    // Check if next is now an extension node - merge them
                    let should_merge = node
                        .next
                        .as_ref()
                        .is_some_and(|n| n.node_type == NodeType::ExtensionNode);

                    if should_merge {
                        let child = node.next.as_ref().ok_or_else(|| {
                            MptError::invalid("extension node missing child during merge")
                        })?;
                        if let Some(child_hash) =
                            Self::previous_hash(child, full_state, defer_node_finalization)?
                        {
                            cache.delete_node(child_hash)?;
                        }
                        let next_node = node.take_next().ok_or_else(|| {
                            MptError::invalid("extension node missing child during take")
                        })?;
                        node.key.extend_from_slice(&next_node.key);
                        node.next = next_node.next;
                    }

                    Self::stage_mutated_node(cache, defer_node_finalization, node)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            NodeType::BranchNode => {
                let old_hash = Self::previous_hash(node, full_state, defer_node_finalization)?;
                let result = if path.is_empty() {
                    let child = node
                        .get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::try_delete_node(
                        cache,
                        full_state,
                        defer_node_finalization,
                        child,
                        path,
                        depth + 1,
                    )?
                } else {
                    let index = path[0] as usize;
                    let child = node
                        .get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    Self::try_delete_node(
                        cache,
                        full_state,
                        defer_node_finalization,
                        child,
                        &path[1..],
                        depth + 1,
                    )?
                };
                if !result {
                    return Ok(false);
                }
                if let Some(old_hash) = old_hash {
                    cache.delete_node(old_hash)?;
                }

                // Track up to two non-empty children; more than one keeps the branch.
                let mut first_index = None;
                let mut multiple_children = false;
                for i in 0..BRANCH_CHILD_COUNT {
                    if !node.children[i].is_empty() {
                        if first_index.is_some() {
                            multiple_children = true;
                            break;
                        }
                        first_index = Some(i as u8);
                    }
                }

                if multiple_children {
                    Self::stage_mutated_node(cache, defer_node_finalization, node)?;
                    return Ok(true);
                }

                let last_index = first_index.unwrap_or(BRANCH_VALUE_INDEX as u8);
                let last_child_arc = std::mem::replace(
                    &mut node.children[last_index as usize],
                    Arc::new(Node::new()),
                );

                if last_index as usize == BRANCH_VALUE_INDEX {
                    // Only value remains - extract it
                    let last_child = match Arc::try_unwrap(last_child_arc) {
                        Ok(node) => node,
                        Err(arc) => (*arc).clone(),
                    };
                    *node = last_child;
                    return Ok(true);
                }

                // Resolve hash node if necessary
                let mut last_child = if last_child_arc.node_type == NodeType::HashNode {
                    cache.resolve(&last_child_arc.hash())?.ok_or_else(|| {
                        MptError::storage("unable to resolve hash during trie delete")
                    })?
                } else {
                    match Arc::try_unwrap(last_child_arc) {
                        Ok(node) => node,
                        Err(arc) => (*arc).clone(),
                    }
                };

                if last_child.node_type == NodeType::ExtensionNode {
                    if let Some(child_hash) =
                        Self::previous_hash(&last_child, full_state, defer_node_finalization)?
                    {
                        cache.delete_node(child_hash)?;
                    }
                    let mut key = vec![last_index];
                    key.extend_from_slice(&last_child.key);
                    last_child.key = key;
                    Self::stage_mutated_node(cache, defer_node_finalization, &mut last_child)?;
                    *node = last_child;
                    Ok(true)
                } else {
                    let mut ext = Node::new_extension(vec![last_index], last_child)?;
                    Self::stage_mutated_node(cache, defer_node_finalization, &mut ext)?;
                    *node = ext;
                    Ok(true)
                }
            }
            NodeType::Empty => Ok(false),
            NodeType::HashNode => {
                let resolved = cache.resolve(&node.hash())?.ok_or_else(|| {
                    MptError::storage("unable to resolve hash during trie delete")
                })?;
                *node = resolved;
                Self::try_delete_node(
                    cache,
                    full_state,
                    defer_node_finalization,
                    node,
                    path,
                    depth + 1,
                )
            }
        }
    }

    fn seek_node(
        cache: &mut MptCache<S>,
        node: &mut Node,
        path: &[u8],
    ) -> MptResult<(Vec<u8>, Option<Node>)> {
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    Ok((Vec::new(), Some(node.clone_for_traversal())))
                } else {
                    Ok((Vec::new(), None))
                }
            }
            NodeType::Empty => Ok((Vec::new(), None)),
            NodeType::HashNode => {
                let resolved = cache
                    .resolve(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during trie seek"))?;
                *node = resolved;
                Self::seek_node(cache, node, path)
            }
            NodeType::BranchNode => {
                if path.is_empty() {
                    Ok((Vec::new(), Some(node.clone_for_traversal())))
                } else {
                    let nibble = path[0];
                    let child = node
                        .get_child_mut(nibble as usize)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    let (mut suffix, start) = Self::seek_node(cache, child, &path[1..])?;
                    if start.is_none() && suffix.is_empty() {
                        return Ok((Vec::new(), None));
                    }
                    let mut result = Vec::with_capacity(1 + suffix.len());
                    result.push(nibble);
                    result.append(&mut suffix);
                    Ok((result, start))
                }
            }
            NodeType::ExtensionNode => {
                if path.is_empty() {
                    let start = node.next.as_ref().map(|n| n.as_ref().clone_for_traversal());
                    return Ok((node.key.clone(), start));
                }
                if path.starts_with(&node.key) {
                    let consumed = node.key.len();
                    let next = node
                        .get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    let (mut suffix, start) = Self::seek_node(cache, next, &path[consumed..])?;
                    let mut result = node.key.clone();
                    result.append(&mut suffix);
                    Ok((result, start))
                } else if node.key.starts_with(path) {
                    let start = node.next.as_ref().map(|n| n.as_ref().clone_for_traversal());
                    Ok((node.key.clone(), start))
                } else {
                    Ok((Vec::new(), None))
                }
            }
        }
    }

    /// Depth-first enumeration step. Returns `Ok(true)` to continue the
    /// traversal and `Ok(false)` once the visitor has requested a stop;
    /// callers must propagate `false` upward without resolving or visiting
    /// any further sibling subtree (the C# lazy-`IEnumerable` break).
    fn traverse<F>(
        cache: &mut MptCache<S>,
        node: Option<Node>,
        path: Vec<u8>,
        from: &[u8],
        offset: usize,
        visit: &mut F,
    ) -> MptResult<bool>
    where
        F: FnMut(TrieEntry) -> bool,
    {
        let Some(node) = node else {
            return Ok(true);
        };
        match node.node_type {
            NodeType::LeafNode => {
                if from.len() <= offset && path != from {
                    let key = Self::from_nibbles(&path)?;
                    return Ok(visit(TrieEntry {
                        key,
                        value: node.value,
                    }));
                }
                Ok(true)
            }
            NodeType::Empty => Ok(true),
            NodeType::HashNode => {
                let resolved = cache
                    .resolve(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during trie find"))?;
                Self::traverse(cache, Some(resolved), path, from, offset, visit)
            }
            NodeType::BranchNode => {
                if offset < from.len() {
                    for i in 0..(BRANCH_CHILD_COUNT - 1) {
                        let nibble = i as u8;
                        match from[offset].cmp(&nibble) {
                            Ordering::Less => {
                                let mut new_path = path.clone();
                                new_path.push(nibble);
                                // Use Arc::clone for efficient structural sharing
                                let child = node.children[i].as_ref().clone_for_traversal();
                                if !Self::traverse(
                                    cache,
                                    Some(child),
                                    new_path,
                                    from,
                                    from.len(),
                                    visit,
                                )? {
                                    return Ok(false);
                                }
                            }
                            Ordering::Equal => {
                                let mut new_path = path.clone();
                                new_path.push(nibble);
                                let child = node.children[i].as_ref().clone_for_traversal();
                                if !Self::traverse(
                                    cache,
                                    Some(child),
                                    new_path,
                                    from,
                                    offset + 1,
                                    visit,
                                )? {
                                    return Ok(false);
                                }
                            }
                            Ordering::Greater => {}
                        }
                    }
                } else {
                    let child = node.children[BRANCH_VALUE_INDEX]
                        .as_ref()
                        .clone_for_traversal();
                    if !Self::traverse(cache, Some(child), path.clone(), from, offset, visit)? {
                        return Ok(false);
                    }
                    for i in 0..(BRANCH_CHILD_COUNT - 1) {
                        let mut new_path = path.clone();
                        new_path.push(i as u8);
                        let child = node.children[i].as_ref().clone_for_traversal();
                        if !Self::traverse(cache, Some(child), new_path, from, offset, visit)? {
                            return Ok(false);
                        }
                    }
                }
                Ok(true)
            }
            NodeType::ExtensionNode => {
                let mut new_path = path;
                new_path.extend_from_slice(&node.key);
                if offset < from.len() && from[offset..].starts_with(&node.key) {
                    let child = node.next.as_ref().map(|n| n.as_ref().clone_for_traversal());
                    Self::traverse(cache, child, new_path, from, offset + node.key.len(), visit)
                } else if from.len() <= offset
                    || node.key.as_slice().cmp(&from[offset..]) == Ordering::Greater
                {
                    let child = node.next.as_ref().map(|n| n.as_ref().clone_for_traversal());
                    Self::traverse(cache, child, new_path, from, from.len(), visit)
                } else {
                    Ok(true)
                }
            }
        }
    }

    fn ensure_lookup_key(key: &[u8]) -> MptResult<Vec<u8>> {
        let path = Self::to_nibbles(key);
        Self::validate_lookup_path(&path)?;
        Ok(path)
    }

    fn fill_lookup_key(key: &[u8], path: &mut Vec<u8>) -> MptResult<()> {
        Self::to_nibbles_into(key, path);
        Self::validate_lookup_path(path)
    }

    fn validate_lookup_path(path: &[u8]) -> MptResult<()> {
        if path.is_empty() {
            return Err(MptError::key(
                "the key cannot be empty; at least one nibble is required",
            ));
        }
        if path.len() > MAX_KEY_LENGTH {
            return Err(MptError::key(format!(
                "key length {} exceeds maximum {}",
                path.len(),
                MAX_KEY_LENGTH
            )));
        }
        Ok(())
    }

    fn ensure_prefix(key: &[u8]) -> MptResult<Vec<u8>> {
        let path = Self::to_nibbles(key);
        if path.len() > MAX_KEY_LENGTH {
            return Err(MptError::key(format!(
                "key length {} exceeds maximum {}",
                path.len(),
                MAX_KEY_LENGTH
            )));
        }
        Ok(path)
    }

    fn ensure_value_length(value: &[u8]) -> MptResult<()> {
        if value.len() > MAX_VALUE_LENGTH {
            return Err(MptError::invalid(format!(
                "value length {} exceeds maximum {}",
                value.len(),
                MAX_VALUE_LENGTH
            )));
        }
        Ok(())
    }

    /// Optimized nibble conversion with pre-allocated capacity.
    fn to_nibbles(bytes: &[u8]) -> Vec<u8> {
        let mut result = Vec::with_capacity(bytes.len() * 2);
        Self::to_nibbles_into(bytes, &mut result);
        result
    }

    fn to_nibbles_into(bytes: &[u8], out: &mut Vec<u8>) {
        out.clear();
        out.reserve(bytes.len().saturating_mul(2));
        for byte in bytes {
            out.push(byte >> 4);
            out.push(byte & 0x0F);
        }
    }

    fn from_nibbles(path: &[u8]) -> MptResult<Vec<u8>> {
        if !path.len().is_multiple_of(2) {
            return Err(MptError::invalid("nibble path must have even length"));
        }
        let mut key = Vec::with_capacity(path.len() / 2);
        for chunk in path.chunks(2) {
            let hi = chunk[0] << 4;
            let lo = chunk[1] & 0x0F;
            key.push(hi | lo);
        }
        Ok(key)
    }

    /// Optimized common prefix length calculation.
    fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
        a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
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
