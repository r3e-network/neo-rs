//! State Store Implementation
//!
//! Matches C# Neo.Plugins.StateService.Storage.StateStore exactly.
//! Provides storage and management for state roots and the state Merkle trie.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      StateStore                              │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                   State Roots                            ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  ││
//! │  │  │ Local    │  │ Validated│  │ Pending              │  ││
//! │  │  │ Roots    │  │ Roots    │  │ (unvalidated)        │  ││
//! │  │  └──────────┘  └──────────┘  └──────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                   Merkle Patricia Trie                   ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  ││
//! │  │  │ Root Hash│  │ Proof    │  │ State Snapshots      │  ││
//! │  │  │ Tracking │  │ Generation│ │ (per block)          │  ││
//! │  │  └──────────┘  └──────────┘  └──────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │ Backend      │  │ Verifier     │  │ Metrics          │  │
//! │  │ (storage)    │  │ (signatures) │  │ (stats)          │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Components
//!
//! - [`StateStore`]: Main state storage with local/validated root tracking
//! - [`StateSnapshot`]: Point-in-time view for atomic state updates
//! - [`StateStoreBackend`]: Pluggable storage backend trait
//! - [`StateRootVerifier`]: Signature verification for state roots
//!
//! # State Root Types
//!
//! - **Local Root**: Computed locally from block execution
//! - **Validated Root**: Received from network with consensus signatures
//! - **Pending Root**: Awaiting validation signatures
//!
//! # Proof Generation
//!
//! The store supports Merkle proof generation for state verification:
//! 1. `get_proof()`: Generate inclusion proof for a storage key
//! 2. `verify_proof()`: Verify proof against a known root hash
//! 3. `encode_proof_payload()`: Serialize proof for network transmission

use super::keys::Keys;
use super::metrics;
use super::state_root::StateRoot;
use crate::cryptography::mpt_trie::{MptResult, MptStoreSnapshot, Trie};
use crate::error::CoreResult;
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::persistence::{
    data_cache::DataCache, i_store::IStore, i_store_provider::IStoreProvider,
    store_cache::StoreCache, TrackState,
};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use crate::smart_contract::{StorageItem, StorageKey};
use crate::UInt256;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;

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

/// Adapter to make our store work with the MPT Trie's `MptStoreSnapshot` trait.
pub struct StateStoreSnapshot {
    store: Arc<dyn StateStoreBackend>,
}

impl StateStoreSnapshot {
    pub fn new(store: Arc<dyn StateStoreBackend>) -> Self {
        Self { store }
    }
}

impl MptStoreSnapshot for StateStoreSnapshot {
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

/// Minimal transactional wrapper over a `StateStoreBackend`.
///
/// This helper batches writes and applies them atomically via the backend, keeping
/// commit semantics explicit for callers that need a scoped write.
pub struct StateStoreTransaction {
    backend: Arc<dyn StateStoreBackend>,
    writes: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

impl StateStoreTransaction {
    /// Creates a transaction bound to the provided backend.
    pub fn new(backend: Arc<dyn StateStoreBackend>) -> Self {
        Self {
            backend,
            writes: Vec::new(),
        }
    }

    /// Enqueue a put operation.
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.writes.push((key, Some(value)));
    }

    /// Enqueue a delete operation.
    pub fn delete(&mut self, key: &[u8]) {
        self.writes.push((key.to_vec(), None));
    }

    /// Applies all enqueued operations and commits the backend.
    pub fn commit(mut self) {
        for (key, value) in self.writes.drain(..) {
            match value {
                Some(v) => self.backend.put(key, v),
                None => self.backend.delete(&key),
            }
        }
        self.backend.commit();
    }
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
        if let Some(pending_value) = self.pending.read().get(key).cloned() {
            return pending_value;
        }
        // Then check committed data
        self.data.read().get(key).cloned()
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.pending.write().insert(key, Some(value));
    }

    fn delete(&self, key: &[u8]) {
        self.pending.write().insert(key.to_vec(), None);
    }

