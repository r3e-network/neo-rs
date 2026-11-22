use super::*;
impl<S> Trie<S>
where
    S: MptStoreSnapshot,
{
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
                    .resolve_node(&node.hash())?
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

    pub(super) fn seek_node(
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
                    .resolve_node(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during trie seek"))?;
                *node = resolved;
                Self::seek_node(cache, node, path)
            }
            NodeType::BranchNode => {
                if path.is_empty() {
                    Ok((Vec::new(), Some(node.clone())))
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
                    let start = node.next.as_ref().map(|n| (**n).clone());
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
                    let start = node.next.as_ref().map(|n| (**n).clone());
                    Ok((node.key.clone(), start))
                } else {
                    Ok((Vec::new(), None))
                }
            }
        }
    }

    pub(super) fn traverse(
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
                    .resolve_node(&node.hash())?
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
                    Self::traverse(cache, Some(child), path.clone(), from, offset, results)?;
                    for i in 0..(BRANCH_CHILD_COUNT - 1) {
                        let mut new_path = path.clone();
                        new_path.push(i as u8);
                        let child = node.children[i].as_ref().clone();
                        Self::traverse(cache, Some(child), new_path, from, offset, results)?;
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
                    Self::traverse(cache, child, new_path, from, from.len(), results)?;
                }
            }
        }
        Ok(())
    }
}
