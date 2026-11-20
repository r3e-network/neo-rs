//! Commit handlers for state service operations.
//!
//! This module provides handlers for committing state changes during block processing.
//! It implements the state root computation and persistence logic that runs during
//! blockchain commit events.
//!
//! # State root computation
//!
//! The state root is the Merkle Patricia Trie (MPT) root hash over the node's
//! contract storage. Each block's storage changes (from the block snapshot) are
//! applied to an incremental trie that extends the previously persisted root, so:
//! - blocks with different changes produce different roots,
//! - two nodes applying the same changes derive the identical root,
//! - any individual storage entry can be proven against a root with an MPT
//!   inclusion proof (see `state_verification::verify_merkle_proof`).
//!
//! # Persistence layout
//!
//! Both trie nodes and per-block root records are stored through the shared
//! [`StoreCache`] under the dedicated storage id [`STATE_ROOT_STORAGE_ID`]
//! (`i32::MIN`), which cannot collide with native contracts (small negative
//! ids) or deployed contracts (positive ids):
//! - root records: key `[PREFIX_STATE_ROOT] + index (u32, big-endian)`,
//! - trie nodes: key `[PREFIX_TRIE_NODE] + MPT node key`.
//!
//! Because both live in the shared store cache they commit atomically with the
//! block that produced them.
//!
//! # Reference
//! C# Neo StateService: neo/Plugins/StateService/StatePlugin.cs
//! - Blockchain_Committing event handler computes state roots
//! - Blockchain_Committed event handler notifies and persists state roots

use super::context::StateCommitContext;
use super::state_root::CURRENT_VERSION;
use super::{StateRoot, ValidatedRootPersisted};
use crate::store::PREFIX_LOCAL_ROOT_INDEX;
use neo_crypto::mpt_trie::{
    GlobalNodeCache, MptError, MptProfile, MptProfileSnapshot, MptResult, MptStoreSnapshot, Trie,
    node_key,
};
use neo_primitives::UInt256;
use neo_storage::persistence::{PersistenceDataCache, SeekDirection, StoreCache};
use neo_storage::types::{StorageItem, StorageKey};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, error, warn};

/// Wall-clock substages of state-root commitment, emitted only when a sink is attached.
#[derive(Clone, Copy, Debug, Default)]
pub struct StateRootCommitProfile {
    pub change_extraction: Duration,
    pub mpt: Duration,
    pub mpt_detail: MptProfileSnapshot,
}

/// Observation-only state-root profiling callback.
pub type StateRootProfileSink = Arc<dyn Fn(StateRootCommitProfile) + Send + Sync>;

/// Error type for state service operations.
#[derive(Debug, Clone)]
pub struct StateServiceError {
    pub message: String,
}

impl std::fmt::Display for StateServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "State service error: {}", self.message)
    }
}

impl std::error::Error for StateServiceError {}

impl StateServiceError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Dedicated storage identifier for state-service records.
///
/// Native contracts occupy small negative ids and deployed contracts positive
/// ids, so `i32::MIN` can never collide with either namespace.
pub(crate) const STATE_ROOT_STORAGE_ID: i32 = i32::MIN;

/// Key prefix for persisted state-root records, followed by the block index
/// as a big-endian `u32` so records sort by height.
pub(crate) const PREFIX_STATE_ROOT: u8 = 0xf0;

/// Key prefix for persisted MPT trie nodes, followed by the trie-internal
/// node key (which already carries the MPT node prefix byte).
pub(crate) const PREFIX_TRIE_NODE: u8 = 0xf1;

/// Builds the storage key under which the state-root record for `index` is
/// persisted.
pub(crate) fn state_root_record_key(index: u32) -> StorageKey {
    StorageKey::create_with_uint32(STATE_ROOT_STORAGE_ID, PREFIX_STATE_ROOT, index)
}

/// Serializes a state-root record for storage.
///
/// Layout: `root_hash (32 bytes) + version (1 byte) + witness flag (1 byte)`
/// followed, when a witness is present, by length-prefixed invocation and
/// verification scripts (`u32` little-endian lengths).
pub(crate) fn serialize_state_root_record(state_root: &StateRoot) -> Vec<u8> {
    let mut value = Vec::with_capacity(34);
    value.extend_from_slice(state_root.root_hash.as_ref());
    value.push(state_root.version);
    if let Some(ref witness) = state_root.witness {
        value.push(1);
        let inv_len = witness.invocation_script().len() as u32;
        value.extend_from_slice(&inv_len.to_le_bytes());
        value.extend_from_slice(witness.invocation_script());
        let ver_len = witness.verification_script().len() as u32;
        value.extend_from_slice(&ver_len.to_le_bytes());
        value.extend_from_slice(witness.verification_script());
    } else {
        value.push(0);
    }
    value
}

/// Deserializes a state-root record persisted by
/// [`serialize_state_root_record`].
///
/// Returns `None` for malformed input (fail-closed); the block index is taken
/// from the caller because it is encoded in the storage key, not the value.
pub(crate) fn deserialize_state_root_record(index: u32, bytes: &[u8]) -> Option<StateRoot> {
    if bytes.len() < 34 {
        return None;
    }
    let root_hash = UInt256::from_bytes(&bytes[..32]).ok()?;
    let version = bytes[32];
    let witness = match bytes[33] {
        0 => None,
        1 => {
            let mut offset = 34;
            let invocation = read_length_prefixed(bytes, &mut offset)?;
            let verification = read_length_prefixed(bytes, &mut offset)?;
            if offset != bytes.len() {
                return None;
            }
            Some(neo_protocol::witness::Witness::new_with_scripts(
                invocation,
                verification,
            ))
        }
        _ => return None,
    };
    let mut state_root = StateRoot::new_current(index, root_hash);
    state_root.version = version;
    state_root.witness = witness;
    Some(state_root)
}

/// Reads and strictly decodes the canonical state-root record at `index`.
///
/// Unlike [`StateServiceCommitHandlers::get_state_root`], this diagnostic API
/// distinguishes a missing record from malformed persisted bytes.
pub fn read_state_root_record(
    store_cache: &StoreCache,
    index: u32,
) -> Result<Option<StateRoot>, String> {
    let Some(item) = store_cache.get(&state_root_record_key(index)) else {
        return Ok(None);
    };
    deserialize_state_root_record(index, item.value())
        .map(Some)
        .ok_or_else(|| format!("malformed persisted state-root record at height {index}"))
}

/// Reads the durable local-commit cursor (`PREFIX_LOCAL_ROOT_INDEX`).
///
/// The cursor is advanced only inside a block commit, so it names the highest
/// height whose backing block exists locally.
fn read_local_root_cursor(store_cache: &StoreCache) -> Result<Option<u32>, StateServiceError> {
    let key = StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_LOCAL_ROOT_INDEX]);
    let Some(item) = store_cache.get(&key) else {
        return Ok(None);
    };
    let bytes: [u8; 8] = item
        .value()
        .try_into()
        .map_err(|_| StateServiceError::new("malformed persisted local state-root cursor"))?;
    let index = u32::try_from(u64::from_be_bytes(bytes)).map_err(|_| {
        StateServiceError::new("persisted local state-root cursor exceeds u32 heights")
    })?;
    Ok(Some(index))
}

/// Reads a `u32` little-endian length followed by that many bytes.
fn read_length_prefixed(bytes: &[u8], offset: &mut usize) -> Option<Vec<u8>> {
    if *offset + 4 > bytes.len() {
        return None;
    }
    let len = u32::from_le_bytes([
        bytes[*offset],
        bytes[*offset + 1],
        bytes[*offset + 2],
        bytes[*offset + 3],
    ]) as usize;
    *offset += 4;
    if *offset + len > bytes.len() {
        return None;
    }
    let data = bytes[*offset..*offset + len].to_vec();
    *offset += len;
    Some(data)
}

/// Persistent MPT node reader used for historical proofs.
pub(crate) struct TrieNodeStore {
    store_cache: Arc<StoreCache>,
}

impl TrieNodeStore {
    /// Maps an MPT-internal node key onto its namespaced storage key.
    fn namespaced_key(mpt_key: &[u8]) -> StorageKey {
        let mut key = Vec::with_capacity(1 + mpt_key.len());
        key.push(PREFIX_TRIE_NODE);
        key.extend_from_slice(mpt_key);
        StorageKey::new(STATE_ROOT_STORAGE_ID, key)
    }
}

