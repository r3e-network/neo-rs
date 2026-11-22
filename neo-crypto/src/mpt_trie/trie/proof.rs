use super::*;
impl<S> Trie<S>
where
    S: MptStoreSnapshot,
{
    /// Returns the committed-root marker for this trie, or `None` while the
    /// in-memory root is still mutable.
    ///
    /// A dirty root means uncommitted updates are pending, so a proof built
    /// against it would describe state that may never be persisted. Callers
    /// that already hold a persisted root record can bypass this check with
    /// [`CommittedRoot::from_committed`].
    #[must_use]
    pub fn committed_root(&self) -> Option<CommittedRoot> {
        if self.root.is_dirty() {
            return None;
        }
        self.root_hash().map(CommittedRoot::from_committed)
    }

    /// Builds a Merkle proof for the supplied key.
    pub fn try_get_proof(&mut self, key: &[u8]) -> MptResult<Option<HashSet<Vec<u8>>>> {
        let path = Self::ensure_lookup_key(key)?;
        let mut nodes = Vec::new();
        if Self::get_proof_node(&mut self.cache, &mut self.root, &path, &mut nodes)? {
            Ok(Some(nodes.into_iter().collect()))
        } else {
            Ok(None)
        }
    }

    /// Builds a Merkle proof for `key` under a committed root, reusing the
    /// cached bundle when one exists.
    ///
    /// `committed` must name the finalized root this trie is pinned to and
    /// `namespace` must identify the node store backing it. Both are part of
    /// the cache key, so proofs for distinct roots, distinct stores or distinct
    /// keys can never be confused.
    ///
    /// # Errors
    ///
    /// Rejects a mutable (`dirty`) root, an empty trie and a marker that does
    /// not match the trie's own root hash, so a speculative root can never be
    /// cached. A key that is absent from the trie yields `Ok(None)` and is not
    /// cached.
    pub fn get_proof_cached(
        &mut self,
        cache: &ProofCache,
        committed: CommittedRoot,
        namespace: ProofCacheNamespace,
        key: &[u8],
    ) -> MptResult<Option<Arc<ProofBundle>>> {
        if self.root.is_dirty() {
            return Err(speculative_root_error(
                "trie root has uncommitted updates and is not immutable",
            ));
        }
        let root_hash = self
            .root_hash()
            .ok_or_else(|| speculative_root_error("trie is empty and has no root"))?;
        if root_hash != committed.hash() {
            return Err(speculative_root_error(format_args!(
                "committed root {} does not match trie root {root_hash}",
                committed.hash()
            )));
        }
        // Validate the key before inserting anything, so malformed requests can
        // never occupy a cache slot.
        let path = Self::ensure_lookup_key(key)?;
        let cache_key = ProofCacheKey::new(committed, namespace, key);
        cache.get_or_construct(&cache_key, || {
            let mut nodes = Vec::new();
            if Self::get_proof_node(&mut self.cache, &mut self.root, &path, &mut nodes)? {
                Ok(Some(nodes))
            } else {
                Ok(None)
            }
        })
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
            store.put(node_key(&hash), stored)?;
        }

        let mut trie = Trie::new(store, Some(root), false);
        trie.get_required(key)
    }

    fn get_proof_node(
        cache: &mut MptCache<S>,
        node: &mut Node,
        path: &[u8],
        proof: &mut Vec<Vec<u8>>,
    ) -> MptResult<bool> {
        match node.node_type {
            NodeType::LeafNode => {
                if path.is_empty() {
                    proof.push(node.to_array_without_reference()?);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            NodeType::Empty => Ok(false),
            NodeType::HashNode => {
                let resolved = cache
                    .resolve_node(&node.hash())?
                    .ok_or_else(|| MptError::storage("unable to resolve hash during proof"))?;
                *node = resolved;
                Self::get_proof_node(cache, node, path, proof)
            }
            NodeType::BranchNode => {
                proof.push(node.to_array_without_reference()?);
                if path.is_empty() {
                    let child = node
                        .get_child_mut(BRANCH_VALUE_INDEX)
                        .ok_or_else(|| MptError::invalid("branch node missing value child"))?;
                    Self::get_proof_node(cache, child, path, proof)
                } else {
                    let index = path[0] as usize;
                    let child = node
                        .get_child_mut(index)
                        .ok_or_else(|| MptError::invalid("branch child index out of bounds"))?;
                    Self::get_proof_node(cache, child, &path[1..], proof)
                }
            }
            NodeType::ExtensionNode => {
                if path.starts_with(&node.key) {
                    proof.push(node.to_array_without_reference()?);
                    let consumed = node.key.len();
                    let next = node
                        .get_next_mut()
                        .ok_or_else(|| MptError::invalid("extension node missing child"))?;
                    Self::get_proof_node(cache, next, &path[consumed..], proof)
                } else {
                    Ok(false)
                }
            }
        }
    }
}
