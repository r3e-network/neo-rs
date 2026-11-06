use super::{StateServiceSettings, StateStoreBackend, StateStoreSnapshot};
use crate::StateRoot;
use crate::keys::Keys;
use neo_crypto::mpt_trie::{GlobalNodeCache, Node, Trie};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt256;
use std::sync::Arc;

/// Snapshot of the state store for atomic operations.
/// Matches C# StateSnapshot class.
pub struct StateSnapshot {
    store: Arc<dyn StateStoreBackend>,
    /// The Merkle Patricia Trie for state storage.
    pub trie: Trie<StateStoreSnapshot>,
    _settings: StateServiceSettings,
}

impl StateSnapshot {
    /// Creates a new state snapshot.
    pub fn new(store: Arc<dyn StateStoreBackend>, settings: StateServiceSettings) -> Self {
        Self::new_with_node_cache(store, settings, None)
    }

    /// Creates a new state snapshot backed by a store-scoped [`GlobalNodeCache`].
    pub fn new_with_node_cache(
        store: Arc<dyn StateStoreBackend>,
        settings: StateServiceSettings,
        node_cache: Option<Arc<GlobalNodeCache>>,
    ) -> Self {
        Self::new_with_cached_root(store, settings, node_cache, None)
    }

    /// Creates a new state snapshot reusing a pre-resolved root [`Node`] when its hash matches.
    pub fn new_with_cached_root(
        store: Arc<dyn StateStoreBackend>,
        settings: StateServiceSettings,
        node_cache: Option<Arc<GlobalNodeCache>>,
        cached_root: Option<(UInt256, Node)>,
    ) -> Self {
        let current_root = Self::get_current_local_root_hash_internal(&*store);
        let snapshot = StateStoreSnapshot::new(Arc::clone(&store));
        let root_node = match (current_root, cached_root) {
            (Some(curr), Some((cached_hash, cached_node))) if curr == cached_hash => cached_node,
            (Some(curr), _) => Node::new_hash(curr),
            (None, _) => Node::new(),
        };
        let trie = Trie::new_with_root_node(
            Arc::new(snapshot),
            root_node,
            settings.full_state,
            node_cache,
        );

        Self {
            store,
            trie,
            _settings: settings,
        }
    }

    fn get_current_local_root_hash_internal(store: &dyn StateStoreBackend) -> Option<UInt256> {
        let index = store.try_get(Keys::CURRENT_LOCAL_ROOT_INDEX)?;
        if index.len() < 4 {
            return None;
        }
        let index = u32::from_le_bytes([index[0], index[1], index[2], index[3]]);
        let key = Keys::state_root(index);
        let data = store.try_get(&key)?;
        let mut reader = MemoryReader::new(&data);
        let state_root = StateRoot::deserialize(&mut reader).ok()?;
        // A zero root hash must be treated as "no root" so that Trie::new
        // creates an Empty node instead of a HashNode that fails to resolve.
        if state_root.root_hash.is_zero() {
            return None;
        }
        Some(state_root.root_hash)
    }

    /// Gets a state root by index.
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        let key = Keys::state_root(index);
        let data = self.store.try_get(&key)?;
        let mut reader = MemoryReader::new(&data);
        StateRoot::deserialize(&mut reader).ok()
    }

    /// Adds a local state root (without witness).
    pub fn add_local_state_root(&self, state_root: &StateRoot) -> Result<(), String> {
        let key = Keys::state_root(state_root.index);
        let mut writer = BinaryWriter::new();
        state_root
            .serialize(&mut writer)
            .map_err(|e| format!("Serialization failed: {:?}", e))?;
        self.store.put(key, writer.into_bytes());

        // Update current local root index
        self.store.put(
            Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
            state_root.index.to_le_bytes().to_vec(),
        );
        Ok(())
    }

    /// Adds a validated state root (with witness).
    pub fn add_validated_state_root(&self, state_root: &StateRoot) -> Result<(), String> {
        if state_root.witness.is_none() {
            return Err("Missing witness in validated state root".to_string());
        }

        let key = Keys::state_root(state_root.index);
        let mut writer = BinaryWriter::new();
        state_root
            .serialize(&mut writer)
            .map_err(|e| format!("Serialization failed: {:?}", e))?;
        self.store.put(key, writer.into_bytes());

        // Update current validated root index
        self.store.put(
            Keys::CURRENT_VALIDATED_ROOT_INDEX.to_vec(),
            state_root.index.to_le_bytes().to_vec(),
        );
        Ok(())
    }

    /// Gets the current local root index.
    pub fn current_local_root_index(&self) -> Option<u32> {
        let bytes = self.store.try_get(Keys::CURRENT_LOCAL_ROOT_INDEX)?;
        if bytes.len() < 4 {
            return None;
        }
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Gets the current local root hash.
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        let index = self.current_local_root_index()?;
        let state_root = self.get_state_root(index)?;
        Some(state_root.root_hash)
    }

    /// Gets the current validated root index.
    pub fn current_validated_root_index(&self) -> Option<u32> {
        let bytes = self.store.try_get(Keys::CURRENT_VALIDATED_ROOT_INDEX)?;
        if bytes.len() < 4 {
            return None;
        }
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Gets the current validated root hash.
    pub fn current_validated_root_hash(&self) -> Option<UInt256> {
        let index = self.current_validated_root_index()?;
        let state_root = self.get_state_root(index)?;
        state_root.witness.as_ref()?;
        Some(state_root.root_hash)
    }

    /// Stages trie cache changes without committing the backend.
    pub fn stage_commit(&mut self) -> Result<(), String> {
        self.trie
            .commit()
            .map_err(|e| format!("Trie commit failed: {:?}", e))
    }

    /// Discards all pending changes in the backing overlay.
    pub fn discard_pending(&self) {
        self.store.discard_pending();
    }

    /// Commits staged trie updates and root metadata to the backing store.
    pub fn commit_backend(&mut self) -> Result<(), String> {
        self.store.commit()
    }

    /// Commits all pending changes.
    pub fn commit(&mut self) -> Result<(), String> {
        self.stage_commit()?;
        self.commit_backend()
    }
}
