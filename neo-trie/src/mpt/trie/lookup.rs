use super::super::cache::{MptCache, MptStoreSnapshot};
use super::super::error::{MptError, MptResult};
use super::super::node::{
    BRANCH_CHILD_COUNT, BRANCH_VALUE_INDEX, MAX_KEY_LENGTH, MAX_VALUE_LENGTH, Node,
};
use super::super::node_type::NodeType;
use super::{Trie, TrieEntry};
use std::cmp::Ordering;

impl<S> Trie<S>
where
    S: MptStoreSnapshot + 'static,
{
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

    pub(super) fn ensure_lookup_key(key: &[u8]) -> MptResult<Vec<u8>> {
        let path = Self::to_nibbles(key);
        Self::validate_lookup_path(&path)?;
        Ok(path)
    }

    pub(super) fn fill_lookup_key(key: &[u8], path: &mut Vec<u8>) -> MptResult<()> {
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

    pub(super) fn ensure_value_length(value: &[u8]) -> MptResult<()> {
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
    pub(super) fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
        a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
    }
}
