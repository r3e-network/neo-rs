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

mod blockchain;
mod handlers;
mod recovery;
#[cfg(test)]
mod tests;
mod trie_stores;

pub use trie_stores::StateRootNodeCacheStats;
pub(crate) use trie_stores::TrieNodeStore;

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