impl MptStoreSnapshot for TrieNodeStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self
            .store_cache
            .get(&Self::namespaced_key(key))
            .map(|item| item.value().to_vec()))
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        let storage_key = Self::namespaced_key(&key);
        let item = StorageItem::from_bytes(value);
        let result = if self.store_cache.get(&storage_key).is_some() {
            self.store_cache.data_cache().try_update(storage_key, item)
        } else {
            self.store_cache.data_cache().try_add(storage_key, item)
        };
        result.map_err(|e| MptError::storage(format!("failed to persist MPT node: {e}")))
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        let storage_key = Self::namespaced_key(&key);
        if self.store_cache.get(&storage_key).is_some() {
            self.store_cache
                .data_cache()
                .try_delete(&storage_key)
                .map_err(|e| MptError::storage(format!("failed to delete MPT node: {e}")))?;
        }
        Ok(())
    }
}

/// Block-scoped MPT writes. All new nodes are staged in the exact snapshot
/// committed by the ledger; previously committed nodes remain readable through
/// the persistent store. No independent cache write can survive a block abort.
struct BlockTrieNodeStore {
    persisted: Arc<TrieNodeStore>,
    snapshot: PersistenceDataCache,
}

impl BlockTrieNodeStore {
    fn get_item(&self, key: &StorageKey) -> Option<StorageItem> {
        let item = self.snapshot.get(key);
        if item.is_some() || self.snapshot.has_backing_store() {
            item
        } else {
            // Detached test/embedder snapshots have no persistent read view.
            self.persisted.store_cache.get(key)
        }
    }
}

impl MptStoreSnapshot for BlockTrieNodeStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        let storage_key = TrieNodeStore::namespaced_key(key);
        Ok(self
            .get_item(&storage_key)
            .map(|item| item.value().to_vec()))
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        let storage_key = TrieNodeStore::namespaced_key(&key);
        let item = StorageItem::from_bytes(value);
        let result = if self.get_item(&storage_key).is_some() {
            self.snapshot.try_update(storage_key, item)
        } else {
            self.snapshot.try_add(storage_key, item)
        };
        result.map_err(|e| MptError::storage(format!("failed to stage MPT node: {e}")))
    }

    fn put_with_hint(
        &self,
        key: Vec<u8>,
        value: Vec<u8>,
        known_absent_in_store: bool,
    ) -> MptResult<()> {
        if !known_absent_in_store {
            return self.put(key, value);
        }

        // MptCache established absence while resolving this node. Keep the
        // snapshot's own state machine fail-closed: try_add still rejects an
        // unexpected Added/Changed/None entry instead of silently upserting it.
        self.snapshot
            .try_add(
                TrieNodeStore::namespaced_key(&key),
                StorageItem::from_bytes(value),
            )
            .map_err(|e| MptError::storage(format!("failed to stage new MPT node: {e}")))
    }

    fn put_full_state(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        // Full-state tries never use reference counts to delete history. Stage
        // the content-addressed body directly instead of reading the previous
        // reference count from RocksDB merely to increment an unused field.
        self.snapshot
            .try_update(
                TrieNodeStore::namespaced_key(&key),
                StorageItem::from_bytes(value),
            )
            .map_err(|e| MptError::storage(format!("failed to stage full-state MPT node: {e}")))
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        let storage_key = TrieNodeStore::namespaced_key(&key);
        if self.get_item(&storage_key).is_some() {
            self.snapshot
                .try_delete(&storage_key)
                .map_err(|e| MptError::storage(format!("failed to stage MPT deletion: {e}")))?;
        }
        Ok(())
    }
}

/// Handles state service commit operations during block processing.
///
/// This handler computes and persists state roots for each committed block,
/// enabling state proof verification and light client support.
///
/// # Architecture
/// - `blockchain_committing_handler`: Computes state root from the block's
///   storage changes before commit
/// - `blockchain_committed_handler`: Persists state root and emits notification
///   after commit
/// - `commit_block`: Direct commit interface for explicit state root computation
/// - `rollback`: Reverts uncommitted state changes
/// - `recover_from_storage`: Re-seeds the trie baseline after a restart
/// Maximum compact MPT nodes retained by each state-service store.
///
/// This is deliberately store-scoped rather than process-global: independent
/// chains cannot share entries, and a restart always begins with a cold cache.
const STATE_ROOT_NODE_CACHE_CAPACITY: usize = 65_536;

/// Observation-only snapshot of a state-service store's compact-node cache.
///
/// `Default` is the zero snapshot a caller uses when the state service is not
/// registered; `enabled` distinguishes that from a registered but idle cache.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StateRootNodeCacheStats {
    pub enabled: bool,
    pub capacity: usize,
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate_percent: f32,
    /// Resolved nodes reachable from the committed warm root, excluding hash stubs.
    pub warm_root_nodes: usize,
    /// Owned-node and buffer estimate; excludes allocator, Arc and lock overhead.
    pub warm_root_estimated_bytes: usize,
}

#[derive(Clone)]
pub struct StateServiceCommitHandlers {
    /// Store cache for state root persistence
    store_cache: Arc<StoreCache>,
    /// Whether state root tracking is enabled
    state_root_enabled: bool,
    /// Pending state root computed during committing phase
    pending_state_root: Arc<Mutex<Option<StateRoot>>>,
    /// Fully resolved paths from the pending trie. This advances only together
    /// with `pending_state_root`, after the block snapshot accepts its writes.
    pending_root_node: Arc<Mutex<Option<neo_crypto::mpt_trie::Node>>>,
    /// Content-addressed MPT node store shared by every trie this handler builds
    node_store: Arc<TrieNodeStore>,
    /// Bounded, store-scoped compact nodes reused across block boundaries.
    /// Full-state tries never use cached reference counts to prune old roots.
    node_cache: Arc<Mutex<Option<Arc<GlobalNodeCache>>>>,
    /// Shared across clones so an offline A/B switch has one consistent value.
    node_cache_enabled: Arc<AtomicBool>,
    /// Root hash of the last successfully persisted state trie
    current_root: Arc<Mutex<Option<UInt256>>>,
    /// Structural-sharing root retained only after the corresponding block is
    /// durable. Copy-on-write keeps this committed baseline immutable.
    current_root_node: Arc<Mutex<Option<neo_crypto::mpt_trie::Node>>>,
    /// Optional observer notified whenever a validated root is persisted
    event_sink: Option<Arc<dyn Fn(ValidatedRootPersisted) + Send + Sync>>,
    /// Optional diagnostic sink shared with clones of this handler.
    profile_sink: Arc<Mutex<Option<StateRootProfileSink>>>,
}

impl StateServiceCommitHandlers {
    /// Creates a new StateServiceCommitHandlers instance.
    ///
    /// # Arguments
    /// * `store_cache` - Store cache for persisting state roots and trie nodes
    /// * `state_root_enabled` - Whether to compute and track state roots
    ///
    /// # Reference
    /// C# Neo StatePlugin constructor initializes event handlers
    pub fn new(
        store_cache: Arc<StoreCache>,
        state_root_enabled: bool,
    ) -> Result<Self, StateServiceError> {
        Ok(Self {
            node_store: Arc::new(TrieNodeStore {
                store_cache: store_cache.clone(),
            }),
            store_cache,
            state_root_enabled,
            pending_state_root: Arc::new(Mutex::new(None)),
            pending_root_node: Arc::new(Mutex::new(None)),
            node_cache: Arc::new(Mutex::new(state_root_enabled.then(|| {
                Arc::new(GlobalNodeCache::with_capacity(
                    NonZeroUsize::new(STATE_ROOT_NODE_CACHE_CAPACITY)
                        .expect("state-root node cache capacity is nonzero"),
                ))
            }))),
            node_cache_enabled: Arc::new(AtomicBool::new(true)),
            current_root: Arc::new(Mutex::new(None)),
            current_root_node: Arc::new(Mutex::new(None)),
            event_sink: None,
            profile_sink: Arc::new(Mutex::new(None)),
        })
    }

