//! State Store Implementation
//!
//! Matches C# Neo.Plugins.StateService.Storage.StateStore exactly.
//! Provides storage and management for state roots and the state Merkle trie.

use super::keys::Keys;
use super::state_root::StateRoot;
use crate::cryptography::mpt_trie::{IStoreSnapshot, MptResult, Trie};
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::persistence::TrackState;
use crate::smart_contract::{StorageItem, StorageKey};
use crate::UInt256;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Maximum number of state roots to cache before persistence.
pub const MAX_CACHE_COUNT: usize = 100;

/// Settings for the state service.
#[derive(Debug, Clone)]
pub struct StateServiceSettings {
    /// Whether to maintain full state history.
    pub full_state: bool,
    /// Path to the state store database.
    pub path: String,
}

impl Default for StateServiceSettings {
    fn default() -> Self {
        Self {
            full_state: false,
            path: "StateRoot".to_string(),
        }
    }
}

/// Adapter to make our store work with the MPT Trie's IStoreSnapshot trait.
pub struct StateStoreSnapshot {
    store: Arc<dyn StateStoreBackend>,
}

impl StateStoreSnapshot {
    pub fn new(store: Arc<dyn StateStoreBackend>) -> Self {
        Self { store }
    }
}

impl IStoreSnapshot for StateStoreSnapshot {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self.store.try_get(key))
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.store.put(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.store.delete(&key);
        Ok(())
    }
}

/// Backend trait for state store persistence.
pub trait StateStoreBackend: Send + Sync {
    /// Try to get a value by key.
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>>;
    /// Put a key-value pair.
    fn put(&self, key: Vec<u8>, value: Vec<u8>);
    /// Delete a key.
    fn delete(&self, key: &[u8]);
    /// Commit changes.
    fn commit(&self);
}

/// In-memory implementation of StateStoreBackend for testing.
#[derive(Default)]
pub struct MemoryStateStoreBackend {
    data: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
    pending: RwLock<HashMap<Vec<u8>, Option<Vec<u8>>>>,
}

impl MemoryStateStoreBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StateStoreBackend for MemoryStateStoreBackend {
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        // Check pending first
        if let Some(pending_value) = self.pending.read().unwrap().get(key) {
            return pending_value.clone();
        }
        // Then check committed data
        self.data.read().unwrap().get(key).cloned()
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.pending.write().unwrap().insert(key, Some(value));
    }

    fn delete(&self, key: &[u8]) {
        self.pending.write().unwrap().insert(key.to_vec(), None);
    }

    fn commit(&self) {
        let mut data = self.data.write().unwrap();
        let mut pending = self.pending.write().unwrap();
        for (key, value) in pending.drain() {
            match value {
                Some(v) => {
                    data.insert(key, v);
                }
                None => {
                    data.remove(&key);
                }
            }
        }
    }
}

/// Snapshot of the state store for atomic operations.
/// Matches C# StateSnapshot class.
pub struct StateSnapshot {
    store: Arc<dyn StateStoreBackend>,
    /// The Merkle Patricia Trie for state storage.
    pub trie: Trie<StateStoreSnapshot>,
    #[allow(dead_code)]
    settings: StateServiceSettings,
}

