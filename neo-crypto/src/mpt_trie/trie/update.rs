use super::*;
impl<S> Trie<S>
where
    S: MptStoreSnapshot,
{
    /// Flushes all in-memory dirty nodes into the underlying [`MptCache`].
    pub fn flush_dirty(&mut self) -> MptResult<()> {
        self.flush_dirty_with_parallelism(true)
    }

    fn flush_dirty_with_parallelism(&mut self, parallel_hash: bool) -> MptResult<()> {
        if !self.root.is_dirty() {
            return Ok(());
        }
        if parallel_hash {
            Self::parallel_hash_subtrees(&self.root, self.profile.as_deref())?;
        } else {
            Self::serial_hash_subtrees(&self.root, self.profile.as_deref())?;
        }
        let _ = Self::flush_dirty_node(&mut self.cache, &mut self.root, self.profile.as_deref())?;
        Ok(())
    }

    /// Commits pending cache changes to the underlying store.
    ///
    /// # Security Note
    /// In debug builds, this performs post-commit verification to ensure the
    /// in-memory root hash matches the persisted state. This catches partial
    /// commit failures that could lead to consensus divergence.
    pub fn commit(&mut self) -> MptResult<()> {
        self.commit_with_parallelism(true)
    }

    /// Commits with explicit control over dirty-subtree hash scheduling.
    /// The resulting nodes and root are identical; small updates can avoid
    /// parallel dispatcher overhead without changing larger updates.
    pub fn commit_with_parallelism(&mut self, parallel_hash: bool) -> MptResult<()> {
        let profile = self.profile.clone();
        let _timer = MptProfileTimer::new(profile.as_deref(), MptProfileStage::Commit);
        self.flush_dirty_with_parallelism(parallel_hash)?;

        #[cfg(debug_assertions)]
        let expected_root = self.root_hash();

        self.cache.commit()?;

        #[cfg(debug_assertions)]
        {
            // Post-commit verification: ensure root hash consistency
            let actual_root = self.root_hash();
            if expected_root != actual_root {
                return Err(MptError::invalid(
                    "root hash mismatch after commit - cache/store inconsistency detected",
                ));
            }
        }

        Ok(())
    }

    /// Inserts or updates the value stored under `key`.
    pub fn put(&mut self, key: &[u8], value: &[u8]) -> MptResult<()> {
        let _timer = MptProfileTimer::new(self.profile.as_deref(), MptProfileStage::Update);
        let path = Self::ensure_lookup_key(key)?;
        Self::ensure_value_length(value)?;
        let leaf = Node::new_leaf(value.to_vec());
        Self::put_internal(
            &mut self.cache,
            self.full_state,
            self.profile.as_deref(),
            &mut self.root,
            &path,
            leaf,
        )
    }

    /// Deletes the entry stored under `key`. Returns `true` if an entry was removed.
    pub fn delete(&mut self, key: &[u8]) -> MptResult<bool> {
        let _timer = MptProfileTimer::new(self.profile.as_deref(), MptProfileStage::Update);
        let path = Self::ensure_lookup_key(key)?;
        Self::try_delete_node(
            &mut self.cache,
            self.full_state,
            self.profile.as_deref(),
            &mut self.root,
            &path,
        )
    }

    fn serial_hash_subtrees(node: &Node, profile: Option<&MptProfile>) -> MptResult<()> {
        let _timer = MptProfileTimer::new(profile, MptProfileStage::Hash);
        Self::serial_hash_subtrees_depth(node, profile)
    }

    fn serial_hash_subtrees_depth(node: &Node, profile: Option<&MptProfile>) -> MptResult<()> {
        if !node.is_dirty() {
            return Ok(());
        }
        match node.node_type {
            NodeType::ExtensionNode => {
                if let Some(ref next) = node.next {
                    Self::serial_hash_subtrees_depth(next, profile)?;
                }
            }
            NodeType::BranchNode => {
                for child in &node.children {
                    if child.is_dirty() {
                        Self::serial_hash_subtrees_depth(child, profile)?;
                    }
                }
            }
            _ => {}
        }
        let _ = node.try_hash_profiled(profile)?;
        Ok(())
    }

    pub(super) fn parallel_hash_subtrees(
        node: &Node,
        profile: Option<&MptProfile>,
    ) -> MptResult<()> {
        let _timer = MptProfileTimer::new(profile, MptProfileStage::Hash);
        Self::parallel_hash_subtrees_depth(node, 0, profile)
    }

    fn parallel_hash_subtrees_depth(
        node: &Node,
        fanout_depth: usize,
        profile: Option<&MptProfile>,
    ) -> MptResult<()> {
        if !node.is_dirty() {
            return Ok(());
        }
        match node.node_type {
            NodeType::ExtensionNode => {
                if let Some(ref next) = node.next {
                    Self::parallel_hash_subtrees_depth(next, fanout_depth, profile)?;
                }
                let _ = node.try_hash_profiled(profile)?;
                Ok(())
            }
            NodeType::BranchNode if fanout_depth < 2 => {
                let dirty_count = node.children.iter().filter(|c| c.is_dirty()).count();
                if dirty_count >= 2 {
                    use rayon::prelude::*;
                    node.children
                        .par_iter()
                        .filter(|c| c.is_dirty())
                        .try_for_each(|child| {
                            Self::parallel_hash_subtrees_depth(child, fanout_depth + 1, profile)
                        })?;
                } else {
                    for child in &node.children {
                        if child.is_dirty() {
                            Self::parallel_hash_subtrees_depth(child, fanout_depth, profile)?;
                        }
                    }
                }
                let _ = node.try_hash_profiled(profile)?;
                Ok(())
            }
            _ => {
                let _ = node.try_hash_profiled(profile)?;
                Ok(())
            }
        }
    }

    fn cache_flushed_node(
        cache: &mut MptCache<S>,
        node: &mut Node,
        known_absent_in_store: bool,
    ) -> MptResult<bool> {
        let preimage = node.take_hash_preimage();
        node.clear_dirty();
        let cached = node.clone();
        if let Some(preimage) = preimage {
            cached.set_hash_preimage(preimage);
        }
        cache.put_node_with_hint(cached, known_absent_in_store)
    }

    fn flush_dirty_node(
        cache: &mut MptCache<S>,
        node: &mut Node,
        profile: Option<&MptProfile>,
    ) -> MptResult<bool> {
        if !node.is_dirty() {
            return Ok(false);
        }
        match node.node_type {
            NodeType::Empty | NodeType::HashNode => {
                node.clear_dirty();
                Ok(false)
            }
            NodeType::LeafNode => {
                let _ = node.try_hash_profiled(profile)?;
                Self::cache_flushed_node(cache, node, false)
            }
            NodeType::ExtensionNode => {
                let mut child_absent = false;
                if let Some(ref mut next_arc) = node.next
                    && next_arc.is_dirty()
                {
                    let next = Arc::make_mut(next_arc);
                    child_absent = Self::flush_dirty_node(cache, next, profile)?;
                }
                let _ = node.try_hash_profiled(profile)?;
                Self::cache_flushed_node(cache, node, child_absent)
            }
            NodeType::BranchNode => {
                let mut any_child_absent = false;
                for child_arc in &mut node.children {
                    if child_arc.is_dirty() {
                        let child = Arc::make_mut(child_arc);
                        if Self::flush_dirty_node(cache, child, profile)? {
                            any_child_absent = true;
                        }
                    }
                }
                let _ = node.try_hash_profiled(profile)?;
                Self::cache_flushed_node(cache, node, any_child_absent)
            }
        }
    }

    fn put_internal(
        cache: &mut MptCache<S>,
        full_state: bool,
        profile: Option<&MptProfile>,
        node: &mut Node,
        path: &[u8],
        val: Node,
    ) -> MptResult<()> {
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    if !node.is_dirty() && !full_state {
                        let old_hash = node.try_hash_profiled(profile)?;
                        cache.delete_node(old_hash)?;
                    }
                    *node = val;
                    return Ok(());
                }
                let mut branch = Node::new_branch();
                let old_leaf = std::mem::take(node);
                branch.set_child(BRANCH_VALUE_INDEX, old_leaf);
                let index = path[0] as usize;

                // Use get_child_mut for copy-on-write semantics
                let child = branch
                    .get_child_mut(index)
                    .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                Self::put_internal(cache, full_state, profile, child, &path[1..], val)?;

                *node = branch;
            }
            NodeType::ExtensionNode => {
                if path.starts_with(&node.key) {
                    let consumed = node.key.len();
                    let old_hash = if !node.is_dirty() && !full_state {
                        Some(node.try_hash_profiled(profile)?)
                    } else {
                        None
                    };

                    // Use get_next_mut for copy-on-write semantics
                    let next = node
                        .get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    Self::put_internal(cache, full_state, profile, next, &path[consumed..], val)?;

                    if let Some(old_hash) = old_hash {
                        cache.delete_node(old_hash)?;
                    }
                    node.set_dirty();
                    return Ok(());
                }

                let prefix_len = Self::common_prefix_len(&node.key, path);
                if !node.is_dirty() && !full_state {
                    let old_hash = node.try_hash_profiled(profile)?;
                    cache.delete_node(old_hash)?;
                }

                let original_key = node.key.clone();
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
                    let ext_child = Node::new_extension(key_remain[1..].to_vec(), next_node)?;
                    child_branch.set_child(key_remain[0] as usize, ext_child);
                }

                if path_remain.is_empty() {
                    let mut value_child = Node::new();
                    Self::put_internal(cache, full_state, profile, &mut value_child, &[], val)?;
                    child_branch.set_child(BRANCH_VALUE_INDEX, value_child);
                } else {
                    let mut value_child = Node::new();
                    Self::put_internal(
                        cache,
                        full_state,
                        profile,
                        &mut value_child,
                        &path_remain[1..],
                        val,
                    )?;
                    child_branch.set_child(path_remain[0] as usize, value_child);
                }

                if prefix.is_empty() {
                    *node = child_branch;
                } else {
                    let ext = Node::new_extension(prefix, child_branch)?;
                    *node = ext;
                }
            }
            NodeType::BranchNode => {
                let old_hash = if !node.is_dirty() && !full_state {
                    Some(node.try_hash_profiled(profile)?)
                } else {
                    None
                };
                if path.is_empty() {
                    let child = node
                        .get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::put_internal(cache, full_state, profile, child, path, val)?;
                } else {
                    let index = path[0] as usize;
                    let child = node
                        .get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    Self::put_internal(cache, full_state, profile, child, &path[1..], val)?;
                }
                if let Some(old_hash) = old_hash {
                    cache.delete_node(old_hash)?;
                }
                node.set_dirty();
            }
            NodeType::Empty => {
                if path.is_empty() {
                    *node = val;
                } else {
                    // C# uses extension node to store the remaining path,
                    // pointing to the leaf which stores only the value.
                    let ext = Node::new_extension(path.to_vec(), val)?;
                    *node = ext;
                }
            }
            NodeType::HashNode => {
                let resolved = cache
                    .resolve_node(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during trie put"))?;
                *node = resolved;
                Self::put_internal(cache, full_state, profile, node, path, val)?;
            }
        }
        Ok(())
    }

    fn try_delete_node(
        cache: &mut MptCache<S>,
        full_state: bool,
        profile: Option<&MptProfile>,
        node: &mut Node,
        path: &[u8],
    ) -> MptResult<bool> {
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    if !node.is_dirty() && !full_state {
                        let old_hash = node.try_hash_profiled(profile)?;
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
                    let old_hash = if !node.is_dirty() && !full_state {
                        Some(node.try_hash_profiled(profile)?)
                    } else {
                        None
                    };
                    let result = {
                        let next = node
                            .get_next_mut()
                            .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                        Self::try_delete_node(cache, full_state, profile, next, &path[consumed..])?
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
                        let next_node = node.take_next().ok_or_else(|| {
                            MptError::invalid("extension node missing child during take")
                        })?;
                        if !next_node.is_dirty() && !full_state {
                            let child_hash = next_node.try_hash_profiled(profile)?;
                            cache.delete_node(child_hash)?;
                        }
                        node.key.extend_from_slice(&next_node.key);
                        node.next = next_node.next;
                    }

                    node.set_dirty();
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            NodeType::BranchNode => {
                let old_hash = if !node.is_dirty() && !full_state {
                    Some(node.try_hash_profiled(profile)?)
                } else {
                    None
                };
                let result = if path.is_empty() {
                    let child = node
                        .get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::try_delete_node(cache, full_state, profile, child, path)?
                } else {
                    let index = path[0] as usize;
                    let child = node
                        .get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    Self::try_delete_node(cache, full_state, profile, child, &path[1..])?
                };
                if !result {
                    return Ok(false);
                }
                if let Some(old_hash) = old_hash {
                    cache.delete_node(old_hash)?;
                }

                // Collect non-empty child indexes
                let mut indexes = Vec::with_capacity(2);
                for i in 0..BRANCH_CHILD_COUNT {
                    if !node.children[i].is_empty() {
                        indexes.push(i as u8);
                        if indexes.len() > 1 {
                            break; // Early exit if more than one child
                        }
                    }
                }

                if indexes.len() > 1 {
                    node.set_dirty();
                    return Ok(true);
                }

                let last_index = indexes.first().copied().unwrap_or(BRANCH_VALUE_INDEX as u8);
                let last_child_arc = Arc::clone(&node.children[last_index as usize]);

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
                    cache.resolve_node(&last_child_arc.hash())?.ok_or_else(|| {
                        MptError::storage("unable to resolve hash during trie delete")
                    })?
                } else {
                    match Arc::try_unwrap(last_child_arc) {
                        Ok(node) => node,
                        Err(arc) => (*arc).clone(),
                    }
                };

                if last_child.node_type == NodeType::ExtensionNode {
                    if !last_child.is_dirty() && !full_state {
                        let child_hash = last_child.try_hash_profiled(profile)?;
                        cache.delete_node(child_hash)?;
                    }
                    let mut key = vec![last_index];
                    key.extend_from_slice(&last_child.key);
                    last_child.key = key;
                    last_child.set_dirty();
                    *node = last_child;
                    Ok(true)
                } else {
                    let ext = Node::new_extension(vec![last_index], last_child)?;
                    *node = ext;
                    Ok(true)
                }
            }
            NodeType::Empty => Ok(false),
            NodeType::HashNode => {
                let resolved = cache.resolve_node(&node.hash())?.ok_or_else(|| {
                    MptError::storage("unable to resolve hash during trie delete")
                })?;
                *node = resolved;
                Self::try_delete_node(cache, full_state, profile, node, path)
            }
        }
    }
}