    /// Enables or bypasses compact-node caching for subsequent trie operations.
    /// Disabling does not clear existing entries or reset statistics.
    pub fn set_node_cache_enabled(&self, enabled: bool) {
        self.node_cache_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Replaces the cache with an empty bounded cache for an explicit cold A/B run.
    /// Content-addressed persistent nodes and the committed root are unaffected.
    pub fn reset_node_cache_capacity(&self, capacity: NonZeroUsize) {
        let mut cache = self.node_cache.lock().unwrap();
        if cache.is_some() {
            *cache = Some(Arc::new(GlobalNodeCache::with_capacity(capacity)));
        }
    }

    /// Returns cache activity without changing execution or eviction state.
    #[must_use]
    pub fn node_cache_stats(&self) -> StateRootNodeCacheStats {
        let cache = self.node_cache.lock().unwrap().clone();
        let (hits, misses, hit_rate_percent) =
            cache.as_ref().map_or((0, 0, 0.0), |cache| cache.stats());
        let (warm_root_nodes, warm_root_estimated_bytes) = self
            .current_root_node
            .lock()
            .unwrap()
            .as_ref()
            .map_or((0, 0), neo_crypto::Node::resolved_retention_stats);
        StateRootNodeCacheStats {
            enabled: cache.is_some() && self.node_cache_enabled.load(Ordering::Relaxed),
            capacity: cache.as_ref().map_or(0, |cache| cache.capacity()),
            entries: cache.as_ref().map_or(0, |cache| cache.len()),
            hits,
            misses,
            hit_rate_percent,
            warm_root_nodes,
            warm_root_estimated_bytes,
        }
    }

    fn active_node_cache(&self) -> Option<Arc<GlobalNodeCache>> {
        if self.node_cache_enabled.load(Ordering::Relaxed) {
            self.node_cache.lock().unwrap().clone()
        } else {
            None
        }
    }

    /// Attaches or removes the observation-only state-root timing sink.
    pub fn set_profile_sink(&self, sink: Option<StateRootProfileSink>) {
        *self.profile_sink.lock().unwrap() = sink;
    }

    /// Returns whether a state-root timing sink is currently attached.
    ///
    /// This is observation-only introspection used to prove the embedding
    /// wired profiling; it never enables profiling by itself.
    #[must_use]
    pub fn profile_sink_registered(&self) -> bool {
        self.profile_sink.lock().unwrap().is_some()
    }

    /// Attaches an observer invoked whenever a validated state root is
    /// persisted and announced.
    ///
    /// This is the embedding seam for consumers that cannot be reached through
    /// the `NeoSystemContext` event stream (e.g. tests and embedders).
    #[must_use]
    pub fn with_event_sink(
        mut self,
        sink: Arc<dyn Fn(ValidatedRootPersisted) + Send + Sync>,
    ) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Commits state changes for a block by computing its state root.
    ///
    /// This method:
    /// 1. Extracts the block's storage changes from the snapshot
    /// 2. Applies them to the incremental MPT trie extending the previously
    ///    persisted root
    /// 3. Stages the resulting trie nodes and root record in this block's snapshot
    /// 4. Stores the root as pending, awaiting the committed handler
    ///
    /// A block with no storage changes keeps the previous root unchanged. A
    /// block with no changes and no previous root is rejected: an empty state
    /// trie has no representable root in this MPT port.
    ///
    /// # Arguments
    /// * `height` - Block height to compute state root for
    /// * `snapshot` - Data cache carrying the block's storage changes
    ///
    /// # Reference
    /// C# Neo StateService: StatePlugin.OnCommitting computes state root from snapshot
    pub fn commit_block(
        &self,
        height: u32,
        snapshot: &PersistenceDataCache,
    ) -> Result<(), StateServiceError> {
        self.commit_block_with_parallelism(height, snapshot, true)
    }

    fn commit_block_with_parallelism(
        &self,
        height: u32,
        snapshot: &PersistenceDataCache,
        parallel_hash: bool,
    ) -> Result<(), StateServiceError> {
        if !self.state_root_enabled {
            return Ok(());
        }

        debug!(
            target: "neo::state_service",
            height,
            "computing state root for block"
        );

        let profile_sink = self.profile_sink.lock().unwrap().clone();
        let extraction_started = profile_sink.as_ref().map(|_| Instant::now());
        let mpt_profile = profile_sink
            .as_ref()
            .map(|_| Arc::new(MptProfile::default()));

        // Capture contract changes before adding state-service entries to the snapshot.
        // Neo's MPT commits contract storage only. LedgerContract indexes (-4)
        // are chain bookkeeping and are deliberately excluded by both C# and Go.
        let mut changes = snapshot
            .extract_raw_changes()
            .into_iter()
            .filter(|(key, _)| key.len() < 4 || key[..4] != (-4_i32).to_le_bytes())
            .collect::<Vec<_>>();
        // PersistenceDataCache uses a HashMap, so its iteration order randomly jumps around
        // the trie. Lexicographic storage-key order is also MPT nibble-path order:
        // adjacent updates can then reuse the path resolved by the previous put.
        // Keys are unique and MPT updates are order-independent for the final root.
        changes.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let change_extraction = extraction_started.map(|started| started.elapsed());
        let mpt_started = profile_sink.as_ref().map(|_| Instant::now());
        let (root_hash, root_node) =
            self.compute_state_root(snapshot, &changes, mpt_profile.clone(), parallel_hash)?;
        let mpt = mpt_started.map(|started| started.elapsed());
        let state_root = StateRoot::new_current(height, root_hash);
        snapshot
            .try_add(
                state_root_record_key(height),
                StorageItem::from_bytes(serialize_state_root_record(&state_root)),
            )
            .map_err(|error| {
                StateServiceError::new(format!(
                    "failed to stage state root at index {height}: {error}"
                ))
            })?;
        *self.pending_state_root.lock().unwrap() = Some(state_root);
        *self.pending_root_node.lock().unwrap() = Some(root_node);
        if let (Some(sink), Some(change_extraction), Some(mpt)) =
            (profile_sink, change_extraction, mpt)
        {
            sink(StateRootCommitProfile {
                change_extraction,
                mpt,
                mpt_detail: mpt_profile
                    .as_deref()
                    .map(MptProfile::snapshot)
                    .unwrap_or_default(),
            });
        }

        debug!(
            target: "neo::state_service",
            height,
            %root_hash,
            changes = changes.len(),
            "state root computed and staged successfully"
        );

        Ok(())
    }

    /// Computes the state root hash by applying the given storage changes to
    /// the incremental MPT trie.
    ///
    /// The trie extends the previously persisted root, so historical roots
    /// remain provable (shared nodes are never removed: `full_state = true`).
    ///
    /// # Reference
    /// C# Neo StateService: GetLocalRootHash() - computes MPT root from snapshot
    fn compute_state_root(
        &self,
        snapshot: &PersistenceDataCache,
        changes: &[(Vec<u8>, Option<Vec<u8>>)],
        profile: Option<Arc<MptProfile>>,
        parallel_hash: bool,
    ) -> Result<(UInt256, neo_crypto::mpt_trie::Node), StateServiceError> {
        let previous_root = *self.current_root.lock().unwrap();

        if changes.is_empty() && previous_root.is_none() {
            return Err(StateServiceError::new(
                "cannot compute a state root: the state trie is empty \
                 (no previous root and no storage changes in this block)",
            ));
        }

        let block_store = Arc::new(BlockTrieNodeStore {
            persisted: self.node_store.clone(),
            snapshot: snapshot.clone(),
        });
        // Cloning retains immutable Arc subtrees. Trie updates detach every node
        // on the modified path, so an aborted block cannot mutate this baseline.
        let root_node = self.current_root_node.lock().unwrap().clone();
        let root_node = root_node.unwrap_or_else(|| {
            previous_root.map_or_else(neo_crypto::mpt_trie::Node::new, |root| {
                neo_crypto::mpt_trie::Node::new_hash(root)
            })
        });
        let mut trie = Trie::new_with_root_node(
            block_store,
            root_node,
            // Full-state mode retains nodes shared with older roots, which is
            // required for proof generation against historical roots.
            true,
            self.active_node_cache(),
        );
        if let Some(profile) = profile {
            trie = trie.with_profile(profile);
        }

        for (key, value) in changes {
            match value {
                Some(value) => trie.put(key, value).map_err(|e| {
                    StateServiceError::new(format!("MPT put failed for storage key: {e}"))
                })?,
                None => {
                    trie.delete(key).map_err(|e| {
                        StateServiceError::new(format!("MPT delete failed for storage key: {e}"))
                    })?;
                }
            }
        }

        // Flush the new/changed nodes into the shared node store so they commit
        // atomically with this block.
        trie.commit_with_parallelism(parallel_hash)
            .map_err(|e| StateServiceError::new(format!("MPT commit failed: {e}")))?;

        let root_hash = trie.root_hash().ok_or_else(|| {
            StateServiceError::new(
                "cannot compute a state root: applying this block left the state trie empty",
            )
        })?;
        Ok((root_hash, trie.take_root()))
    }

    /// Returns the root hash of the most recently persisted state trie.
    pub fn current_root_hash(&self) -> Option<UInt256> {
        *self.current_root.lock().unwrap()
    }

    /// Returns the persisted state-root record for `index`, if any.
    ///
    /// Records are readable immediately after persistence (through the shared
    /// store cache) and after restart (through the underlying store).
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        read_state_root_record(&self.store_cache, index)
            .ok()
            .flatten()
    }

