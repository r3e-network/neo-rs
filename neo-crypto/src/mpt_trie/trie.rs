use super::cache::MptStoreSnapshot;
use super::error::{MptError, MptResult};
use super::node::{Node, BRANCH_CHILD_COUNT, BRANCH_VALUE_INDEX, MAX_KEY_LENGTH, MAX_VALUE_LENGTH};
use super::node_type::NodeType;
use super::MptCache;
use crate::Crypto;
use neo_primitives::UInt256;
use neo_primitives::UINT256_SIZE;
use parking_lot::Mutex;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const CACHE_PREFIX: u8 = 0xf0;

/// Merkle Patricia Trie backed by an [`MptStoreSnapshot`].
pub struct Trie<S>
where
    S: MptStoreSnapshot,
{
    cache: MptCache<S>,
    root: Node,
    full_state: bool,
}

impl<S> Trie<S>
where
    S: MptStoreSnapshot,
{
    /// Creates a new trie instance using the supplied store snapshot and optional root hash.
    pub fn new(store: Arc<S>, root: Option<UInt256>, full_state: bool) -> Self {
        let cache = MptCache::new(store, CACHE_PREFIX);
        let root_node = root.map_or_else(Node::new, Node::new_hash);
        Self {
            cache,
            root: root_node,
            full_state,
        }
    }

    /// Returns a reference to the current root node.
    pub const fn root(&self) -> &Node {
        &self.root
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
        self.cache.commit()
    }

    /// Retrieves the value associated with the supplied key (if present).
    pub fn get(&mut self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        let path = Self::ensure_lookup_key(key)?;
        Self::try_get_node(&mut self.cache, self.full_state, &mut self.root, &path)
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
        Self::ensure_value_length(value)?;
        let leaf = Node::new_leaf(value.to_vec());
        Self::put_internal(
            &mut self.cache,
            self.full_state,
            &mut self.root,
            &path,
            leaf,
        )
    }

    /// Deletes the entry stored under `key`. Returns `true` if an entry was removed.
    pub fn delete(&mut self, key: &[u8]) -> MptResult<bool> {
        let path = Self::ensure_lookup_key(key)?;
        Self::try_delete_node(&mut self.cache, self.full_state, &mut self.root, &path)
    }

    /// Enumerates key/value pairs under the supplied prefix, optionally resuming from `from`.
    pub fn find(&mut self, prefix: &[u8], from: Option<&[u8]>) -> MptResult<Vec<TrieEntry>> {
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
                        return Ok(Vec::new());
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

        let mut results = Vec::new();
        Self::traverse(
            &mut self.cache,
            start,
            resolved_path,
            from_path.as_deref().unwrap_or(&[]),
            offset,
            &mut results,
        )?;
        Ok(results)
    }

    /// Builds a Merkle proof for the supplied key.
    pub fn try_get_proof(&mut self, key: &[u8]) -> MptResult<Option<HashSet<Vec<u8>>>> {
        let path = Self::ensure_lookup_key(key)?;
        let mut proof = HashSet::new();
        if Self::get_proof_node(&mut self.cache, &mut self.root, &path, &mut proof)? {
            Ok(Some(proof))
        } else {
            Ok(None)
        }
    }

    /// Verifies a Merkle proof captured from `try_get_proof` against the provided root hash.
    pub fn verify_proof(root: UInt256, key: &[u8], proof: &HashSet<Vec<u8>>) -> MptResult<Vec<u8>> {
        #[derive(Default)]
        struct ProofStore {
            data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
        }

        impl ProofStore {
            fn new() -> Self {
                Self {
                    data: Mutex::new(HashMap::new()),
                }
            }
        }

        impl MptStoreSnapshot for ProofStore {
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

        let store = Arc::new(ProofStore::new());
        for data in proof {
            let hash_bytes = Crypto::hash256(data);
            let hash = UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?;
            let mut stored = data.clone();
            stored.push(1);
            store.put(Self::cache_key(&hash), stored)?;
        }

        let mut trie = Trie::new(store, Some(root), false);
        trie.get_required(key)
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
                    let child = node.get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::try_get_node(cache, _full_state, child, path)
                } else {
                    let index = path[0] as usize;
                    let child = node.get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch node child index out of bounds"))?;
                    Self::try_get_node(cache, _full_state, child, &path[1..])
                }
            }
            NodeType::ExtensionNode => {
                if path.starts_with(&node.key) {
                    let consumed = node.key.len();
                    let next = node.get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    Self::try_get_node(cache, _full_state, next, &path[consumed..])
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn put_internal(
        cache: &mut MptCache<S>,
        full_state: bool,
        node: &mut Node,
        path: &[u8],
        val: Node,
    ) -> MptResult<()> {
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    if !full_state {
                        let old_hash = node.try_hash()?;
                        cache.delete_node(old_hash)?;
                    }
                    *node = val;
                    cache.put_node(node.clone())?;
                    return Ok(());
                }
                let mut branch = Node::new_branch();
                let old_leaf = std::mem::replace(node, Node::new());
                branch.set_child(BRANCH_VALUE_INDEX, old_leaf);
                let index = path[0] as usize;
                
                // Use get_child_mut for copy-on-write semantics
                let child = branch.get_child_mut(index)
                    .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                Self::put_internal(cache, full_state, child, &path[1..], val)?;
                
                cache.put_node(branch.clone())?;
                *node = branch;
            }
            NodeType::ExtensionNode => {
                if path.starts_with(&node.key) {
                    let consumed = node.key.len();
                    let old_hash = node.try_hash()?;
                    
                    // Use get_next_mut for copy-on-write semantics
                    let next = node.get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    Self::put_internal(cache, full_state, next, &path[consumed..], val)?;
                    
                    if !full_state {
                        cache.delete_node(old_hash)?;
                    }
                    node.set_dirty();
                    cache.put_node(node.clone())?;
                    return Ok(());
                }

                let prefix_len = Self::common_prefix_len(&node.key, path);
                let old_hash = node.try_hash()?;
                if !full_state {
                    cache.delete_node(old_hash)?;
                }

                let original_key = node.key.clone();
                let prefix = original_key[..prefix_len].to_vec();
                let key_remain = original_key[prefix_len..].to_vec();
                let path_remain = path[prefix_len..].to_vec();

                let mut child_branch = Node::new_branch();
                let next_node = node.take_next()
                    .ok_or_else(|| MptError::invalid("extension node missing child"))?;

                if key_remain.len() == 1 {
                    child_branch.set_child(key_remain[0] as usize, next_node);
                } else {
                    let ext_child = Node::new_extension(key_remain[1..].to_vec(), next_node)?;
                    cache.put_node(ext_child.clone())?;
                    child_branch.set_child(key_remain[0] as usize, ext_child);
                }

                if path_remain.is_empty() {
                    let mut value_child = Node::new();
                    Self::put_internal(cache, full_state, &mut value_child, &[], val)?;
                    child_branch.set_child(BRANCH_VALUE_INDEX, value_child);
                } else {
                    let mut value_child = Node::new();
                    Self::put_internal(
                        cache,
                        full_state,
                        &mut value_child,
                        &path_remain[1..],
                        val,
                    )?;
                    child_branch.set_child(path_remain[0] as usize, value_child);
                }

                cache.put_node(child_branch.clone())?;

                if prefix.is_empty() {
                    *node = child_branch;
                } else {
                    let ext = Node::new_extension(prefix, child_branch)?;
                    cache.put_node(ext.clone())?;
                    *node = ext;
                }
            }
            NodeType::BranchNode => {
                let old_hash = node.try_hash()?;
                if path.is_empty() {
                    let child = node.get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::put_internal(cache, full_state, child, path, val)?;
                } else {
                    let index = path[0] as usize;
                    let child = node.get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    Self::put_internal(cache, full_state, child, &path[1..], val)?;
                }
                if !full_state {
                    cache.delete_node(old_hash)?;
                }
                node.set_dirty();
                cache.put_node(node.clone())?;
            }
            NodeType::Empty => {
                if path.is_empty() {
                    *node = val;
                } else {
                    // Persist the newly introduced child node so the extension node's hashed child
                    // reference can be resolved after committing.
                    cache.put_node(val.clone())?;
                    let ext = Node::new_extension(path.to_vec(), val)?;
                    cache.put_node(ext.clone())?;
                    *node = ext;
                }
                if node.node_type == NodeType::LeafNode {
                    cache.put_node(node.clone())?;
                }
            }
            NodeType::HashNode => {
                let resolved = cache
                    .resolve(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during trie put"))?;
                *node = resolved;
                Self::put_internal(cache, full_state, node, path, val)?;
            }
        }
        Ok(())
    }

    fn try_delete_node(
        cache: &mut MptCache<S>,
        full_state: bool,
        node: &mut Node,
        path: &[u8],
    ) -> MptResult<bool> {
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    if !full_state {
                        let old_hash = node.try_hash()?;
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
                    let old_hash = node.try_hash()?;
                    let result = {
                        let next = node.get_next_mut()
                            .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                        Self::try_delete_node(cache, full_state, next, &path[consumed..])?
                    };
                    if !result {
                        return Ok(false);
                    }
                    if !full_state {
                        cache.delete_node(old_hash)?;
                    }
                    
                    // Check if next is now empty
                    let next_is_empty = node.next.as_ref().map_or(true, |n| n.is_empty());
                    if next_is_empty {
                        let next = node.take_next().unwrap_or_default();
                        *node = next;
                        return Ok(true);
                    }
                    
                    // Check if next is now an extension node - merge them
                    let should_merge = node.next.as_ref()
                        .map_or(false, |n| n.node_type == NodeType::ExtensionNode);
                    
                    if should_merge {
                        if !full_state {
                            let child_hash = node.next.as_ref().unwrap().hash();
                            cache.delete_node(child_hash)?;
                        }
                        let next_node = node.take_next().unwrap();
                        node.key.extend_from_slice(&next_node.key);
                        node.next = next_node.next;
                    }
                    
                    node.set_dirty();
                    cache.put_node(node.clone())?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            NodeType::BranchNode => {
                let old_hash = node.try_hash()?;
                let result = if path.is_empty() {
                    let child = node.get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::try_delete_node(cache, full_state, child, path)?
                } else {
                    let index = path[0] as usize;
                    let child = node.get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    Self::try_delete_node(cache, full_state, child, &path[1..])?
                };
                if !result {
                    return Ok(false);
                }
                if !full_state {
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
                    cache.put_node(node.clone())?;
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
                    if !full_state {
                        let child_hash = last_child.try_hash()?;
                        cache.delete_node(child_hash)?;
                    }
                    let mut key = vec![last_index];
                    key.extend_from_slice(&last_child.key);
                    last_child.key = key;
                    last_child.set_dirty();
                    cache.put_node(last_child.clone())?;
                    *node = last_child;
                    Ok(true)
                } else {
                    let ext = Node::new_extension(vec![last_index], last_child)?;
                    cache.put_node(ext.clone())?;
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
                Self::try_delete_node(cache, full_state, node, path)
            }
        }
    }

    fn get_proof_node(
        cache: &mut MptCache<S>,
        node: &mut Node,
        path: &[u8],
        proof: &mut HashSet<Vec<u8>>,
    ) -> MptResult<bool> {
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    proof.insert(node.to_array_without_reference()?);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            NodeType::Empty => Ok(false),
            NodeType::HashNode => {
                let resolved = cache
                    .resolve(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during proof"))?;
                *node = resolved;
                Self::get_proof_node(cache, node, path, proof)
            }
            NodeType::BranchNode => {
                proof.insert(node.to_array_without_reference()?);
                if path.is_empty() {
                    let child = node.get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::get_proof_node(cache, child, path, proof)
                } else {
                    let index = path[0] as usize;
                    let child = node.get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    Self::get_proof_node(cache, child, &path[1..], proof)
                }
            }
            NodeType::ExtensionNode => {
                if path.starts_with(&node.key) {
                    proof.insert(node.to_array_without_reference()?);
                    let consumed = node.key.len();
                    let next = node.get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    Self::get_proof_node(cache, next, &path[consumed..], proof)
                } else {
                    Ok(false)
                }
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
                    Ok((Vec::new(), Some(node.clone())))
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
                    Ok((Vec::new(), Some(node.clone())))
                } else {
                    let nibble = path[0];
                    let child = node.get_child_mut(nibble as usize)
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
                    let start = node.next.as_ref().map(|n| (**n).clone());
                    return Ok((node.key.clone(), start));
                }
                if path.starts_with(&node.key) {
                    let consumed = node.key.len();
                    let next = node.get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    let (mut suffix, start) = Self::seek_node(cache, next, &path[consumed..])?;
                    let mut result = node.key.clone();
                    result.append(&mut suffix);
                    Ok((result, start))
                } else if node.key.starts_with(path) {
                    let start = node.next.as_ref().map(|n| (**n).clone());
                    Ok((node.key.clone(), start))
                } else {
                    Ok((Vec::new(), None))
                }
            }
        }
    }

    fn traverse(
        cache: &mut MptCache<S>,
        node: Option<Node>,
        path: Vec<u8>,
        from: &[u8],
        offset: usize,
        results: &mut Vec<TrieEntry>,
    ) -> MptResult<()> {
        let Some(node) = node else {
            return Ok(());
        };
        match node.node_type {
            NodeType::LeafNode => {
                if from.len() <= offset && path != from {
                    let key = Self::from_nibbles(&path)?;
                    results.push(TrieEntry {
                        key,
                        value: node.value,
                    });
                }
            }
            NodeType::Empty => {}
            NodeType::HashNode => {
                let resolved = cache
                    .resolve(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during trie find"))?;
                Self::traverse(cache, Some(resolved), path, from, offset, results)?;
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
                                let child = node.children[i].as_ref().clone();
                                Self::traverse(
                                    cache,
                                    Some(child),
                                    new_path,
                                    from,
                                    from.len(),
                                    results,
                                )?;
                            }
                            Ordering::Equal => {
                                let mut new_path = path.clone();
                                new_path.push(nibble);
                                let child = node.children[i].as_ref().clone();
                                Self::traverse(
                                    cache,
                                    Some(child),
                                    new_path,
                                    from,
                                    offset + 1,
                                    results,
                                )?;
                            }
                            Ordering::Greater => {}
                        }
                    }
                } else {
                    let child = node.children[BRANCH_VALUE_INDEX].as_ref().clone();
                    Self::traverse(
                        cache,
                        Some(child),
                        path.clone(),
                        from,
                        offset,
                        results,
                    )?;
                    for i in 0..(BRANCH_CHILD_COUNT - 1) {
                        let mut new_path = path.clone();
                        new_path.push(i as u8);
                        let child = node.children[i].as_ref().clone();
                        Self::traverse(
                            cache,
                            Some(child),
                            new_path,
                            from,
                            offset,
                            results,
                        )?;
                    }
                }
            }
            NodeType::ExtensionNode => {
                let mut new_path = path;
                new_path.extend_from_slice(&node.key);
                if offset < from.len() && from[offset..].starts_with(&node.key) {
                    let child = node.next.as_ref().map(|n| (**n).clone());
                    Self::traverse(
                        cache,
                        child,
                        new_path,
                        from,
                        offset + node.key.len(),
                        results,
                    )?;
                } else if from.len() <= offset
                    || node.key.as_slice().cmp(&from[offset..]) == Ordering::Greater
                {
                    let child = node.next.as_ref().map(|n| (**n).clone());
                    Self::traverse(
                        cache,
                        child,
                        new_path,
                        from,
                        from.len(),
                        results,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn ensure_lookup_key(key: &[u8]) -> MptResult<Vec<u8>> {
        let path = Self::to_nibbles(key);
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
        Ok(path)
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
        for byte in bytes {
            result.push(byte >> 4);
            result.push(byte & 0x0F);
        }
        result
    }

    fn from_nibbles(path: &[u8]) -> MptResult<Vec<u8>> {
        if path.len() % 2 != 0 {
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

    fn cache_key(hash: &UInt256) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(1 + UINT256_SIZE);
        buffer.push(CACHE_PREFIX);
        buffer.extend_from_slice(&hash.to_bytes());
        buffer
    }

    /// Optimized common prefix length calculation.
    fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
        a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
    }
}

/// Key/value entry returned by trie enumeration helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrieEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}
