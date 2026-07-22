use super::super::cache::{MptCache, MptStoreSnapshot, PendingNodeFinalization};
use super::super::error::{MptError, MptResult};
use super::super::node::{BRANCH_CHILD_COUNT, BRANCH_VALUE_INDEX, Node};
use super::super::node_type::NodeType;
use super::Trie;
use neo_primitives::UInt256;
use std::sync::Arc;

impl<S> Trie<S>
where
    S: MptStoreSnapshot + 'static,
{
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
        let before = super::super::metrics::hash_computations();
        let result = Self::put_internal(
            &mut self.cache,
            self.full_state,
            self.defer_node_finalization,
            &mut self.root,
            path,
            leaf,
            1,
        );
        self.cache.record_hash_computations(
            super::super::metrics::hash_computations().saturating_sub(before),
        );
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
        let before = super::super::metrics::hash_computations();
        let result = Self::try_delete_node(
            &mut self.cache,
            self.full_state,
            self.defer_node_finalization,
            &mut self.root,
            path,
            1,
        );
        self.cache.record_hash_computations(
            super::super::metrics::hash_computations().saturating_sub(before),
        );
        result
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

    pub(super) fn collect_dirty_subtree(
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

    pub(super) fn mark_dirty_subtree_finalized(node: &mut Node) {
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
}
