use super::MptCache;
use super::cache::{GlobalNodeCache, MptStoreSnapshot, NODE_KEY_PREFIX, node_key};
use super::error::{MptError, MptResult};
use super::node::{BRANCH_CHILD_COUNT, BRANCH_VALUE_INDEX, MAX_KEY_LENGTH, MAX_VALUE_LENGTH, Node};
use super::node_type::NodeType;
use super::profile::{MptProfile, MptProfileStage, MptProfileTimer};
use super::proof_cache::{
    CommittedRoot, ProofBundle, ProofCache, ProofCacheKey, ProofCacheNamespace,
    speculative_root_error,
};
use crate::Crypto;
use neo_primitives::UInt256;
use parking_lot::Mutex;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

mod lookup;
mod proof;
mod update;

/// Merkle Patricia Trie backed by an [`MptStoreSnapshot`].
pub struct Trie<S>
where
    S: MptStoreSnapshot,
{
    cache: MptCache<S>,
    root: Node,
    full_state: bool,
    profile: Option<Arc<MptProfile>>,
}

impl<S> Trie<S>
where
    S: MptStoreSnapshot,
{
    /// Creates a new trie instance using the supplied store snapshot and optional root hash.
    pub fn new(store: Arc<S>, root: Option<UInt256>, full_state: bool) -> Self {
        let mut cache = MptCache::with_global_cache(store, NODE_KEY_PREFIX, full_state);
        cache.set_full_state(full_state);
        let root_node = root.map_or_else(Node::new, Node::new_hash);
        Self {
            cache,
            root: root_node,
            full_state,
            profile: None,
        }
    }

    /// Creates a new trie instance backed by a store-scoped [`GlobalNodeCache`].
    pub fn new_with_node_cache(
        store: Arc<S>,
        root: Option<UInt256>,
        full_state: bool,
        node_cache: Option<Arc<GlobalNodeCache>>,
    ) -> Self {
        let root_node = root.map_or_else(Node::new, Node::new_hash);
        Self::new_with_root_node(store, root_node, full_state, node_cache)
    }

    /// Creates a new trie instance starting from a pre-resolved root [`Node`].
    pub fn new_with_root_node(
        store: Arc<S>,
        root_node: Node,
        full_state: bool,
        node_cache: Option<Arc<GlobalNodeCache>>,
    ) -> Self {
        let mut cache = MptCache::with_node_cache(store, NODE_KEY_PREFIX, node_cache);
        cache.set_full_state(full_state);
        Self {
            cache,
            root: root_node,
            full_state,
            profile: None,
        }
    }

    /// Attaches an observation-only profiler to this trie and its node cache.
    #[must_use]
    pub fn with_profile(mut self, profile: Arc<MptProfile>) -> Self {
        self.cache.set_profile(Some(Arc::clone(&profile)));
        self.profile = Some(profile);
        self
    }

    /// Returns a reference to the current root node.
    pub const fn root(&self) -> &Node {
        &self.root
    }

    /// Takes ownership of the current in-memory root [`Node`], leaving an empty node in its place.
    pub fn take_root(&mut self) -> Node {
        std::mem::take(&mut self.root)
    }

    /// Returns the current root hash if the trie is not empty.
    pub fn root_hash(&self) -> Option<UInt256> {
        let _timer = MptProfileTimer::new(self.profile.as_deref(), MptProfileStage::RootHash);
        if self.root.is_empty() {
            None
        } else {
            let _ = Self::parallel_hash_subtrees(&self.root, self.profile.as_deref());
            Some(self.root.hash())
        }
    }

    /// Enumerates key/value pairs under the supplied prefix, optionally resuming from `from`.
    pub fn find(&mut self, prefix: &[u8], from: Option<&[u8]>) -> MptResult<Vec<TrieEntry>> {
        if let Some(from_bytes) = from
            && !from_bytes.starts_with(prefix)
        {
            return Err(MptError::invalid(
                "`from` parameter must start with the supplied prefix",
            ));
        }

        let path = Self::ensure_prefix(prefix)?;
        let from_path = from.map(Self::ensure_prefix).transpose()?;

        if let Some(ref from_vec) = from_path
            && from_vec.len() > MAX_KEY_LENGTH
        {
            return Err(MptError::key(
                "`from` key length exceeds maximum".to_string(),
            ));
        }

        let (resolved_path, start) = Self::seek_node(&mut self.cache, &mut self.root, &path)?;

        let mut offset = 0;
        if let Some(ref from_vec) = from_path
            && !from_vec.is_empty()
        {
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
