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
use super::root_cache::StateRootCache;
use super::state_root::StateRoot;
use crate::UInt256;
use crate::cryptography::mpt_trie::{MptResult, MptStoreSnapshot, Trie};
use crate::error::{CoreError, CoreResult};
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::persistence::{
    TrackState, data_cache::DataCache, i_store::IStore, i_store_provider::IStoreProvider,
};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use crate::smart_contract::{StorageItem, StorageKey};
use crate::unhandled_exception_policy::UnhandledExceptionPolicy;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;

mod cache_ops;
mod proof_ops;
mod verifier;
pub use verifier::StateRootVerifier;

/// Maximum number of state roots to cache before persistence.
pub const MAX_CACHE_COUNT: usize = 100;

/// Settings for the state service.
#[derive(Debug, Clone)]
pub struct StateServiceSettings {
    /// Whether to maintain full state history.
    pub full_state: bool,
    /// Path to the state store database.
    pub path: String,
    /// Network magic number (used for config validation and path formatting).
    pub network: u32,
    /// Whether to auto-start state root verification when a wallet is available.
    pub auto_verify: bool,
    /// Maximum number of results returned by findstates.
    pub max_find_result_items: usize,
    /// Policy for handling unhandled exceptions.
    pub exception_policy: UnhandledExceptionPolicy,
}

impl Default for StateServiceSettings {
    fn default() -> Self {
        Self {
            full_state: false,
            path: "Data_MPT_{0}".to_string(),
            network: 0,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: UnhandledExceptionPolicy::StopPlugin,
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
    _settings: StateServiceSettings,
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

/// Result of state root verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateRootVerificationResult {
    /// Verification succeeded.
    Valid,
    /// State root mismatch - computed root differs from expected.
    RootMismatch,
    /// State root not found.
    NotFound,
    /// Missing witness for validated root.
    MissingWitness,
    /// Witness verification failed.
    InvalidWitness,
    /// State root index mismatch.
    IndexMismatch,
    /// Verifier not configured.
    VerifierNotConfigured,
}

impl std::fmt::Display for StateRootVerificationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid => write!(f, "valid"),
            Self::RootMismatch => write!(f, "root hash mismatch"),
            Self::NotFound => write!(f, "state root not found"),
            Self::MissingWitness => write!(f, "missing witness"),
            Self::InvalidWitness => write!(f, "invalid witness"),
            Self::IndexMismatch => write!(f, "index mismatch"),
            Self::VerifierNotConfigured => write!(f, "verifier not configured"),
        }
    }
}

/// State store for managing state roots and the state trie.
/// Matches C# StateStore class.
pub struct StateStore {
    store: Arc<dyn StateStoreBackend>,
    settings: StateServiceSettings,
    cache: RwLock<HashMap<u32, StateRoot>>,
    /// LRU cache for recent state roots to avoid disk lookups.
    root_cache: RwLock<StateRootCache>,
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
            root_cache: RwLock::new(StateRootCache::with_default_capacity()),
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

    /// Returns the maximum number of results allowed by findstates.
    pub fn max_find_result_items(&self) -> usize {
        self.settings.max_find_result_items
    }

    /// Returns the configured exception policy.
    pub fn exception_policy(&self) -> UnhandledExceptionPolicy {
        self.settings.exception_policy
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
            tracing::debug!(
                target: "state",
                index = state_root.index,
                "rejecting state root: missing witness"
            );
            metrics::record_ingest_result(false);
            return false;
        }

        // Check if already validated
        if let Some(validated_index) = self.validated_root_index() {
            if state_root.index <= validated_index {
                tracing::debug!(
                    target: "state",
                    index = state_root.index,
                    validated_index,
                    "rejecting state root: index not ahead of validated root"
                );
                metrics::record_ingest_result(false);
                return false;
            }
        }

        let local_index = match self.local_root_index() {
            Some(idx) => idx,
            None => {
                tracing::debug!(
                    target: "state",
                    index = state_root.index,
                    "rejecting state root: missing local root index"
                );
                metrics::record_ingest_result(false);
                return false;
            }
        };

        // Cache future state roots
        if local_index < state_root.index && state_root.index < local_index + MAX_CACHE_COUNT as u32
        {
            tracing::debug!(
                target: "state",
                index = state_root.index,
                local_index,
                "caching future state root"
            );
            self.cache.write().insert(state_root.index, state_root);
            return true;
        }

        // Verify and store
        let snapshot = self.get_snapshot();
        let local_root = match snapshot.get_state_root(state_root.index) {
            None => {
                tracing::debug!(
                    target: "state",
                    index = state_root.index,
                    local_index,
                    "rejecting state root: local root not found"
                );
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
            tracing::debug!(
                target: "state",
                index = state_root.index,
                "rejecting state root: witness verification failed"
            );
            metrics::record_ingest_result(false);
            return false;
        }

        // Already validated
        if local_root.witness.is_some() {
            tracing::debug!(
                target: "state",
                index = state_root.index,
                "rejecting state root: local root already has witness"
            );
            metrics::record_ingest_result(false);
            return false;
        }

        // Root hash must match
        if local_root.root_hash != state_root.root_hash {
            tracing::debug!(
                target: "state",
                index = state_root.index,
                local_root_hash = %local_root.root_hash,
                payload_root_hash = %state_root.root_hash,
                "rejecting state root: root hash mismatch"
            );
            metrics::record_ingest_result(false);
            return false;
        }

        // Store validated root via a transaction for clearer commit semantics.
        let mut tx = self.begin_transaction();
        let mut writer = BinaryWriter::new();
        if state_root.serialize(&mut writer).is_err() {
            tracing::debug!(
                target: "state",
                index = state_root.index,
                "rejecting state root: serialization failed"
            );
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
}

#[cfg(test)]
mod tests;
