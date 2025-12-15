use super::error::{MptError, MptResult};
use super::node::Node;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt256;
use neo_primitives::UINT256_SIZE;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackState {
    None,
    Added,
    Changed,
    Deleted,
}

fn to_array<T: Serializable>(value: &T) -> neo_io::IoResult<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    value.serialize(&mut writer)?;
    Ok(writer.into_bytes())
}

/// Abstraction over the persistence snapshot used by the trie cache.
pub trait MptStoreSnapshot: Send + Sync {
    /// Retrieves the serialized node associated with the specified key.
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>>;

    /// Persists the serialized node for the supplied key.
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()>;

    /// Removes the value associated with the supplied key.
    fn delete(&self, key: Vec<u8>) -> MptResult<()>;
}

struct MptTrackable {
    node: Option<Node>,
    state: TrackState,
}

impl MptTrackable {
    fn new(node: Option<Node>) -> Self {
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
        let entry = self.resolve_internal(&hash)?;

        match entry.node {
            Some(ref mut existing) => {
                existing.reference = existing.reference.saturating_add(1);
                entry.state = TrackState::Changed;
            }
            None => {
                let mut stored = node.clone();
                stored.reference = 1;
                entry.node = Some(stored);
                entry.state = TrackState::Added;
            }
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
        for (hash, entry) in self.entries.iter() {
            match entry.state {
                TrackState::None => {}
                TrackState::Added | TrackState::Changed => {
                    let node = entry
                        .node
                        .as_ref()
                        .ok_or_else(|| MptError::invalid("cache entry missing node"))?;
                    let data = to_array(node).map_err(MptError::from)?;
                    self.store.put(self.key(hash), data)?;
                }
                TrackState::Deleted => {
                    self.store.delete(self.key(hash))?;
                }
            }
        }
        self.entries.clear();
        Ok(())
    }

    fn resolve_internal(&mut self, hash: &UInt256) -> MptResult<&mut MptTrackable> {
        if !self.entries.contains_key(hash) {
            let node = self.load_from_store(hash)?;
            self.entries.insert(*hash, MptTrackable::new(node));
        }
        self.entries
            .get_mut(hash)
            .ok_or_else(|| MptError::invalid("entry missing after insertion"))
    }

    fn load_from_store(&self, hash: &UInt256) -> MptResult<Option<Node>> {
        let key = self.key(hash);
        let Some(bytes) = self.store.try_get(&key)? else {
            return Ok(None);
        };
        let mut reader = MemoryReader::new(&bytes);
        let node = Node::deserialize(&mut reader).map_err(MptError::from)?;
        Ok(Some(node))
    }

    fn key(&self, hash: &UInt256) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(1 + UINT256_SIZE);
        buffer.push(self.prefix);
        buffer.extend_from_slice(&hash.to_bytes());
        buffer
    }
}
