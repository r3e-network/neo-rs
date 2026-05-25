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
use crate::error::CoreResult;
use crate::neo_io::{BinaryWriter, Serializable};
use crate::persistence::{
    seek_direction::SeekDirection, store::IStore, store_provider::StoreProvider, TrackState,
};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use crate::smart_contract::{StorageItem, StorageKey};
use crate::unhandled_exception_policy::UnhandledExceptionPolicy;
use crate::UInt256;
use parking_lot::RwLock;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

mod backend;
mod cache_ops;
mod proof;
mod settings;
mod snapshot;
mod verification_ops;
mod verification_result;
mod verifier;
pub use backend::{
    MemoryStateStoreBackend, SnapshotBackedStateStoreBackend, StateStoreBackend,
    StateStoreSnapshot, StateStoreTransaction,
};
pub use settings::StateServiceSettings;
pub use snapshot::StateSnapshot;
pub use verification_result::StateRootVerificationResult;
pub use verifier::StateRootVerifier;

#[derive(Deserialize)]
struct ReferenceRootLine {
    height: u32,
    roothash: String,
}

/// Maximum number of state roots to cache before persistence.
pub const MAX_CACHE_COUNT: usize = 100;

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
    /// Reference state roots for validation (loaded from JSONL file).
    reference_roots: HashMap<u32, UInt256>,
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
            reference_roots: HashMap::new(),
        }
    }

    /// Creates a new in-memory state store for testing.
    pub fn new_in_memory() -> Self {
        let backend = Arc::new(MemoryStateStoreBackend::new());
        Self::new(backend, StateServiceSettings::default())
    }

    /// Loads reference state roots from a JSONL file for validation.
    /// Each line: {"height": N, "roothash": "0x..."}
    pub fn load_reference_roots(&mut self, path: &str) {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) => {
                warn!(target: "neo::state_service", "reference roots file not found: {path}: {e}");
                return;
            }
        };
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead;
        let mut count = 0u32;
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            if line.trim().is_empty() {
                continue;
            }

            let Ok(reference) = serde_json::from_str::<ReferenceRootLine>(&line) else {
                continue;
            };
            if let Ok(hash) = UInt256::parse(&reference.roothash) {
                self.reference_roots.insert(reference.height, hash);
                count += 1;
            }
        }
        info!(target: "neo::state_service", "loaded {count} reference state roots for validation");
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
        provider: Arc<dyn StoreProvider>,
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

    /// Initializes the MPT trie from the current blockchain storage state.
    ///
    /// This must be called once when state root computation is first enabled on a node
    /// that already has synced blocks. Without this, the trie starts empty and only
    /// contains block deltas, producing incorrect state roots.
    ///
    /// If the trie already has a stored root (i.e. state roots were previously computed),
    /// this is a no-op.
    pub fn initialize_trie_from_store(&self, blockchain_store: &Arc<dyn IStore>) {
        // Check if we already have a stored root — if so, no initialization needed.
        if self.local_root_index().is_some() {
            info!(target: "neo", "state trie already initialized, skipping population");
            return;
        }

        // Determine the current block height so the initial state root can be
        // recorded at the correct index.  Without this metadata subsequent
        // snapshots will start from an empty trie after restart, causing every
        // state root to diverge from the C# reference.
        //
        // The current-block key is: contract_id (4 LE bytes) + prefix byte 12.
        // The value is: block_hash (32 bytes) + index (4 LE bytes).
        let current_height = {
            let bc_snapshot = blockchain_store.get_snapshot();
            let mut cb_key = LedgerContract::ID.to_le_bytes().to_vec();
            cb_key.push(12u8); // PREFIX_CURRENT_BLOCK
            bc_snapshot
                .try_get(&cb_key)
                .and_then(|v: Vec<u8>| {
                    if v.len() >= 36 {
                        Some(u32::from_le_bytes([v[32], v[33], v[34], v[35]]))
                    } else {
                        None
                    }
                })
                .unwrap_or(0)
        };

        // When importing from genesis (block 0), the trie must start EMPTY and
        // build incrementally as each block is persisted.  Pre-populating from
        // current storage would include all future state, making every historical
        // state root incorrect and causing contract storage divergence.
        if current_height == 0 {
            info!(target: "neo",
                "blockchain at genesis — starting state trie empty for incremental computation");
            return;
        }

        info!(target: "neo", "initializing state trie from current blockchain storage at height {}", current_height);

        let snapshot = blockchain_store.get_snapshot();
        let mut state_snap = self.get_snapshot();
        let mut count: u64 = 0;

        // Iterate over ALL storage entries and insert into the trie,
        // excluding LedgerContract storage (matching C# behavior).
        // The raw find returns (Vec<u8>, Vec<u8>) where values are stored
        // as get_value() output (raw bytes without is_constant prefix).
        let entries: Vec<_> = snapshot.find(None, SeekDirection::Forward).collect();
        for (key_bytes, value_bytes) in entries {
            let key = StorageKey::from_bytes(&key_bytes);
            if key.id == LedgerContract::ID {
                continue;
            }
            if let Err(e) = state_snap.trie.put(&key_bytes, &value_bytes) {
                warn!(target: "neo", "failed to insert key into trie during init: {:?}", e);
            }
            count += 1;
        }

        // Compute and store the root hash for the current state.
        let root_hash = state_snap.trie.root_hash().unwrap_or_else(UInt256::zero);
        info!(target: "neo",
            "state trie initialized with {} entries, root hash: {:?}, height: {}",
            count,
            root_hash,
            current_height
        );

        // Record the state root metadata BEFORE committing so the backend's
        // pending buffer includes both trie nodes and root metadata.  Without
        // this, get_snapshot() after restart cannot locate the root and creates
        // an empty trie.
        let state_root = StateRoot::new_current(current_height, root_hash);
        if let Err(e) = state_snap.add_local_state_root(&state_root) {
            warn!(target: "neo", "failed to store initial state root metadata: {:?}", e);
        }

        // Commit the populated trie AND the root metadata to the backend.
        state_snap.commit().expect("commit initial trie");

        // Update current snapshot to reflect the initialized state.
        *self.current_snapshot.write() = Some(self.get_snapshot());
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
        if let Err(e) = tx.commit() {
            tracing::error!(
                target: "state",
                index = state_root.index,
                error = %e,
                "rejecting state root: commit failed"
            );
            metrics::record_ingest_result(false);
            return false;
        }
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
    ) -> Result<(), String> {
        // Skip blocks that have already been processed (e.g., after node restart).
        // The C# StateService does the same check: if local root index >= block height, skip.
        if let Some(current_index) = self.local_root_index() {
            if height <= current_index {
                return Ok(());
            }
        }

        let mut snapshot = self.get_snapshot();
        let mut put_count: u32 = 0;
        let mut del_count: u32 = 0;
        let mut _skip_count: u32 = 0;
        let mut _ledger_skip: u32 = 0;
        let debug_dump =
            height == 172613 || height == 182902 || height == 203262 || height == 274157;
        for (key, item, state) in change_set {
            if debug_dump {
                tracing::warn!(
                    target: "neo::state_service",
                    height,
                    contract_id = key.id,
                    key = %hex::encode(key.as_bytes()),
                    state = ?state,
                    value = %hex::encode(item.value_bytes()),
                    "DEBUG_WRITE"
                );
            }
            // Match Neo.Plugins.StateService behaviour: exclude ledger contract storage
            // from trie updates to keep state root consensus-compatible.
            if key.id == LedgerContract::ID {
                _ledger_skip += 1;
                continue;
            }
            let key_bytes = key.as_bytes();
            match state {
                TrackState::Added | TrackState::Changed => {
                    let value_bytes = item.value_bytes();
                    snapshot.trie.put(&key_bytes, &value_bytes).map_err(|e| {
                        format!(
                            "state root trie put failed at height {height} for contract {}: {e}",
                            key.id
                        )
                    })?;
                    put_count += 1;
                }
                TrackState::Deleted => {
                    snapshot.trie.delete(&key_bytes).map_err(|e| {
                        format!(
                            "state root trie delete failed at height {height} for contract {}: {e}",
                            key.id
                        )
                    })?;
                    del_count += 1;
                }
                TrackState::None | TrackState::NotFound => {
                    _skip_count += 1;
                }
            }
        }

        // Get new root hash
        let root_hash = snapshot.trie.root_hash().unwrap_or_else(UInt256::zero);

        // Validate against reference if available
        if let Some(expected) = self.reference_roots.get(&height) {
            if root_hash == *expected {
                if height % 5000 == 0 {
                    tracing::info!(
                        target: "neo::state_service",
                        height,
                        root_hash = %root_hash,
                        "state root MATCH (checkpoint)"
                    );
                }
            } else {
                return Err(format!(
                    "state root mismatch at height {height}: computed {root_hash}, expected {expected}, put_count {put_count}, del_count {del_count}"
                ));
            }
        } else if height % 5000 == 0 {
            tracing::info!(
                target: "neo::state_service",
                height,
                put_count,
                del_count,
                root_hash = %root_hash,
                "state root computed (no reference)"
            );
        }

        // Create and store state root
        let state_root = StateRoot::new_current(height, root_hash);
        snapshot
            .add_local_state_root(&state_root)
            .map_err(|e| format!("failed to stage local state root at height {height}: {e}"))?;

        *self.state_snapshot.write() = Some(snapshot);
        Ok(())
    }

    /// Commits the local state root after block persistence.
    pub fn update_local_state_root(&self, height: u32) -> Result<(), String> {
        // Commit and dispose snapshot
        {
            let mut state_snap = self.state_snapshot.write();
            if let Some(ref mut snapshot) = *state_snap {
                snapshot.commit()?;
            }
            *state_snap = None;
        }

        self.update_current_snapshot();
        self.check_validated_state_root(height);
        Ok(())
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