impl StateSnapshot {
    /// Creates a new state snapshot.
    pub fn new(store: Arc<dyn StateStoreBackend>, settings: StateServiceSettings) -> Self {
        let current_root = Self::get_current_local_root_hash_internal(&*store);
        let snapshot = StateStoreSnapshot::new(Arc::clone(&store));
        let trie = Trie::new(Arc::new(snapshot), current_root, settings.full_state);

        Self {
            store,
            trie,
            settings,
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
    pub fn add_local_state_root(&self, state_root: &StateRoot) {
        let key = Keys::state_root(state_root.index);
        let mut writer = BinaryWriter::new();
        state_root
            .serialize(&mut writer)
            .expect("Serialization should succeed");
        self.store.put(key, writer.into_bytes());

        // Update current local root index
        self.store.put(
            Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
            state_root.index.to_le_bytes().to_vec(),
        );
    }

    /// Adds a validated state root (with witness).
    pub fn add_validated_state_root(&self, state_root: &StateRoot) {
        if state_root.witness.is_none() {
            panic!("Missing witness in validated state root");
        }

        let key = Keys::state_root(state_root.index);
        let mut writer = BinaryWriter::new();
        state_root
            .serialize(&mut writer)
            .expect("Serialization should succeed");
        self.store.put(key, writer.into_bytes());

        // Update current validated root index
        self.store.put(
            Keys::CURRENT_VALIDATED_ROOT_INDEX.to_vec(),
            state_root.index.to_le_bytes().to_vec(),
        );
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
        let index = self.current_local_root_index()?;
        let state_root = self.get_state_root(index)?;
        if state_root.witness.is_none() {
            return None;
        }
        Some(state_root.root_hash)
    }

    /// Commits all pending changes.
    pub fn commit(&mut self) {
        self.trie.commit().expect("Trie commit should succeed");
        self.store.commit();
    }
}

/// State store for managing state roots and the state trie.
/// Matches C# StateStore class.
pub struct StateStore {
    store: Arc<dyn StateStoreBackend>,
    settings: StateServiceSettings,
    cache: RwLock<HashMap<u32, StateRoot>>,
    current_snapshot: RwLock<Option<StateSnapshot>>,
    state_snapshot: RwLock<Option<StateSnapshot>>,
}

impl StateStore {
    /// Creates a new state store.
    pub fn new(store: Arc<dyn StateStoreBackend>, settings: StateServiceSettings) -> Self {
        let snapshot = StateSnapshot::new(Arc::clone(&store), settings.clone());
        Self {
            store,
            settings,
            cache: RwLock::new(HashMap::new()),
            current_snapshot: RwLock::new(Some(snapshot)),
            state_snapshot: RwLock::new(None),
        }
    }

    /// Creates a new in-memory state store for testing.
    pub fn new_in_memory() -> Self {
        let backend = Arc::new(MemoryStateStoreBackend::new());
        Self::new(backend, StateServiceSettings::default())
    }

    /// Gets a new snapshot.
    pub fn get_snapshot(&self) -> StateSnapshot {
        StateSnapshot::new(Arc::clone(&self.store), self.settings.clone())
    }

    /// Gets the current local root hash.
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        self.current_snapshot
            .read()
            .unwrap()
            .as_ref()
            .and_then(|s| s.current_local_root_hash())
    }

    /// Gets the current local root index.
    pub fn local_root_index(&self) -> Option<u32> {
        self.current_snapshot
            .read()
            .unwrap()
            .as_ref()
            .and_then(|s| s.current_local_root_index())
    }

    /// Gets the current validated root index.
    pub fn validated_root_index(&self) -> Option<u32> {
        self.current_snapshot
            .read()
            .unwrap()
            .as_ref()
            .and_then(|s| s.current_validated_root_index())
    }

    /// Processes a new state root from the network.
    pub fn on_new_state_root(&self, state_root: StateRoot) -> bool {
        // Must have witness
        if state_root.witness.is_none() {
            return false;
        }

        // Check if already validated
        if let Some(validated_index) = self.validated_root_index() {
            if state_root.index <= validated_index {
                return false;
            }
        }

        let local_index = match self.local_root_index() {
            Some(idx) => idx,
            None => return false,
        };

        // Cache future state roots
        if local_index < state_root.index && state_root.index < local_index + MAX_CACHE_COUNT as u32
        {
            self.cache
                .write()
                .unwrap()
                .insert(state_root.index, state_root);
            return true;
        }

        // Verify and store
        let snapshot = self.get_snapshot();
        let local_root = match snapshot.get_state_root(state_root.index) {
            Some(r) => r,
            None => return false,
        };

        // Already validated
        if local_root.witness.is_some() {
            return false;
        }

        // Root hash must match
        if local_root.root_hash != state_root.root_hash {
            return false;
        }

        // TODO: Verify witness against protocol settings
        // For now, trust the witness

        // Store validated root
        let mut snapshot = self.get_snapshot();
        snapshot.add_validated_state_root(&state_root);
        snapshot.commit();
        self.update_current_snapshot();

        true
    }

    /// Updates the local state root snapshot with a change set.
    /// Called during block commit to update the state trie.
    pub fn update_local_state_root_snapshot(
        &self,
        height: u32,
        change_set: impl Iterator<Item = (StorageKey, StorageItem, TrackState)>,
    ) {
        // Dispose old snapshot
        {
            let mut state_snap = self.state_snapshot.write().unwrap();
            *state_snap = Some(self.get_snapshot());
        }

        let mut state_snap = self.state_snapshot.write().unwrap();
        if let Some(ref mut snapshot) = *state_snap {
            // Apply changes to trie
            for (key, item, state) in change_set {
                let key_bytes = key.to_array();
                match state {
                    TrackState::Added | TrackState::Changed => {
                        let value_bytes = item.get_value();
                        let _ = snapshot.trie.put(&key_bytes, &value_bytes);
                    }
                    TrackState::Deleted => {
                        let _ = snapshot.trie.delete(&key_bytes);
                    }
                    TrackState::None | TrackState::NotFound => {}
                }
            }

            // Get new root hash
            let root_hash = snapshot.trie.root_hash().unwrap_or_else(UInt256::zero);

            // Create and store state root
            let state_root = StateRoot::new_current(height, root_hash);
            snapshot.add_local_state_root(&state_root);
        }
    }

    /// Commits the local state root after block persistence.
    pub fn update_local_state_root(&self, height: u32) {
        // Commit and dispose snapshot
        {
            let mut state_snap = self.state_snapshot.write().unwrap();
            if let Some(ref mut snapshot) = *state_snap {
                snapshot.commit();
            }
            *state_snap = None;
        }

        self.update_current_snapshot();
        self.check_validated_state_root(height);
    }

    /// Checks if we have a cached validated state root for this height.
    fn check_validated_state_root(&self, index: u32) {
        let state_root = { self.cache.write().unwrap().remove(&index) };

        if let Some(root) = state_root {
            self.on_new_state_root(root);
        }
    }

    /// Updates the current snapshot reference.
    fn update_current_snapshot(&self) {
        let new_snapshot = self.get_snapshot();
        let mut current = self.current_snapshot.write().unwrap();
        *current = Some(new_snapshot);
    }

    /// Gets a state root by index.
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        self.get_snapshot().get_state_root(index)
    }

