use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use neo_primitives::{UINT256_SIZE, UInt256};
use parking_lot::Mutex;

use super::{CACHE_PREFIX, Trie};
use crate::Crypto;
use crate::mpt_trie::MptCache;
use crate::mpt_trie::cache::MptStoreSnapshot;
use crate::mpt_trie::error::{MptError, MptResult};
use crate::mpt_trie::node::{BRANCH_VALUE_INDEX, Node};
use crate::mpt_trie::node_type::NodeType;

impl<S> Trie<S>
where
    S: MptStoreSnapshot,
{
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
            store.put(cache_key(&hash), stored)?;
        }

        let mut trie = Trie::new(store, Some(root), false);
        trie.get_required(key)
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
                    proof.insert(node.to_array_without_reference()?);
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

fn cache_key(hash: &UInt256) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(1 + UINT256_SIZE);
    buffer.push(CACHE_PREFIX);
    buffer.extend_from_slice(&hash.to_bytes());
    buffer
}