    fn commit(&self) {
        let mut data = self.data.write();
        let mut pending = self.pending.write();
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

/// Snapshot-backed backend that persists through the core `IStore`.
pub struct SnapshotBackedStateStoreBackend {
    store: Arc<dyn IStore>,
    pending: Mutex<HashMap<Vec<u8>, Option<Vec<u8>>>>,
}

impl SnapshotBackedStateStoreBackend {
    pub fn new(store: Arc<dyn IStore>) -> Self {
        Self {
            store,
            pending: Mutex::new(HashMap::new()),
        }
    }
}

impl StateStoreBackend for SnapshotBackedStateStoreBackend {
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        // Pending writes take precedence
        if let Some(value) = self.pending.lock().get(key).cloned() {
            return value;
        }

        let snapshot = self.store.get_snapshot();
        snapshot.try_get(&key.to_vec())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.pending.lock().insert(key, Some(value));
    }

    fn delete(&self, key: &[u8]) {
        self.pending.lock().insert(key.to_vec(), None);
    }

    fn commit(&self) {
        let mut pending = self.pending.lock();
        if pending.is_empty() {
            return;
        }

        let mut snapshot = self.store.get_snapshot();
        let Some(snapshot_mut) = Arc::get_mut(&mut snapshot) else {
            tracing::warn!(
                target: "neo",
                "state service commit aborted: snapshot has additional references"
            );
            return;
        };

        for (key, value) in pending.drain() {
            match value {
                Some(v) => snapshot_mut.put(key, v),
                None => snapshot_mut.delete(key),
            }
        }
        snapshot_mut.commit();
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

    /// Commits all pending changes.
    pub fn commit(&mut self) -> Result<(), String> {
        self.trie
            .commit()
            .map_err(|e| format!("Trie commit failed: {:?}", e))?;

        // Flush pending trie updates and any staged root/index updates.
        self.store.commit();
        Ok(())
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
    verifier: Option<StateRootVerifier>,
}

impl StateStore {
    /// Creates a new state store.
    pub fn new(store: Arc<dyn StateStoreBackend>, settings: StateServiceSettings) -> Self {
        Self::new_with_verifier(store, settings, None)
    }

    /// Creates a new state store with an optional verifier.
    pub fn new_with_verifier(
        store: Arc<dyn StateStoreBackend>,
        settings: StateServiceSettings,
        verifier: Option<StateRootVerifier>,
    ) -> Self {
        let snapshot = StateSnapshot::new(Arc::clone(&store), settings.clone());
        Self {
            store,
            settings,
            cache: RwLock::new(HashMap::new()),
            current_snapshot: RwLock::new(Some(snapshot)),
            state_snapshot: RwLock::new(None),
            verifier,
        }
    }

    /// Creates a new in-memory state store for testing.
    pub fn new_in_memory() -> Self {
        let backend = Arc::new(MemoryStateStoreBackend::new());
        Self::new(backend, StateServiceSettings::default())
    }

    /// Returns whether the state store keeps full historical state.
    ///
    /// When `false`, the store prunes historical trie nodes and only the current local root
    /// can be queried for proofs/state (mirrors the C# StateService `FullState` setting).
    pub fn full_state(&self) -> bool {
        self.settings.full_state
    }

    /// Creates a state store backed by the provided blockchain store and protocol settings,
    /// wiring a verifier that reads designated validators from the same store.
    pub fn new_from_store(
        store: Arc<dyn IStore>,
        settings: StateServiceSettings,
        protocol_settings: Arc<ProtocolSettings>,
    ) -> Self {
        let backend = Arc::new(SnapshotBackedStateStoreBackend::new(store.clone()));
        let verifier = StateRootVerifier::from_store(store, protocol_settings);
        Self::new_with_verifier(backend, settings, Some(verifier))
    }

    /// Opens a state store using the provided store provider and path, wiring a verifier that
    /// reads validator designations from the same store. This mirrors the C# StateService
    /// behaviour where the plugin uses the node's database for both state and validator lookups.
    pub fn open_with_provider(
        provider: Arc<dyn IStoreProvider>,
        path: &str,
        settings: StateServiceSettings,
        protocol_settings: Arc<ProtocolSettings>,
    ) -> CoreResult<Self> {
        let store = provider.get_store(path)?;
        Ok(Self::new_from_store(store, settings, protocol_settings))
    }

    /// Gets a new snapshot.
    pub fn get_snapshot(&self) -> StateSnapshot {
        StateSnapshot::new(Arc::clone(&self.store), self.settings.clone())
    }

    /// Opens a transactional view over the underlying state store backend.
    pub fn begin_transaction(&self) -> StateStoreTransaction {
        StateStoreTransaction::new(self.store.clone())
    }

    /// Gets the current local root hash.
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        self.current_snapshot
            .read()
            .as_ref()
            .and_then(|s| s.current_local_root_hash())
    }

    /// Gets the current local root index.
    pub fn local_root_index(&self) -> Option<u32> {
        self.current_snapshot
            .read()
            .as_ref()
            .and_then(|s| s.current_local_root_index())
    }

    /// Gets the current validated root index.
    pub fn validated_root_index(&self) -> Option<u32> {
        self.current_snapshot
            .read()
            .as_ref()
            .and_then(|s| s.current_validated_root_index())
    }

    /// Gets the current validated root hash.
    pub fn current_validated_root_hash(&self) -> Option<UInt256> {
        self.current_snapshot
            .read()
            .as_ref()
            .and_then(|s| s.current_validated_root_hash())
    }

    /// Processes a new state root from the network.
    pub fn on_new_state_root(&self, state_root: StateRoot) -> bool {
        // Must have witness
        if state_root.witness.is_none() {
            metrics::record_ingest_result(false);
            return false;
        }

        // Check if already validated
        if let Some(validated_index) = self.validated_root_index() {
            if state_root.index <= validated_index {
                metrics::record_ingest_result(false);
                return false;
            }
        }

        let local_index = match self.local_root_index() {
            Some(idx) => idx,
            None => {
                metrics::record_ingest_result(false);
                return false;
            }
        };

        // Cache future state roots
        if local_index < state_root.index && state_root.index < local_index + MAX_CACHE_COUNT as u32
        {
            self.cache.write().insert(state_root.index, state_root);
            return true;
        }

        // Verify and store
        let snapshot = self.get_snapshot();
        let local_root = match snapshot.get_state_root(state_root.index) {
            None => {
                metrics::record_ingest_result(false);
                return false;
            }
            Some(r) => r,
        };

        // Validate witness using the configured verifier (if available).
        let Some(verifier) = &self.verifier else {
            tracing::warn!(
                target: "neo",
                index = state_root.index,
                "state root received without verifier configured; rejecting"
            );
            metrics::record_ingest_result(false);
            return false;
        };
        if !verifier.verify(&state_root) {
            metrics::record_ingest_result(false);
            return false;
        }

        // Already validated
        if local_root.witness.is_some() {
            metrics::record_ingest_result(false);
            return false;
        }

        // Root hash must match
        if local_root.root_hash != state_root.root_hash {
            metrics::record_ingest_result(false);
            return false;
        }

        // Store validated root via a transaction for clearer commit semantics.
        let mut tx = self.begin_transaction();
        let mut writer = BinaryWriter::new();
        if state_root.serialize(&mut writer).is_err() {
            metrics::record_ingest_result(false);
            return false;
        }
        tx.put(Keys::state_root(state_root.index), writer.into_bytes());
        tx.put(
            Keys::CURRENT_VALIDATED_ROOT_INDEX.to_vec(),
            state_root.index.to_le_bytes().to_vec(),
        );
        tx.commit();
        self.update_current_snapshot();

        metrics::record_ingest_result(true);
        true
    }

    /// Updates the local state root snapshot with a change set.
    /// Called during block commit to update the state trie.
    pub fn update_local_state_root_snapshot(
        &self,
        height: u32,
        change_set: impl Iterator<Item = (StorageKey, StorageItem, TrackState)>,
    ) {
        let mut state_snap = self.state_snapshot.write();
        *state_snap = Some(self.get_snapshot());
        if let Some(ref mut snapshot) = *state_snap {
            // Apply changes to trie
            for (key, item, state) in change_set {
                // Match Neo.Plugins.StateService behaviour: exclude ledger contract storage
                // from trie updates to keep state root consensus-compatible.
                if key.id == LedgerContract::ID {
                    continue;
                }
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
            let _ = snapshot.add_local_state_root(&state_root);
        }
    }

    /// Commits the local state root after block persistence.
    pub fn update_local_state_root(&self, height: u32) {
        // Commit and dispose snapshot
        {
            let mut state_snap = self.state_snapshot.write();
            if let Some(ref mut snapshot) = *state_snap {
                let _ = snapshot.commit();
            }
            *state_snap = None;
        }

        self.update_current_snapshot();
        self.check_validated_state_root(height);
    }

    /// Checks if we have a cached validated state root for this height.
    fn check_validated_state_root(&self, index: u32) {
        let state_root = self.cache.write().remove(&index);

        if let Some(root) = state_root {
            self.on_new_state_root(root);
        }
    }

    /// Updates the current snapshot reference.
    fn update_current_snapshot(&self) {
        let new_snapshot = self.get_snapshot();
        *self.current_snapshot.write() = Some(new_snapshot);
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

    /// Builds a trie anchored at the supplied root hash for querying state.
    pub fn trie_for_root(&self, root: UInt256) -> Trie<StateStoreSnapshot> {
        let snapshot = StateStoreSnapshot::new(Arc::clone(&self.store));
        Trie::new(Arc::new(snapshot), Some(root), self.settings.full_state)
    }

    /// Verifies a proof.
    pub fn verify_proof(root: UInt256, key: &[u8], proof: &[Vec<u8>]) -> Option<Vec<u8>> {
        let proof_set: std::collections::HashSet<Vec<u8>> = proof.iter().cloned().collect();
        Trie::<StateStoreSnapshot>::verify_proof(root, key, &proof_set).ok()
    }

    /// Serializes a proof payload (key + nodes) for transport over RPC.
    pub fn encode_proof_payload(key: &[u8], nodes: &[Vec<u8>]) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if let Err(err) = writer.write_var_bytes(key) {
            tracing::error!("failed to serialize proof key: {err}");
            return Vec::new();
        }
        if let Err(err) = writer.write_var_int(nodes.len() as u64) {
            tracing::error!("failed to serialize proof length: {err}");
            return Vec::new();
        }
        for node in nodes {
            if let Err(err) = writer.write_var_bytes(node) {
                tracing::error!("failed to serialize proof node: {err}");
                return Vec::new();
            }
        }
        writer.into_bytes()
    }

    /// Deserializes a proof payload produced by `encode_proof_payload`.
    pub fn decode_proof_payload(bytes: &[u8]) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
        let mut reader = MemoryReader::new(bytes);
        let key = reader.read_var_bytes(usize::MAX).ok()?;
        let count = reader.read_var_int(u64::MAX).ok()? as usize;
        let mut nodes = Vec::with_capacity(count);
        for _ in 0..count {
            nodes.push(reader.read_var_bytes(usize::MAX).ok()?);
        }
        Some((key, nodes))
    }
}

/// Verifies state roots using the designated validator set.
#[derive(Clone)]
pub struct StateRootVerifier {
    settings: Arc<ProtocolSettings>,
    snapshot_provider: Arc<dyn Fn() -> DataCache + Send + Sync>,
}

impl StateRootVerifier {
    pub fn new(
        settings: Arc<ProtocolSettings>,
        snapshot_provider: Arc<dyn Fn() -> DataCache + Send + Sync>,
    ) -> Self {
        Self {
            settings,
            snapshot_provider,
        }
    }

    fn verify(&self, state_root: &StateRoot) -> bool {
        let snapshot = (self.snapshot_provider)();
        state_root.verify(&self.settings, &snapshot)
    }

    /// Builds a verifier that reads state from the provided store using a read-only cache.
    pub fn from_store(store: Arc<dyn IStore>, settings: Arc<ProtocolSettings>) -> Self {
        Self::new(
            settings,
            Arc::new(move || {
                // Fresh read-only view for each verification to avoid mutability concerns.
                let cache = StoreCache::new_from_store(store.clone(), true);
                cache.data_cache().clone_cache()
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::Witness;
    use crate::persistence::providers::memory_store_provider::MemoryStoreProvider;
    use crate::persistence::TrackState;
    use crate::protocol_settings::ProtocolSettings;
    use crate::smart_contract::native::LedgerContract;
    use crate::smart_contract::native::{role_management::RoleManagement, NativeContract, Role};
    use crate::smart_contract::Contract;
    use crate::wallets::KeyPair;
    use neo_vm::op_code::OpCode;
    use std::sync::Arc;

    fn cache_with_designated_state_validators(
        index: u32,
        validators: &[crate::ECPoint],
    ) -> DataCache {
        let cache = DataCache::new(false);
        let mut suffix = vec![Role::StateValidator as u8];
        suffix.extend_from_slice(&index.to_be_bytes());
        let key = StorageKey::new(RoleManagement::new().id(), suffix);

        let mut value = Vec::with_capacity(4 + 33 * validators.len());
        value.extend_from_slice(&(validators.len() as u32).to_le_bytes());
        for validator in validators {
            let encoded = validator
                .encode_compressed()
                .expect("validator key must encode");
            value.extend_from_slice(&encoded);
        }

        cache.add(key, StorageItem::from_bytes(value));
        cache
    }

    #[test]
    fn test_state_store_creation() {
        let store = StateStore::new(
            Arc::new(MemoryStateStoreBackend::new()),
            StateServiceSettings {
                full_state: true,
                ..StateServiceSettings::default()
            },
        );
        assert!(store.local_root_index().is_none());
        assert!(store.validated_root_index().is_none());
    }

    #[test]
    fn test_state_root_storage() {
        let store = StateStore::new_in_memory();
        let mut snapshot = store.get_snapshot();

        let root_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let state_root = StateRoot::new_current(100, root_hash);

        snapshot.add_local_state_root(&state_root).unwrap();
        snapshot.commit().unwrap();

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
        let _ = snapshot.commit();
    }

    #[test]
    fn ledger_storage_is_excluded_from_state_trie() {
        let store = StateStore::new_in_memory();
        let height = 0;

        let ledger_key = StorageKey::new(LedgerContract::ID, vec![0x01]);
        let ledger_value = vec![0xAA, 0xBB, 0xCC];
        let other_key = StorageKey::new(123, vec![0x02]);
        let other_value = vec![0x10, 0x11];

        let changes = vec![
            (
                ledger_key.clone(),
                StorageItem::from_bytes(ledger_value),
                TrackState::Added,
            ),
            (
                other_key.clone(),
                StorageItem::from_bytes(other_value.clone()),
                TrackState::Added,
            ),
        ];

        store.update_local_state_root_snapshot(height, changes.into_iter());
        store.update_local_state_root(height);

        let root = store
            .get_state_root(height)
            .expect("state root should be stored");
        let mut trie = store.trie_for_root(root.root_hash);
        assert_eq!(
            trie.get(&other_key.to_array()).expect("trie get"),
            Some(other_value)
        );
        assert_eq!(trie.get(&ledger_key.to_array()).expect("trie get"), None);
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

    #[test]
    fn validated_root_hash_prefers_validated_index() {
        let store = StateStore::new_in_memory();

        // Seed a local root at height 1
        let mut snapshot = store.get_snapshot();
        let local_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let local_root = StateRoot::new_current(1, local_hash);
        snapshot.add_local_state_root(&local_root).unwrap();
        snapshot.commit().unwrap();

        // Persist a validated root at a different height to ensure we read from CURRENT_VALIDATED_ROOT_INDEX
        let mut validated_root =
            StateRoot::new_current(2, UInt256::from_bytes(&[2u8; 32]).unwrap());
        validated_root.witness = Some(Witness::new_with_scripts(vec![0x01], vec![0x02]));
        let mut validated_snapshot = store.get_snapshot();
        validated_snapshot
            .add_validated_state_root(&validated_root)
            .unwrap();
        validated_snapshot.commit().unwrap();

        assert_eq!(
            store.current_validated_root_hash(),
            Some(validated_root.root_hash)
        );
    }

    #[test]
    fn rejects_state_root_without_verifier() {
        let store = StateStore::new_in_memory();

        // Seed a local root at height 10
        let mut snapshot = store.get_snapshot();
        let root_hash = UInt256::from_bytes(&[3u8; 32]).unwrap();
        let local_root = StateRoot::new_current(10, root_hash);
        snapshot.add_local_state_root(&local_root).unwrap();
        snapshot.commit().unwrap();

        // Build a dummy witness to exercise the verifier path
        let witness = Witness::new_with_scripts(vec![0x01], vec![0x02]);
        let mut incoming = StateRoot::new_current(10, root_hash);
        incoming.witness = Some(witness);

        assert!(!store.on_new_state_root(incoming));
        assert!(store.validated_root_index().is_none());
    }

    #[test]
    fn state_store_transaction_applies_pending_writes() {
        let backend = Arc::new(MemoryStateStoreBackend::new());
        let mut tx = StateStoreTransaction::new(backend.clone());

        let key = b"tx-key".to_vec();
        let value = b"tx-value".to_vec();
        tx.put(key.clone(), value.clone());
        tx.delete(b"to-delete");

        tx.commit();

        assert_eq!(backend.try_get(&key), Some(value));
        assert!(backend.try_get(b"to-delete").is_none());
    }

    #[test]
    fn rejects_state_root_with_invalid_signature() {
        let settings = ProtocolSettings::default_settings();
        let keypair = KeyPair::generate().expect("generate keypair");
        let validator = keypair.get_public_key_point().expect("public key point");
        let validators = vec![validator.clone()];

        let verifier = StateRootVerifier::new(
            Arc::new(settings.clone()),
            Arc::new(move || cache_with_designated_state_validators(7, &validators)),
        );
        let backend = Arc::new(MemoryStateStoreBackend::new());
        let store =
            StateStore::new_with_verifier(backend, StateServiceSettings::default(), Some(verifier));

        // Seed local root
        let mut local_snapshot = store.get_snapshot();
        let root_hash = UInt256::from_bytes(&[8u8; 32]).unwrap();
        let local_root = StateRoot::new_current(7, root_hash);
        local_snapshot
            .add_local_state_root(&local_root)
            .expect("local state root");
        local_snapshot.commit().expect("commit local root");

        // Build signed state root but use an incorrect verification script (single-sig)
        let mut signed_root = StateRoot::new_current(7, root_hash);
        let hash = signed_root.hash();
        let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
        sign_data.extend_from_slice(&settings.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.to_array());
        let signature = keypair.sign(&sign_data).expect("sign state root");

        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);

        // Use a single-sig script instead of multi-sig to force failure
        let verification_script = Contract::create_signature_contract(validator).script;
        signed_root.witness = Some(Witness::new_with_scripts(invocation, verification_script));

        assert!(!store.on_new_state_root(signed_root));
        assert!(store.validated_root_index().is_none());
    }

    #[test]
    fn open_with_provider_uses_snapshot_backend() {
        let provider = Arc::new(MemoryStoreProvider::new());
        let protocol_settings = Arc::new(ProtocolSettings::default_settings());
        let store = StateStore::open_with_provider(
            provider,
            "StateRoot",
            StateServiceSettings::default(),
            protocol_settings,
        )
        .expect("state store opens");

        let mut snapshot = store.get_snapshot();
        let root_hash = UInt256::from_bytes(&[4u8; 32]).unwrap();
        let state_root = StateRoot::new_current(1, root_hash);
        snapshot.add_local_state_root(&state_root).unwrap();
        snapshot.commit().unwrap();

        assert_eq!(store.current_local_root_hash(), Some(root_hash));
    }

    #[test]
    fn produces_and_verifies_storage_proof() {
        let store = StateStore::new(
            Arc::new(MemoryStateStoreBackend::new()),
            StateServiceSettings {
                full_state: true,
                ..StateServiceSettings::default()
            },
        );
        let key = StorageKey::create(1, 0x01);
        let mut item = StorageItem::default();
        item.set_value(vec![0xAA, 0xBB]);

        // Build a snapshot manually to keep the test focused on proof behaviour.
        let mut snapshot = store.get_snapshot();
        snapshot
            .trie
            .put(&key.to_array(), &item.get_value())
            .expect("put value in trie");
        let proof = snapshot
            .trie
            .try_get_proof(&key.to_array())
            .expect("proof lookup")
            .expect("proof present")
            .into_iter()
            .collect::<Vec<_>>();
        let root_hash = snapshot.trie.root_hash().expect("root hash");
        let value =
            StateStore::verify_proof(root_hash, &key.to_array(), &proof).expect("proof verifies");
        assert_eq!(value, item.get_value());
    }

    #[test]
    fn verifies_state_root_witness_against_designated_state_validators() {
        let settings = ProtocolSettings::default_settings();
        let keypair = KeyPair::generate().expect("generate keypair");
        let validator = keypair.get_public_key_point().expect("public key point");
        let validators = vec![validator.clone()];

        let verifier = StateRootVerifier::new(
            Arc::new(settings.clone()),
            Arc::new(move || cache_with_designated_state_validators(5, &validators)),
        );
        let backend = Arc::new(MemoryStateStoreBackend::new());
        let store =
            StateStore::new_with_verifier(backend, StateServiceSettings::default(), Some(verifier));

        // Seed local root without witness
        let mut local_snapshot = store.get_snapshot();
        let root_hash = UInt256::from_bytes(&[9u8; 32]).unwrap();
        let local_root = StateRoot::new_current(5, root_hash);
        local_snapshot
            .add_local_state_root(&local_root)
            .expect("local state root");
        local_snapshot.commit().expect("commit local root");

        // Build signed state root
        let mut signed_root = StateRoot::new_current(5, root_hash);
        let hash = signed_root.hash();
        let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
        sign_data.extend_from_slice(&settings.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.to_array());
        let signature = keypair.sign(&sign_data).expect("sign state root");

        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);

        let verification_script = Contract::create_multi_sig_redeem_script(1, &[validator]);
        signed_root.witness = Some(Witness::new_with_scripts(invocation, verification_script));

        assert!(store.on_new_state_root(signed_root));
        assert_eq!(store.validated_root_index(), Some(5));
    }
}
