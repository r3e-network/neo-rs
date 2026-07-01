use super::error::{MptError, MptResult};
use super::node::Node;
use neo_io::{MemoryReader, Serializable, SerializableExtensions};
use neo_primitives::UINT256_SIZE;
use neo_primitives::UInt256;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackState {
    None,
    Added,
    Changed,
    Deleted,
}

/// Abstraction over the persistence snapshot used by the trie cache.
pub trait MptStoreSnapshot: Send + Sync {
    /// Retrieves the serialized node associated with the specified key.
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>>;

    /// Persists the serialized node for the supplied key.
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()>;

    /// Removes the value associated with the supplied key.
    fn delete(&self, key: Vec<u8>) -> MptResult<()>;

    /// Applies a batch of serialized-node mutations.
    ///
    /// The default preserves the original per-entry semantics. Hot write-batch
    /// stores can override this to merge bookkeeping/lock acquisition.
    fn apply_overlay(&self, overlay: Vec<(Vec<u8>, Option<Vec<u8>>)>) -> MptResult<()> {
        for (key, value) in overlay {
            match value {
                Some(value) => self.put(key, value)?,
                None => self.delete(key)?,
            }
        }
        Ok(())
    }
}

struct MptTrackable {
    node: Option<Node>,
    state: TrackState,
}

impl MptTrackable {
    const fn new(node: Option<Node>) -> Self {
        Self {
            node,
            state: TrackState::None,
        }
    }
}

/// Write-through cache mirroring the behaviour of the C# implementation.
///
/// Nodes are addressed by their hash and reference counted so that multiple
/// parents can point to the same subtree while it lives inside the cache.
pub struct MptCache<S>
where
    S: MptStoreSnapshot,
{
    store: Arc<S>,
    prefix: u8,
    entries: HashMap<UInt256, MptTrackable>,
}

impl<S> MptCache<S>
where
    S: MptStoreSnapshot,
{
    /// Creates a new cache backed by the given store snapshot with the specified key prefix.
    pub fn new(store: Arc<S>, prefix: u8) -> Self {
        Self {
            store,
            prefix,
            entries: HashMap::new(),
        }
    }

    /// Resolves the node identified by the supplied hash if present either in the
    /// in-memory cache or the underlying store.
    pub fn resolve(&mut self, hash: &UInt256) -> MptResult<Option<Node>> {
        let entry = self.resolve_internal(hash)?;
        Ok(entry.node.clone())
    }

    /// Adds or updates the supplied node inside the cache.
    pub fn put_node(&mut self, node: Node) -> MptResult<()> {
        let hash = node.try_hash()?;
        self.put_node_with_hash(node, hash)
    }

    /// Adds or updates the supplied node inside the cache while keeping the
    /// caller's node hash cached.
    pub(crate) fn put_node_cached(&mut self, node: &mut Node) -> MptResult<()> {
        node.set_dirty();
        let hash = node.try_hash()?;
        self.put_node_with_hash(node.clone_with_cached_hash(), hash)
    }

    fn put_node_with_hash(&mut self, node: Node, hash: UInt256) -> MptResult<()> {
        let entry = self.resolve_internal(&hash)?;

        if let Some(ref mut existing) = entry.node {
            existing.reference = existing.reference.saturating_add(1);
            entry.state = TrackState::Changed;
        } else {
            let mut stored = node;
            stored.reference = 1;
            entry.node = Some(stored);
            entry.state = TrackState::Added;
        }
        Ok(())
    }

    /// Decrements the reference count for the node or marks it for deletion when it
    /// is no longer referenced.
    pub fn delete_node(&mut self, hash: UInt256) -> MptResult<()> {
        let entry = self.resolve_internal(&hash)?;
        let Some(node) = entry.node.as_mut() else {
            return Ok(());
        };
        if node.reference > 1 {
            node.reference -= 1;
            entry.state = TrackState::Changed;
        } else {
            entry.node = None;
            entry.state = TrackState::Deleted;
        }
        Ok(())
    }

    /// Flushes the pending changes to the underlying store.
    pub fn commit(&mut self) -> MptResult<()> {
        let mut overlay = Vec::with_capacity(self.entries.len());
        for (hash, entry) in &self.entries {
            match entry.state {
                TrackState::None => {}
                TrackState::Added | TrackState::Changed => {
                    let node = entry
                        .node
                        .as_ref()
                        .ok_or_else(|| MptError::invalid("cache entry missing node"))?;
                    let data = node.to_array().map_err(MptError::from)?;
                    overlay.push((self.key(hash), Some(data)));
                }
                TrackState::Deleted => {
                    overlay.push((self.key(hash), None));
                }
            }
        }
        self.store.apply_overlay(overlay)?;
        self.entries.clear();
        Ok(())
    }

    fn resolve_internal(&mut self, hash: &UInt256) -> MptResult<&mut MptTrackable> {
        let store = Arc::clone(&self.store);
        let prefix = self.prefix;

        match self.entries.entry(*hash) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let node = Self::load_from_store_snapshot(&store, prefix, hash)?;
                Ok(entry.insert(MptTrackable::new(node)))
            }
        }
    }

    fn load_from_store_snapshot(store: &S, prefix: u8, hash: &UInt256) -> MptResult<Option<Node>> {
        let key = Self::key_bytes(prefix, hash);
        let Some(bytes) = store.try_get(&key)? else {
            return Ok(None);
        };
        let mut reader = MemoryReader::new(&bytes);
        let node = Node::deserialize(&mut reader).map_err(MptError::from)?;
        Ok(Some(node))
    }

    fn key(&self, hash: &UInt256) -> Vec<u8> {
        Self::key_for(self.prefix, hash)
    }

    fn key_for(prefix: u8, hash: &UInt256) -> Vec<u8> {
        Self::key_bytes(prefix, hash).to_vec()
    }

    fn key_bytes(prefix: u8, hash: &UInt256) -> [u8; 1 + UINT256_SIZE] {
        let mut buffer = [0u8; 1 + UINT256_SIZE];
        buffer[0] = prefix;
        buffer[1..].copy_from_slice(&hash.to_array());
        buffer
    }
}