    /// Re-seeds the incremental trie baseline from persisted state-root records.
    ///
    /// After a restart the handler has no in-memory root, but root records and
    /// trie nodes persisted by earlier runs live in the store. Recovery adopts
    /// **only the locally committed root**:
    ///
    /// 1. the durable local cursor ([`PREFIX_LOCAL_ROOT_INDEX`]) names the last
    ///    height whose block this node actually committed, so an unsigned root
    ///    that arrived from the network for a block that does not exist locally
    ///    can never be adopted (S-1);
    /// 2. without a cursor (databases written before cursors existed) the
    ///    highest *unsigned* record is used, because only a record without a
    ///    witness proves this node computed the root itself;
    /// 3. the root's backing trie node must exist locally, otherwise extending
    ///    the trie would silently start from state that is not here.
    ///
    /// Returns the block index of the recovered root, or `None` when no valid
    /// local records exist.
    pub fn recover_from_storage(&self) -> Result<Option<u32>, StateServiceError> {
        let Some(index) = self.recoverable_local_root_index()? else {
            debug!(
                target: "neo::state_service",
                "no locally committed state roots found; starting from an empty trie"
            );
            return Ok(None);
        };

        let root = read_state_root_record(&self.store_cache, index)
            .map_err(StateServiceError::new)?
            .ok_or_else(|| {
                StateServiceError::new(format!(
                    "cannot recover state root: local progress names height {index} but no record is persisted"
                ))
            })?;

        if root.version != CURRENT_VERSION {
            return Err(StateServiceError::new(format!(
                "cannot recover state root: unsupported version {} at height {index}",
                root.version
            )));
        }

        self.ensure_root_node_is_durable(&root.root_hash, index)?;

        *self.current_root.lock().unwrap() = Some(root.root_hash);
        // Recovery deliberately starts from a HashNode. Only a root whose nodes
        // are known durable may become a warm baseline.
        *self.current_root_node.lock().unwrap() = None;
        *self.pending_root_node.lock().unwrap() = None;
        debug!(
            target: "neo::state_service",
            index,
            root = %root.root_hash,
            "recovered state root baseline from storage"
        );
        Ok(Some(index))
    }