    /// Gets a proof for a storage key at a given root.
    pub fn get_proof(&self, root: UInt256, key: &StorageKey) -> Option<Vec<Vec<u8>>> {
        let snapshot = StateStoreSnapshot::new(Arc::clone(&self.store));
        let mut trie = Trie::new(Arc::new(snapshot), Some(root), self.settings.full_state);
        let key_bytes = key.to_array();
        trie.try_get_proof(&key_bytes)
            .ok()
            .flatten()
            .map(|set| set.into_iter().collect())
    }

    /// Verifies a proof.
    pub fn verify_proof(root: UInt256, key: &[u8], proof: &[Vec<u8>]) -> Option<Vec<u8>> {
        let proof_set: std::collections::HashSet<Vec<u8>> = proof.iter().cloned().collect();
        Trie::<StateStoreSnapshot>::verify_proof(root, key, &proof_set).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_store_creation() {
        let store = StateStore::new_in_memory();
        assert!(store.local_root_index().is_none());
        assert!(store.validated_root_index().is_none());
    }

    #[test]
    fn test_state_root_storage() {
        let store = StateStore::new_in_memory();
        let mut snapshot = store.get_snapshot();

        let root_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let state_root = StateRoot::new_current(100, root_hash);

        snapshot.add_local_state_root(&state_root);
        snapshot.commit();

        let retrieved = store.get_state_root(100);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.index, 100);
        assert_eq!(retrieved.root_hash, root_hash);
    }

    #[test]
    fn test_state_snapshot_operations() {
        let backend = Arc::new(MemoryStateStoreBackend::new());
        let settings = StateServiceSettings::default();
        let mut snapshot = StateSnapshot::new(backend, settings);

        // Put some data in the trie
        snapshot.trie.put(&[1, 2, 3], &[4, 5, 6]).unwrap();
        snapshot.trie.put(&[1, 2, 4], &[7, 8, 9]).unwrap();

        // Get the data back
        let value = snapshot.trie.get(&[1, 2, 3]).unwrap();
        assert_eq!(value, Some(vec![4, 5, 6]));

        let value = snapshot.trie.get(&[1, 2, 4]).unwrap();
        assert_eq!(value, Some(vec![7, 8, 9]));

        // Commit
        snapshot.commit();
    }

    #[test]
    fn test_memory_backend() {
        let backend = MemoryStateStoreBackend::new();

        // Put and get
        backend.put(vec![1, 2, 3], vec![4, 5, 6]);
        assert_eq!(backend.try_get(&[1, 2, 3]), Some(vec![4, 5, 6]));

        // Commit
        backend.commit();
        assert_eq!(backend.try_get(&[1, 2, 3]), Some(vec![4, 5, 6]));

        // Delete
        backend.delete(&[1, 2, 3]);
        assert_eq!(backend.try_get(&[1, 2, 3]), None);

        // Commit
        backend.commit();
        assert_eq!(backend.try_get(&[1, 2, 3]), None);
    }
}