    /// Returns the height whose root may be adopted at startup.
    ///
    /// The local cursor wins unconditionally: it is the only durable proof that
    /// the block backing the root was committed here. The legacy scan is a
    /// fallback for databases without a cursor and considers unsigned records
    /// only, since a signed record may describe a network height this node has
    /// not reached.
    fn recoverable_local_root_index(&self) -> Result<Option<u32>, StateServiceError> {
        if let Some(index) = read_local_root_cursor(&self.store_cache)? {
            return Ok(Some(index));
        }

        let prefix_key = StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_STATE_ROOT]);
        let mut highest: Option<u32> = None;
        for (key, item) in self
            .store_cache
            .find(Some(&prefix_key), SeekDirection::Forward)
        {
            let raw = key.key();
            if raw.len() != 5 || raw[0] != PREFIX_STATE_ROOT {
                // Unrelated entry inside the state-service namespace; skip it.
                continue;
            }
            let index = u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]]);
            let Some(record) = deserialize_state_root_record(index, item.value()) else {
                warn!(
                    target: "neo::state_service",
                    index,
                    "skipping malformed persisted state root record"
                );
                continue;
            };
            if record.witness.is_some() {
                // Quorum-validated network root: it proves nothing about local
                // block progress and must never seed the local trie.
                continue;
            }
            if highest.is_none_or(|current| index > current) {
                highest = Some(index);
            }
        }
        Ok(highest)
    }

    /// Fails unless the root node of `root_hash` is present in the local store.
    ///
    /// A root record can be persisted without its trie (a network root, a
    /// partially written batch, or pruned history). Adopting such a root makes
    /// every later trie operation fail or, worse, extend a nonexistent state.
    fn ensure_root_node_is_durable(
        &self,
        root_hash: &UInt256,
        index: u32,
    ) -> Result<(), StateServiceError> {
        // The node-key layout belongs to `neo-crypto`'s trie module; the state
        // service only namespaces whatever key the trie uses.
        let node = self
            .node_store
            .try_get(&node_key(root_hash))
            .map_err(|e| {
                StateServiceError::new(format!(
                    "cannot recover state root: failed to read trie node {root_hash} for height {index}: {e}"
                ))
            })?;
        if node.is_none() {
            return Err(StateServiceError::new(format!(
                "cannot recover state root: trie node {root_hash} for height {index} is missing from local storage"
            )));
        }
        Ok(())
    }

    /// Rolls back uncommitted state changes.
    ///
    /// This clears any pending state root that was computed but not yet
    /// persisted, typically called when block commit fails or is cancelled.
    /// The trie baseline is untouched (it only advances on successful
    /// persistence), so the next block re-derives its root from the last
    /// confirmed state.
    ///
    /// # Reference
    /// C# Neo StateService: rollback clears pending state changes
    pub fn rollback(&self) -> Result<(), StateServiceError> {
        if !self.state_root_enabled {
            return Ok(());
        }

        debug!(
            target: "neo::state_service",
            "rolling back pending state root"
        );

        // Discard both representations of the speculative block. The committed
        // hash and warm root remain untouched and still describe durable state.
        *self.pending_state_root.lock().unwrap() = None;
        *self.pending_root_node.lock().unwrap() = None;

        Ok(())
    }

    /// Finalizes a root whose record and MPT nodes committed with the block.
    ///
    /// The committed phase performs no storage writes: it only consumes the
    /// pending value, advances the in-memory trie baseline and returns the root
    /// for event publication.
    fn persist_pending_state_root(
        &self,
        index: u32,
    ) -> Result<Option<StateRoot>, StateServiceError> {
        let state_root = {
            let mut pending = self.pending_state_root.lock().unwrap();
            pending.take()
        };
        let root_node = self.pending_root_node.lock().unwrap().take();

        let Some(state_root) = state_root else {
            return Ok(None);
        };

        if state_root.index != index {
            warn!(
                target: "neo::state_service",
                expected_index = index,
                actual_index = state_root.index,
                "state root index mismatch, skipping committed notification"
            );
            return Ok(None);
        }

        *self.current_root.lock().unwrap() = Some(state_root.root_hash);
        *self.current_root_node.lock().unwrap() = root_node;

        debug!(
            target: "neo::state_service",
            index,
            root_hash = %state_root.root_hash,
            "committed state root finalized"
        );

        Ok(Some(state_root))
    }

    /// Stages the local-root cursor in the same snapshot as the block and root record.
    fn stage_local_root_progress(
        &self,
        context: Option<&dyn StateCommitContext>,
        snapshot: &PersistenceDataCache,
    ) -> Result<(), StateServiceError> {
        let Some(context) = context else {
            // Direct embedders without a system context retain the post-commit
            // fallback in StateRootStore::accept_local_root.
            return Ok(());
        };
        let Some(state_store) = context.state_store()? else {
            return Ok(());
        };
        let state_root = self
            .pending_state_root
            .lock()
            .unwrap()
            .as_ref()
            .cloned()
            .ok_or_else(|| StateServiceError::new("missing pending local state root"))?;
        if state_store.stage_local_root(&state_root, snapshot) {
            Ok(())
        } else {
            Err(StateServiceError::new(format!(
                "failed to stage local-root cursor at height {}",
                state_root.index
            )))
        }
    }

    /// Removes the in-memory staged marker when the block transaction aborts.
    fn discard_staged_local_root(&self, context: Option<&dyn StateCommitContext>, index: u32) {
        if let Some(context) = context
            && let Ok(Some(state_store)) = context.state_store()
        {
            state_store.discard_staged_local_root(index);
        }
    }

    /// Notifies downstream consumers that a validated state root was persisted.
    ///
    /// Channels, in order:
    /// 1. the injected event sink (embedding/test seam),
    /// 2. the `NeoSystemContext` actor event stream plus its registered
    ///    `StateStoreService` (production wiring),
    /// 3. a warning when neither channel is available — never a silent skip.
    fn announce_validated_root(
        &self,
        context: Option<&dyn StateCommitContext>,
        index: u32,
        state_root: &StateRoot,
    ) {
        let event = ValidatedRootPersisted { index };

        if let Some(sink) = &self.event_sink {
            sink(event.clone());
        }

        if let Some(context) = context {
            // Keep the registered state store service in sync with the new root.
            match context.state_store() {
                Ok(Some(state_store)) => {
                    let accepted = if state_root.witness.is_none() {
                        state_store.on_local_root_committed(state_root)
                    } else {
                        state_store.on_new_state_root(state_root.clone())
                    };
                    if !accepted {
                        warn!(
                            target: "neo::state_service",
                            index,
                            "state store service rejected the persisted state root"
                        );
                    }
                }
                Ok(None) => {
                    debug!(
                        target: "neo::state_service",
                        index,
                        "no state store service registered; skipping on_new_state_root notification"
                    );
                }
                Err(err) => {
                    warn!(
                        target: "neo::state_service",
                        index,
                        error = ?err,
                        "failed to resolve the state store service"
                    );
                }
            }
            context.publish_validated_root_persisted(event);
        } else if self.event_sink.is_none() {
            warn!(
                target: "neo::state_service",
                index,
                "ValidatedRootPersisted dropped: no event sink attached and the system context could not be resolved"
            );
        }
    }

    /// Indicates whether this handler should run during fast sync.
    ///
    /// State root computation should run during fast sync to maintain
    /// state root history for verification.
    pub fn run_during_fast_sync(&self) -> bool {
        self.state_root_enabled
    }

    /// Pre-commit hook: computes the state root from the snapshot before it is
    /// persisted to storage, then stages the local-root cursor.
    ///
    /// `context` is the runtime commit port; `None` retains the post-commit
    /// fallback for direct embedders without a system context.
    ///
    /// # Reference
    /// C# Neo StateService: Blockchain_Committing event handler
    pub fn blockchain_committing_handler(
        &self,
        context: Option<&dyn StateCommitContext>,
        block: &neo_protocol::execution::Block,
        snapshot: &PersistenceDataCache,
        _application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) {
        if !self.state_root_enabled {
            return;
        }

        let height = block.index();

        match self
            .commit_block_with_parallelism(height, snapshot, !block.transactions.is_empty())
            .and_then(|()| self.stage_local_root_progress(context, snapshot))
        {
            Ok(()) => {
                debug!(
                    target: "neo::state_service",
                    height,
                    "blockchain_committing: state root computed"
                );
            }
            Err(err) => {
                error!(
                    target: "neo::state_service",
                    height,
                    %err,
                    "blockchain_committing: failed to compute state root"
                );
            }
        }
    }

    /// Fallible committing hook for protocol-critical state root computation.
    ///
    /// Returns an error if state root computation fails, allowing the
    /// blockchain to handle the failure appropriately.
    pub fn try_blockchain_committing_handler(
        &self,
        context: Option<&dyn StateCommitContext>,
        block: &neo_protocol::execution::Block,
        snapshot: &PersistenceDataCache,
        _application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) -> Result<(), StateServiceError> {
        if !self.state_root_enabled {
            return Ok(());
        }

        let height = block.index();
        self.commit_block_with_parallelism(height, snapshot, !block.transactions.is_empty())
            .and_then(|()| self.stage_local_root_progress(context, snapshot))
            .map_err(|e| {
                error!(
                    target: "neo::state_service",
                    height,
                    error = %e,
                    "try_blockchain_committing_handler: state root computation failed"
                );
                e
            })
    }

    /// Rollback hook: discards the staged local-root marker and rolls back the
    /// incremental trie state after a failed participant vote or store commit.
    pub fn blockchain_rollback_handler(
        &self,
        context: Option<&dyn StateCommitContext>,
        block: &neo_protocol::execution::Block,
        _snapshot: &PersistenceDataCache,
    ) -> Result<(), StateServiceError> {
        self.discard_staged_local_root(context, block.index());
        self.rollback()
    }

    /// Post-commit hook: persists the state root computed during the committing
    /// phase and emits notification events.
    ///
    /// # Reference
    /// C# Neo StateService: Blockchain_Committed event handler
    /// - Persists state root to storage
    /// - Emits ValidatedRootPersisted event
    pub fn blockchain_committed_handler(
        &self,
        context: Option<&dyn StateCommitContext>,
        block: &neo_protocol::execution::Block,
    ) {
        if !self.state_root_enabled {
            return;
        }

        let index = block.index();

        match self.persist_pending_state_root(index) {
            Ok(Some(state_root)) => {
                self.announce_validated_root(context, index, &state_root);
                debug!(
                    target: "neo::state_service",
                    index,
                    "blockchain_committed: state root persisted and announced"
                );
            }
            Ok(None) => {
                debug!(
                    target: "neo::state_service",
                    index,
                    "blockchain_committed: no pending state root to persist"
                );
            }
            Err(err) => {
                error!(
                    target: "neo::state_service",
                    index,
                    %err,
                    "blockchain_committed: failed to persist state root"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::mpt_trie::{ProofCache, ProofCacheNamespace};
    use neo_protocol::execution::{ApplicationExecuted, Block, BlockHeader};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::store::Store;

    /// Builds a block snapshot carrying the given storage changes.
    fn snapshot_with_changes(changes: &[(i32, Vec<u8>, Vec<u8>)]) -> PersistenceDataCache {
        let snapshot = PersistenceDataCache::new(false);
        for (id, key, value) in changes {
            snapshot
                .try_add(
                    StorageKey::new(*id, key.clone()),
                    StorageItem::from_bytes(value.clone()),
                )
                .expect("seed snapshot change");
        }
        snapshot
    }

    fn empty_snapshot() -> PersistenceDataCache {
        PersistenceDataCache::new(false)
    }

    fn pending_root(handlers: &StateServiceCommitHandlers) -> UInt256 {
        handlers
            .pending_state_root
            .lock()
            .unwrap()
            .as_ref()
            .expect("pending state root")
            .root_hash
    }

    /// Mirrors PersistenceDataCache::clone_cache().commit(): the child changes become
    /// visible in the parent StoreCache before the committed callback runs.
    fn finish_commit(
        handlers: &StateServiceCommitHandlers,
        index: u32,
        snapshot: &PersistenceDataCache,
    ) {
        handlers
            .store_cache
            .data_cache()
            .merge_tracked_items(&snapshot.tracked_items());
        snapshot.commit();
        handlers.persist_pending_state_root(index).unwrap();
    }

    /// Writes everything the handler staged into the backing store, mirroring a
    /// real block commit.
    fn persist_to_store(handlers: &StateServiceCommitHandlers, store: &Arc<dyn Store>) {
        let mut persisted = store.snapshot();
        let writer = Arc::get_mut(&mut persisted).expect("exclusive store snapshot");
        handlers
            .store_cache
            .data_cache()
            .apply_tracked_to_writer(writer)
            .expect("persist tracked state-service entries");
        writer.try_commit().expect("store commit");
    }

    /// Persists the local-commit cursor the way `StateRootStore` does.
    fn stage_local_cursor(handlers: &StateServiceCommitHandlers, index: u32) {
        let key = StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_LOCAL_ROOT_INDEX]);
        let item = StorageItem::from_bytes(u64::from(index).to_be_bytes().to_vec());
        let result = if handlers.store_cache.get(&key).is_some() {
            handlers.store_cache.data_cache().try_update(key, item)
        } else {
            handlers.store_cache.data_cache().try_add(key, item)
        };
        result.expect("stage local root cursor");
    }

    /// Builds a record carrying a witness, i.e. a quorum-validated network root.
    fn network_root_record(index: u32, root_hash: UInt256) -> StateRoot {
        let mut record = StateRoot::new_current(index, root_hash);
        record.witness = Some(neo_protocol::witness::Witness::new_with_scripts(
            vec![0x00],
            vec![0x51],
        ));
        record
    }

    #[test]
    fn backed_block_snapshot_is_authoritative_for_mpt_absence() {
        let mpt_key = b"cold-node";
        let storage_key = TrieNodeStore::namespaced_key(mpt_key);
        let stored_value = vec![1, 2, 3];
        let mut persisted_cache = StoreCache::new(Arc::new(MemoryStore::new()));
        persisted_cache
            .try_add(
                storage_key.clone(),
                StorageItem::from_bytes(stored_value.clone()),
            )
            .unwrap();
        persisted_cache.try_commit().unwrap();
        let persisted = Arc::new(TrieNodeStore {
            store_cache: Arc::new(persisted_cache),
        });

        let empty_backing = StoreCache::new(Arc::new(MemoryStore::new()));
        let backed = BlockTrieNodeStore {
            persisted: Arc::clone(&persisted),
            snapshot: empty_backing.data_cache().clone(),
        };
        assert_eq!(backed.try_get(mpt_key).unwrap(), None);

        let detached = BlockTrieNodeStore {
            persisted,
            snapshot: PersistenceDataCache::new(false),
        };
        assert_eq!(detached.try_get(mpt_key).unwrap(), Some(stored_value));
    }

    #[test]
    fn test_commit_handlers_creation() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));

        let handlers = StateServiceCommitHandlers::new(cache, true);
        assert!(handlers.is_ok());
    }

    #[test]
    fn test_commit_handlers_disabled() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, false).unwrap();

        // When disabled, commit_block should succeed but do nothing
        assert!(handlers.commit_block(0, &empty_snapshot()).is_ok());

        // Pending state root should remain None
        let pending = handlers.pending_state_root.lock().unwrap();
        assert!(pending.is_none());
    }

    #[test]
    fn test_commit_block_computes_real_state_root() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"balance".to_vec())]);
        assert!(handlers.commit_block(100, &snapshot).is_ok());

        // Should have pending state root
        let pending = handlers.pending_state_root.lock().unwrap();
        assert!(pending.is_some());

        let state_root = pending.as_ref().unwrap();
        assert_eq!(state_root.index, 100);
        assert_eq!(
            state_root.version,
            super::super::state_root::CURRENT_VERSION
        );
        // The root is a real MPT hash over the storage changes: non-zero and
        // nothing is persisted before the committed handler runs.
        assert!(!state_root.root_hash.is_zero());
        drop(pending);
        assert!(handlers.get_state_root(100).is_none());
    }

    #[test]
    fn test_state_root_is_deterministic_across_nodes() {
        let changes: &[(i32, Vec<u8>, Vec<u8>)] = &[
            (2, b"alpha".to_vec(), b"one".to_vec()),
            (1, b"shared-prefix-a".to_vec(), b"three".to_vec()),
            (2, b"beta".to_vec(), b"two".to_vec()),
            (1, b"shared-prefix-b".to_vec(), b"four".to_vec()),
        ];
        let mut reversed = changes.to_vec();
        reversed.reverse();

        let handlers_a = StateServiceCommitHandlers::new(
            Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
            true,
        )
        .unwrap();
        let handlers_b = StateServiceCommitHandlers::new(
            Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
            true,
        )
        .unwrap();

        handlers_a
            .commit_block(4, &snapshot_with_changes(changes))
            .unwrap();
        handlers_b
            .commit_block(4, &snapshot_with_changes(&reversed))
            .unwrap();

        // Two independent nodes applying the same changes derive the same root.
        assert_eq!(pending_root(&handlers_a), pending_root(&handlers_b));
    }

    #[test]
    fn ledger_contract_bookkeeping_is_excluded_from_state_root() {
        let contract_changes = &[(1, b"account".to_vec(), b"balance".to_vec())];
        let without_ledger = StateServiceCommitHandlers::new(
            Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
            true,
        )
        .unwrap();
        let with_ledger = StateServiceCommitHandlers::new(
            Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
            true,
        )
        .unwrap();
        without_ledger
            .commit_block(0, &snapshot_with_changes(contract_changes))
            .unwrap();

        let mut changes = contract_changes.to_vec();
        changes.push((-4, vec![0x0c], b"hash-and-index".to_vec()));
        changes.push((-4, vec![0x09, 0, 0, 0, 0], b"block-hash".to_vec()));
        with_ledger
            .commit_block(0, &snapshot_with_changes(&changes))
            .unwrap();

        assert_eq!(pending_root(&without_ledger), pending_root(&with_ledger));
    }

    #[test]
    fn test_consecutive_blocks_produce_different_roots() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let snapshot_a = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
        assert!(handlers.commit_block(1, &snapshot_a).is_ok());
        let root_a = pending_root(&handlers);
        finish_commit(&handlers, 1, &snapshot_a);

        let snapshot_b = snapshot_with_changes(&[(1, b"account".to_vec(), b"v2".to_vec())]);
        assert!(handlers.commit_block(2, &snapshot_b).is_ok());
        let root_b = pending_root(&handlers);
        finish_commit(&handlers, 2, &snapshot_b);

        assert_ne!(root_a, root_b);
        assert_eq!(handlers.current_root_hash(), Some(root_b));
        assert_eq!(handlers.get_state_root(1).unwrap().root_hash, root_a);
        assert_eq!(handlers.get_state_root(2).unwrap().root_hash, root_b);
    }

    #[test]
    fn store_scoped_node_cache_matches_uncached_and_cold_restart_roots() {
        let cached_store = Arc::new(MemoryStore::new());
        let cached =
            StateServiceCommitHandlers::new(Arc::new(StoreCache::new(cached_store.clone())), true)
                .unwrap();
        let uncached = StateServiceCommitHandlers::new(
            Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
            true,
        )
        .unwrap();
        assert!(uncached.node_cache_stats().enabled);
        uncached.set_node_cache_enabled(false);
        assert!(!uncached.node_cache_stats().enabled);

        for (height, value) in [
            (1u32, b"one".as_slice()),
            (2, b"two".as_slice()),
            (3, b"three".as_slice()),
        ] {
            let changes = [
                (1, b"account".to_vec(), value.to_vec()),
                (2, b"other".to_vec(), value.to_vec()),
            ];
            let cached_snapshot = snapshot_with_changes(&changes);
            let uncached_snapshot = snapshot_with_changes(&changes);
            cached.commit_block(height, &cached_snapshot).unwrap();
            uncached.commit_block(height, &uncached_snapshot).unwrap();
            assert_eq!(pending_root(&cached), pending_root(&uncached));
            finish_commit(&cached, height, &cached_snapshot);
            finish_commit(&uncached, height, &uncached_snapshot);
        }

        let node_cache = cached.node_cache.lock().unwrap().clone().unwrap();
        // Warm blocks reuse the committed root directly and need not consult
        // this fallback cache. The recovered handler below must still be cold.
        assert!(node_cache.len() <= STATE_ROOT_NODE_CACHE_CAPACITY);
        let uncached_stats = uncached.node_cache_stats();
        assert_eq!((uncached_stats.hits, uncached_stats.misses), (0, 0));
        assert_eq!(uncached_stats.entries, 0);

        let mut persisted = cached_store.snapshot();
        let writer = Arc::get_mut(&mut persisted).expect("new memory-store snapshot");
        cached
            .store_cache
            .data_cache()
            .apply_tracked_to_writer(writer)
            .unwrap();
        writer.try_commit().unwrap();
        let recovered =
            StateServiceCommitHandlers::new(Arc::new(StoreCache::new(cached_store)), true).unwrap();
        assert!(
            recovered
                .node_cache
                .lock()
                .unwrap()
                .clone()
                .unwrap()
                .is_empty()
        );
        assert_eq!(recovered.recover_from_storage().unwrap(), Some(3));

        let changes = [(1, b"account".to_vec(), b"four".to_vec())];
        let cached_snapshot = snapshot_with_changes(&changes);
        let recovered_snapshot = snapshot_with_changes(&changes);
        cached.commit_block(4, &cached_snapshot).unwrap();
        recovered.commit_block(4, &recovered_snapshot).unwrap();
        assert_eq!(pending_root(&cached), pending_root(&recovered));
        let (_, misses, _) = recovered
            .node_cache
            .lock()
            .unwrap()
            .clone()
            .unwrap()
            .stats();
        assert!(misses > 0, "a restart must begin with a cold cache");
    }

    #[test]
    fn test_empty_block_without_prior_root_is_rejected() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // An empty state trie has no representable root: fail closed.
        let result = handlers.commit_block(1, &empty_snapshot());
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("empty"));
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
    }

    #[test]
    fn test_empty_block_after_prior_root_keeps_previous_root() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
        assert!(handlers.commit_block(1, &snapshot).is_ok());
        finish_commit(&handlers, 1, &snapshot);
        let root = handlers.current_root_hash().unwrap();

        // A block with no storage changes keeps the state (and root) unchanged.
        assert!(handlers.commit_block(2, &empty_snapshot()).is_ok());
        assert_eq!(pending_root(&handlers), root);
    }

    #[test]
    fn test_deletions_change_the_state_root() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let key_x = StorageKey::new(3, b"key-x".to_vec());
        let key_y = StorageKey::new(3, b"key-y".to_vec());

        let snapshot_one = snapshot_with_changes(&[
            (3, b"key-x".to_vec(), b"v1".to_vec()),
            (3, b"key-y".to_vec(), b"v2".to_vec()),
        ]);
        handlers.commit_block(1, &snapshot_one).unwrap();
        finish_commit(&handlers, 1, &snapshot_one);
        let root_one = handlers.current_root_hash().unwrap();

        // Block two deletes key-x (tracked change with no value).
        let snapshot_two = PersistenceDataCache::new(false);
        snapshot_two
            .try_add(key_x.clone(), StorageItem::from_bytes(b"v1".to_vec()))
            .unwrap();
        snapshot_two
            .try_add(key_y.clone(), StorageItem::from_bytes(b"v2".to_vec()))
            .unwrap();
        // Establish the entries as the snapshot's committed baseline. Adding
        // and deleting in the same uncommitted change set cancels the entry,
        // whereas deleting this baseline records TrackState::Deleted.
        snapshot_two.commit();
        snapshot_two.try_delete(&key_x).unwrap();

        handlers.commit_block(2, &snapshot_two).unwrap();
        let root_two = pending_root(&handlers);
        assert_ne!(root_one, root_two);
        finish_commit(&handlers, 2, &snapshot_two);

        // The surviving key is still readable (and the deleted key is gone)
        // under the new root through the persisted node store.
        let mut trie =
            Trie::new_with_node_cache(handlers.node_store.clone(), Some(root_two), true, None);
        assert_eq!(trie.get(&key_y.to_array()).unwrap(), Some(b"v2".to_vec()));
        assert_eq!(trie.get(&key_x.to_array()).unwrap(), None);
    }

    #[test]
    fn test_rollback_clears_pending() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // Commit and verify pending state root exists
        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
        assert!(handlers.commit_block(50, &snapshot).is_ok());
        assert!(handlers.pending_state_root.lock().unwrap().is_some());

        // Rollback should clear it
        assert!(handlers.rollback().is_ok());
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
        assert!(handlers.pending_root_node.lock().unwrap().is_none());
    }

    #[test]
    fn warm_root_matches_cold_reload_and_rollback_preserves_committed_baseline() {
        let warm = StateServiceCommitHandlers::new(
            Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
            true,
        )
        .unwrap();
        let cold = StateServiceCommitHandlers::new(
            Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
            true,
        )
        .unwrap();

        let first = [(1, b"account".to_vec(), b"v1".to_vec())];
        let warm_snapshot = snapshot_with_changes(&first);
        let cold_snapshot = snapshot_with_changes(&first);
        warm.commit_block(1, &warm_snapshot).unwrap();
        cold.commit_block(1, &cold_snapshot).unwrap();
        assert_eq!(pending_root(&warm), pending_root(&cold));
        finish_commit(&warm, 1, &warm_snapshot);
        finish_commit(&cold, 1, &cold_snapshot);
        let committed_root = warm.current_root_hash();
        *cold.current_root_node.lock().unwrap() = None;

        // This candidate mutates the shared path but is never made durable.
        let aborted = [(1, b"account".to_vec(), b"aborted".to_vec())];
        let aborted_snapshot = snapshot_with_changes(&aborted);
        warm.commit_block(2, &aborted_snapshot).unwrap();
        warm.rollback().unwrap();
        assert_eq!(warm.current_root_hash(), committed_root);

        let second = [
            (1, b"account".to_vec(), b"v2".to_vec()),
            (2, b"other".to_vec(), b"v2".to_vec()),
        ];
        let warm_snapshot = snapshot_with_changes(&second);
        let cold_snapshot = snapshot_with_changes(&second);
        warm.commit_block(2, &warm_snapshot).unwrap();
        cold.commit_block(2, &cold_snapshot).unwrap();
        assert_eq!(pending_root(&warm), pending_root(&cold));
        finish_commit(&warm, 2, &warm_snapshot);
        finish_commit(&cold, 2, &cold_snapshot);
        *cold.current_root_node.lock().unwrap() = None;

        let third = [
            (1, b"account".to_vec(), b"v3".to_vec()),
            (2, b"other".to_vec(), b"v3".to_vec()),
        ];
        let warm_snapshot = snapshot_with_changes(&third);
        let cold_snapshot = snapshot_with_changes(&third);
        warm.commit_block(3, &warm_snapshot).unwrap();
        cold.commit_block(3, &cold_snapshot).unwrap();
        assert_eq!(pending_root(&warm), pending_root(&cold));
    }

    #[test]
    fn test_persist_pending_state_root() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
        assert!(handlers.commit_block(200, &snapshot).is_ok());
        let expected_root = pending_root(&handlers);

        handlers
            .store_cache
            .data_cache()
            .merge_tracked_items(&snapshot.tracked_items());
        snapshot.commit();

        // The committed callback consumes the pending root; the block snapshot
        // has already made the record visible through the parent cache.
        assert!(matches!(
            handlers.persist_pending_state_root(200),
            Ok(Some(ref persisted)) if persisted.root_hash == expected_root
        ));
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
        assert_eq!(handlers.current_root_hash(), Some(expected_root));

        let record = handlers.get_state_root(200).expect("persisted record");
        assert_eq!(record.index, 200);
        assert_eq!(record.root_hash, expected_root);
        assert!(record.witness.is_none());
    }

    #[test]
    fn test_persist_index_mismatch() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
        assert!(handlers.commit_block(300, &snapshot).is_ok());

        // Try to finalize with the wrong index without committing the snapshot.
        assert!(handlers.persist_pending_state_root(301).unwrap().is_none());
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
        assert!(handlers.get_state_root(300).is_none());
        assert!(handlers.get_state_root(301).is_none());
    }

    #[test]
    fn test_state_root_supports_mpt_proofs() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let storage_key = StorageKey::new(7, b"proof-key".to_vec());
        let snapshot =
            snapshot_with_changes(&[(7, b"proof-key".to_vec(), b"proof-value".to_vec())]);
        assert!(handlers.commit_block(10, &snapshot).is_ok());
        let root = pending_root(&handlers);
        finish_commit(&handlers, 10, &snapshot);

        // Generate an inclusion proof from the persisted node store...
        let mut trie =
            Trie::new_with_node_cache(handlers.node_store.clone(), Some(root), true, None);
        let proof_set = trie
            .try_get_proof(&storage_key.to_array())
            .expect("generate proof")
            .expect("key present in trie");
        assert!(!proof_set.is_empty());

        // ...and validate it: the proof resolves the value under the root.
        assert_eq!(
            Trie::<TrieNodeStore>::verify_proof(root, &storage_key.to_array(), &proof_set)
                .expect("verify proof"),
            b"proof-value".to_vec()
        );

        // The same proof must not verify for a key that was never written.
        let missing = StorageKey::new(8, b"missing".to_vec());
        assert!(
            Trie::<TrieNodeStore>::verify_proof(root, &missing.to_array(), &proof_set).is_err()
        );
    }

    #[test]
    fn test_committed_root_proof_cache_survives_head_advance() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let storage_key = StorageKey::new(7, b"cached-proof-key".to_vec());
        let first_snapshot =
            snapshot_with_changes(&[(7, b"cached-proof-key".to_vec(), b"value-1".to_vec())]);
        assert!(handlers.commit_block(11, &first_snapshot).is_ok());
        let first_root = pending_root(&handlers);
        finish_commit(&handlers, 11, &first_snapshot);

        // A second block advances the head; the first root stays provable.
        let second_snapshot =
            snapshot_with_changes(&[(7, b"cached-proof-key".to_vec(), b"value-2".to_vec())]);
        assert!(handlers.commit_block(12, &second_snapshot).is_ok());
        let second_root = pending_root(&handlers);
        finish_commit(&handlers, 12, &second_snapshot);
        assert_ne!(first_root, second_root);
        assert_eq!(handlers.current_root_hash(), Some(second_root));

        // Proofs are cached per committed root, so history keeps working while
        // the current root moves on.
        let proof_cache = ProofCache::with_defaults();
        let namespace = ProofCacheNamespace::new(u64::from(STATE_ROOT_STORAGE_ID.unsigned_abs()));
        let mut trie =
            Trie::new_with_node_cache(handlers.node_store.clone(), Some(first_root), true, None);
        let committed = trie
            .committed_root()
            .expect("resolved root node is immutable");
        assert_eq!(committed.hash(), first_root);

        let key_bytes = storage_key.to_array();
        let first = trie
            .get_proof_cached(&proof_cache, committed, namespace, &key_bytes)
            .expect("cached proof")
            .expect("key present");
        let replay = trie
            .get_proof_cached(&proof_cache, committed, namespace, &key_bytes)
            .expect("cached proof replay")
            .expect("key present");
        assert!(Arc::ptr_eq(&first, &replay));
        assert_eq!(proof_cache.stats().hits, 1);
        assert_eq!(proof_cache.stats().constructions, 1);
        assert_eq!(
            Trie::<TrieNodeStore>::verify_proof(first_root, &key_bytes, &first.to_hash_set())
                .expect("verify cached proof"),
            b"value-1".to_vec()
        );

        // The cache is keyed by the committed root and does not follow the
        // head, so the entry stays valid after the head advanced.
        assert_eq!(handlers.current_root_hash(), Some(second_root));
        assert_eq!(proof_cache.len(), 1);
    }

    #[test]
    fn test_recover_from_storage_seeds_baseline() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let handlers =
            StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store.clone())), true)
                .unwrap();

        // Commit two real blocks: root records, trie nodes and the local cursor
        // are all persisted by the commit path.
        let mut committed_root = None;
        for (height, value) in [(3u32, b"three".as_slice()), (5, b"five".as_slice())] {
            let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), value.to_vec())]);
            handlers.commit_block(height, &snapshot).unwrap();
            committed_root = Some(pending_root(&handlers));
            finish_commit(&handlers, height, &snapshot);
        }
        stage_local_cursor(&handlers, 5);
        persist_to_store(&handlers, &store);
        let root = committed_root.expect("committed root");

        // A fresh handler over a fresh cache recovers the locally committed root.
        let cache = Arc::new(StoreCache::new(store));
        let recovered = StateServiceCommitHandlers::new(cache, true).unwrap();
        assert_eq!(recovered.current_root_hash(), None);
        assert_eq!(recovered.recover_from_storage().unwrap(), Some(5));
        assert_eq!(recovered.current_root_hash(), Some(root));
        assert_eq!(recovered.get_state_root(5).unwrap().root_hash, root);
        assert!(recovered.get_state_root(4).is_none());

        // The recovered baseline extends the hydrated trie, so the next block
        // derives the same root as the original handler would.
        let changes = [(1, b"account".to_vec(), b"six".to_vec())];
        handlers
            .commit_block(6, &snapshot_with_changes(&changes))
            .unwrap();
        recovered
            .commit_block(6, &snapshot_with_changes(&changes))
            .unwrap();
        assert_eq!(pending_root(&handlers), pending_root(&recovered));
    }

    #[test]
    fn recovery_never_adopts_a_network_root_above_the_local_cursor() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let handlers =
            StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store.clone())), true)
                .unwrap();

        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"local".to_vec())]);
        handlers.commit_block(7, &snapshot).unwrap();
        let local_root = pending_root(&handlers);
        finish_commit(&handlers, 7, &snapshot);
        stage_local_cursor(&handlers, 7);

        // A quorum-validated root was persisted for a height whose block does
        // not exist locally yet. It must not become the trie baseline.
        let network = network_root_record(42, UInt256::from([0xAB; 32]));
        handlers
            .store_cache
            .data_cache()
            .try_add(
                state_root_record_key(42),
                StorageItem::from_bytes(serialize_state_root_record(&network)),
            )
            .unwrap();
        persist_to_store(&handlers, &store);

        let recovered =
            StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store)), true).unwrap();
        assert_eq!(recovered.recover_from_storage().unwrap(), Some(7));
        assert_eq!(recovered.current_root_hash(), Some(local_root));
    }

    #[test]
    fn recovery_without_a_cursor_ignores_network_only_records() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let mut writer = StoreCache::new(store.clone());
        let network = network_root_record(9, UInt256::from([0xCD; 32]));
        writer
            .data_cache()
            .try_add(
                state_root_record_key(9),
                StorageItem::from_bytes(serialize_state_root_record(&network)),
            )
            .unwrap();
        writer.commit();

        // No local cursor and no unsigned record: this node has no locally
        // committed state, so recovery must start from an empty trie.
        let handlers =
            StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store)), true).unwrap();
        assert_eq!(handlers.recover_from_storage().unwrap(), None);
        assert_eq!(handlers.current_root_hash(), None);
    }

    #[test]
    fn recovery_refuses_a_cursor_whose_trie_node_is_missing() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let mut writer = StoreCache::new(store.clone());
        let root = UInt256::from([0x5A; 32]);
        writer
            .data_cache()
            .try_add(
                state_root_record_key(11),
                StorageItem::from_bytes(serialize_state_root_record(&StateRoot::new_current(
                    11, root,
                ))),
            )
            .unwrap();
        writer
            .data_cache()
            .try_add(
                StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_LOCAL_ROOT_INDEX]),
                StorageItem::from_bytes(11u64.to_be_bytes().to_vec()),
            )
            .unwrap();
        writer.commit();

        let handlers =
            StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store)), true).unwrap();
        let error = handlers
            .recover_from_storage()
            .expect_err("a root without its trie must not be adopted");
        assert!(
            error.message.contains("missing from local storage"),
            "unexpected error: {}",
            error.message
        );
        assert_eq!(handlers.current_root_hash(), None);
    }

    #[test]
    fn test_recover_from_storage_without_records() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        assert_eq!(handlers.recover_from_storage().unwrap(), None);
        assert_eq!(handlers.current_root_hash(), None);
    }

    #[test]
    fn test_state_service_storage_keys_are_namespaced() {
        let key = state_root_record_key(42);
        assert_eq!(key.id, STATE_ROOT_STORAGE_ID);
        assert_eq!(key.key(), &[PREFIX_STATE_ROOT, 0, 0, 0, 42][..]);

        // The dedicated id cannot collide with native contracts (small
        // negative ids) or deployed contracts (positive ids).
        assert!(STATE_ROOT_STORAGE_ID < -100);

        // Root-record and trie-node keys share the id but differ in prefix.
        let node_key = TrieNodeStore::namespaced_key(&[0u8; 33]);
        assert_eq!(node_key.id, STATE_ROOT_STORAGE_ID);
        assert_eq!(node_key.key()[0], PREFIX_TRIE_NODE);
        assert_ne!(node_key.key()[0], PREFIX_STATE_ROOT);
    }

    #[test]
    fn test_state_root_record_serialization_round_trip() {
        let root_hash = UInt256::from([7u8; 32]);

        // Unsigned record.
        let state_root = StateRoot::new_current(12, root_hash);
        let bytes = serialize_state_root_record(&state_root);
        assert_eq!(bytes.len(), 32 + 1 + 1);
        let decoded = deserialize_state_root_record(12, &bytes).unwrap();
        assert_eq!(decoded.index, 12);
        assert_eq!(decoded.version, super::super::state_root::CURRENT_VERSION);
        assert_eq!(decoded.root_hash, root_hash);
        assert!(decoded.witness.is_none());

        // Signed record.
        let invocation = vec![0x01u8, 0x02, 0x03];
        let verification = vec![0x04u8, 0x05];
        let mut signed = StateRoot::new_current(13, root_hash);
        signed.witness = Some(neo_protocol::witness::Witness::new_with_scripts(
            invocation.clone(),
            verification.clone(),
        ));
        let bytes = serialize_state_root_record(&signed);
        let decoded = deserialize_state_root_record(13, &bytes).unwrap();
        let witness = decoded.witness.expect("witness round trip");
        assert_eq!(witness.invocation_script(), invocation);
        assert_eq!(witness.verification_script(), verification);

        // Malformed inputs are rejected (fail-closed).
        assert!(deserialize_state_root_record(1, &[]).is_none());
        assert!(deserialize_state_root_record(1, &bytes[..40]).is_none());
    }

    #[test]
    fn test_committing_handler() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store.clone()));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        // Create minimal block
        let header = BlockHeader::default();
        let mut block = Block::new(header, vec![]);
        block.header.index = 42;

        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
        let app_executed: Vec<ApplicationExecuted> = vec![];

        // Call committing hook (no runtime context: direct-embedder fallback)
        handlers.blockchain_committing_handler(None, &block, &snapshot, &app_executed);

        // Should have computed state root
        let pending = handlers.pending_state_root.lock().unwrap();
        assert!(pending.is_some());
        assert_eq!(pending.as_ref().unwrap().index, 42);
    }

    #[test]
    fn test_committed_handler_persists_and_announces() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));
        let captured: Arc<Mutex<Vec<ValidatedRootPersisted>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_capture = captured.clone();
        let handlers = StateServiceCommitHandlers::new(cache, true)
            .unwrap()
            .with_event_sink(Arc::new(move |event| {
                sink_capture.lock().unwrap().push(event);
            }));

        // First compute state root
        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
        assert!(handlers.commit_block(99, &snapshot).is_ok());
        let expected_root = pending_root(&handlers);

        // Create minimal block
        let header = BlockHeader::default();
        let mut block = Block::new(header, vec![]);
        block.header.index = 99;

        // Commit the child snapshot into its parent before the committed callback.
        handlers
            .store_cache
            .data_cache()
            .merge_tracked_items(&snapshot.tracked_items());
        snapshot.commit();

        handlers.blockchain_committed_handler(None, &block);

        // Should have persisted (cleared pending), stored the record and
        // announced the ValidatedRootPersisted event through the sink.
        assert!(handlers.pending_state_root.lock().unwrap().is_none());
        assert_eq!(handlers.current_root_hash(), Some(expected_root));
        assert_eq!(
            handlers.get_state_root(99).unwrap().root_hash,
            expected_root
        );

        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].index, 99);
    }

    #[test]
    fn test_run_during_fast_sync() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store));

        let enabled_handlers = StateServiceCommitHandlers::new(Arc::clone(&cache), true).unwrap();
        let disabled_handlers = StateServiceCommitHandlers::new(cache, false).unwrap();

        assert!(enabled_handlers.run_during_fast_sync());
        assert!(!disabled_handlers.run_during_fast_sync());
    }

    #[test]
    fn test_try_committing_handler() {
        let store = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store.clone()));
        let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

        let header = BlockHeader::default();
        let mut block = Block::new(header, vec![]);
        block.header.index = 77;

        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
        let app_executed: Vec<ApplicationExecuted> = vec![];

        // Call fallible version
        let result =
            handlers.try_blockchain_committing_handler(None, &block, &snapshot, &app_executed);

        assert!(result.is_ok());

        // Should have computed state root
        let pending = handlers.pending_state_root.lock().unwrap();
        assert!(pending.is_some());
        assert_eq!(pending.as_ref().unwrap().index, 77);
    }
}
