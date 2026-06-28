//! [`MptStore`] - persisted MPT-node + local-state-root storage for the
//! state service.
//!
//! Mirrors the storage layer of the C# `StateService` plugin
//! (`Storage/StateStore.cs` + `Storage/StateSnapshot.cs` +
//! `Storage/Keys.cs`, vendored under
//! `neo_csharp/src/Plugins/StateService`): one flat key/value namespace
//! holds
//!
//! - the MPT nodes, written by the [`Trie`] write-through cache under
//!   `0xf0 || node_hash` (the prefix is applied inside
//!   `neo_crypto::mpt_trie`, matching the C# `Cache` prefix);
//! - the per-block state-root records under `0x01 || index_be`
//!   ([`Keys::state_root`]), serialized in the C# `StateRoot` wire
//!   format (unsigned fields + a var-int witness count);
//! - the current local root index under `0x02`
//!   ([`Keys::CURRENT_LOCAL_ROOT_INDEX`]), little-endian `u32` exactly
//!   like the C# `BitConverter.GetBytes(uint)` write.
//!
//! The C# plugin opens a LevelDB store at `Data_MPT_{network}`; this
//! build keeps the namespace in process memory (the rest of
//! `neo-state-service` is in-memory too). The byte layout is the C#
//! one, so pointing the namespace at a disk-backed store later is a
//! storage swap, not a format migration.
//!
//! Block-changeset application mirrors the C# commit pipeline:
//! `Blockchain.Committing` calls `UpdateLocalStateRootSnapshot(height,
//! changeSet)` (Put for Added/Changed, Delete for Deleted, then
//! `Trie.Root.Hash` becomes the block's state root) and
//! `Blockchain.Committed` calls `UpdateLocalStateRoot(height)` (trie +
//! snapshot commit). [`MptStore::apply_block_changes`] performs both
//! halves in one synchronous call, which is the seam the node's
//! persist pipeline is expected to drive.
//!
//! # Reader snapshot isolation
//!
//! C# readers never walk the live store: every RPC handler opens an
//! immutable LevelDB snapshot first (`StateStore.GetStoreSnapshot()`)
//! and builds the `Trie` over it, so a concurrent block commit (which,
//! without `FullState`, *prunes* superseded nodes) can never delete a
//! node out from under an in-flight read. This port reproduces that
//! with a copy-on-write generation map:
//!
//! - the key/value namespace is an `Arc<BTreeMap>` behind an `RwLock`;
//! - [`MptStore::snapshot`] clones the `Arc` under the read lock,
//!   yielding an [`MptReadSnapshot`] — a frozen, point-in-time view
//!   (the `GetStoreSnapshot` analogue) every read trie resolves from;
//! - the single writer ([`MptStore::apply_block_changes`]) stages all
//!   trie-node and state-root mutations in a private write batch and
//!   publishes them in one `Arc::make_mut` critical section: when no
//!   reader holds the previous generation the map is updated in place,
//!   otherwise it is cloned once and the old generation stays alive
//!   (and fully resolvable) until the last reader drops it.

use crate::Keys;
use crate::metrics::{StateRootApplyCountKind, StateRootApplyMetrics, StateRootApplyStage};
use crate::state_root::StateRoot;
use neo_crypto::mpt_trie::{MptError, MptResult, MptStoreSnapshot, Trie};
use neo_primitives::{UINT256_SIZE, UInt256};
use neo_storage::persistence::{SeekDirection, Store};
use parking_lot::{Mutex, RwLock};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

/// Size of the serialized unsigned `StateRoot` prefix:
/// `version (1) + index (4, LE) + root_hash (32)`.
const STATE_ROOT_UNSIGNED_LEN: usize = 1 + 4 + UINT256_SIZE;

/// One storage mutation from a block's change set.
///
/// Mirrors the `TrackState` cases the C# `StateStore.
/// UpdateLocalStateRootSnapshot` consumes: `Added` / `Changed` both
/// become a trie `Put`, `Deleted` becomes a trie `Delete`. Use
/// [`crate::StateStore::apply_snapshot_changes`] for the canonical
/// block-snapshot projection that filters `None` entries and Ledger
/// native-contract records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MptChange {
    /// Insert or update the value stored under `key` (the full
    /// storage-key bytes: little-endian `i32` contract id + key
    /// suffix, i.e. C# `StorageKey.ToArray()`).
    Put {
        /// Full storage-key bytes.
        key: Vec<u8>,
        /// Raw storage-item value bytes (C# `StorageItem.ToArray()`).
        value: Vec<u8>,
    },
    /// Remove the entry stored under `key`.
    Delete {
        /// Full storage-key bytes.
        key: Vec<u8>,
    },
}

/// Persisted MPT-node + state-root store for the state service.
///
/// Thread-safe: readers take a point-in-time [`MptReadSnapshot`] (see
/// the module docs for the isolation design), trie commits and
/// state-root writes go through [`MptStore::apply_block_changes`],
/// which serializes writers behind a dedicated gate (the C# plugin
/// achieves the same single-writer discipline by running `StateStore`
/// as an actor).
pub struct MptStore {
    /// Flat key/value namespace shared by MPT nodes and state-root
    /// records (the C# `IStore` equivalent). The `Arc` is the
    /// copy-on-write generation pointer: readers clone it to freeze a
    /// view, the writer republishes it atomically per applied block.
    kv: RwLock<Arc<BTreeMap<Vec<u8>, Vec<u8>>>>,
    /// Serializes block-changeset application.
    write_gate: Mutex<()>,
    /// Whether historical trie nodes are retained (C#
    /// `StateServiceSettings.FullState`, default `false`). With
    /// `false`, applying a block prunes the nodes the change set made
    /// unreachable, so only the *current* root stays resolvable.
    full_state: bool,
    /// Optional durable backend for the same flat C# byte namespace.
    backing: Option<Arc<dyn Store>>,
}

/// Immutable, point-in-time view of an [`MptStore`] — the analogue of
/// the C# `StateStore.GetStoreSnapshot()` LevelDB snapshot every read
/// path opens before walking the trie.
///
/// A snapshot observes the key/value namespace exactly as it was when
/// [`MptStore::snapshot`] was called: blocks applied afterwards
/// (including pruning-mode node deletion) are invisible, so a trie
/// walk over the snapshot can never lose nodes mid-traversal. The
/// trie-node map and the state-root records are captured together,
/// making `current_local_root_*` + `open_trie` reads mutually
/// consistent.
pub struct MptReadSnapshot {
    /// Frozen generation of the key/value namespace.
    map: Arc<BTreeMap<Vec<u8>, Vec<u8>>>,
    /// Copied [`MptStore::full_state`] flag.
    full_state: bool,
}

impl MptReadSnapshot {
    /// Returns whether historical trie versions are retained.
    pub fn full_state(&self) -> bool {
        self.full_state
    }

    /// Opens a read-only trie rooted at `root` (`None` for the empty
    /// trie) over this frozen view. Mutating the returned [`Trie`] is
    /// rejected at commit time (the snapshot's store surface is
    /// read-only).
    pub fn open_trie(self: &Arc<Self>, root: Option<UInt256>) -> Trie<Self> {
        Trie::new(Arc::clone(self), root, self.full_state)
    }

    /// Returns the state-root record persisted for `index`, if any,
    /// as of this snapshot.
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        MptStore::read_state_root(&self.map, index)
    }

    /// Returns the local root index current as of this snapshot (C#
    /// `StateSnapshot.CurrentLocalRootIndex`).
    pub fn current_local_root_index(&self) -> Option<u32> {
        MptStore::read_current_local_root_index(&self.map)
    }

    /// Returns the local root hash current as of this snapshot (C#
    /// `StateSnapshot.CurrentLocalRootHash`).
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        let index = self.current_local_root_index()?;
        Some(*self.get_state_root(index)?.root_hash())
    }
}

impl std::fmt::Debug for MptReadSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MptReadSnapshot")
            .field("entries", &self.map.len())
            .field("full_state", &self.full_state)
            .finish()
    }
}

impl MptStoreSnapshot for MptReadSnapshot {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self.map.get(key).cloned())
    }

    fn put(&self, _key: Vec<u8>, _value: Vec<u8>) -> MptResult<()> {
        Err(MptError::invalid(
            "cannot write through a read-only MPT store snapshot",
        ))
    }

    fn delete(&self, _key: Vec<u8>) -> MptResult<()> {
        Err(MptError::invalid(
            "cannot write through a read-only MPT store snapshot",
        ))
    }
}

/// Private write-staging view used by [`MptStore::apply_block_changes`]:
/// reads come from the frozen base generation (the writer is the only
/// mutator, so base == live for its whole run), writes are buffered in
/// an overlay (`None` = staged deletion) and published into the live
/// map in a single atomic step after the trie commit succeeds.
struct MptWriteBatch {
    /// Generation the block builds on.
    base: Arc<BTreeMap<Vec<u8>, Vec<u8>>>,
    /// Staged mutations: `Some(value)` = put, `None` = delete.
    overlay: Mutex<BTreeMap<Vec<u8>, Option<Vec<u8>>>>,
}

impl MptStoreSnapshot for MptWriteBatch {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        if let Some(staged) = self.overlay.lock().get(key) {
            return Ok(staged.clone());
        }
        Ok(self.base.get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.overlay.lock().insert(key, Some(value));
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.overlay.lock().insert(key, None);
        Ok(())
    }
}

impl MptStore {
    /// Constructs an empty store.
    ///
    /// `full_state` mirrors the C# `FullState` setting: `true` keeps
    /// every historical trie version resolvable (required for
    /// `getstate`/`getproof` against old roots), `false` prunes
    /// superseded nodes on each block.
    pub fn new(full_state: bool) -> Self {
        Self {
            kv: RwLock::new(Arc::new(BTreeMap::new())),
            write_gate: Mutex::new(()),
            full_state,
            backing: None,
        }
    }

    /// Opens a store over an existing durable byte namespace.
    ///
    /// The backing store is read into the in-process generation map on
    /// construction, and every later block overlay is committed through a
    /// backing snapshot before the live generation advances.
    pub fn from_store(backing: Arc<dyn Store>, full_state: bool) -> MptResult<Self> {
        let snapshot = backing.snapshot();
        let map = snapshot.find(None, SeekDirection::Forward).collect();
        Ok(Self {
            kv: RwLock::new(Arc::new(map)),
            write_gate: Mutex::new(()),
            full_state,
            backing: Some(backing),
        })
    }

    /// Returns whether historical trie versions are retained.
    pub fn full_state(&self) -> bool {
        self.full_state
    }

    /// Captures an immutable, point-in-time view of the store (the C#
    /// `GetStoreSnapshot()` analogue). All read paths that walk the
    /// trie must go through a snapshot so a concurrent
    /// [`MptStore::apply_block_changes`] (which prunes superseded
    /// nodes without `full_state`) cannot delete nodes out from under
    /// the walk.
    pub fn snapshot(&self) -> Arc<MptReadSnapshot> {
        Arc::new(MptReadSnapshot {
            map: Arc::clone(&self.kv.read()),
            full_state: self.full_state,
        })
    }

    /// Opens a read view of the trie rooted at `root` (`None` for the
    /// empty trie) over a fresh [`MptReadSnapshot`]. The returned
    /// [`Trie`] resolves nodes from the frozen view; read operations
    /// never write back.
    ///
    /// Callers that need the current root *and* the trie to agree
    /// (e.g. RPC handlers) should take one [`MptStore::snapshot`] and
    /// read both from it instead.
    pub fn open_trie(&self, root: Option<UInt256>) -> Trie<MptReadSnapshot> {
        self.snapshot().open_trie(root)
    }

    /// Applies one block's storage change set on top of `root_before`
    /// and returns the new state root.
    ///
    /// Mirrors the C# `StateStore.UpdateLocalStateRootSnapshot(height,
    /// changeSet)` + `UpdateLocalStateRoot(height)` pair:
    ///
    /// 1. every change is applied to a trie opened at `root_before`
    ///    (`Put` for Added/Changed, `Delete` for Deleted);
    /// 2. the new root hash is computed (`Trie.Root.Hash`);
    /// 3. the trie nodes are committed to the store;
    /// 4. the block's `StateRoot` record is written under
    ///    `Keys::state_root(block_index)` and the current local root
    ///    index is advanced (`Keys::CURRENT_LOCAL_ROOT_INDEX`).
    ///
    /// [`crate::StateStore::apply_snapshot_changes`] is the normal node path:
    /// it performs the C# `Blockchain_Committing_Handler` filtering before
    /// calling this lower-level method. Tests and replayers may call this
    /// directly when they already have a projected change set.
    ///
    /// `root_before` must be the root produced by the previous block
    /// (or `None` before the first block); it is taken explicitly so
    /// replays and tests can branch from arbitrary roots.
    pub fn apply_block_changes(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        changes: &[MptChange],
    ) -> MptResult<UInt256> {
        let _writer = self.write_gate.lock();
        StateRootApplyMetrics::record_count(StateRootApplyCountKind::Changes, changes.len() as u64);

        // Stage every mutation against the current generation. The
        // writer gate guarantees the base cannot change underneath us.
        let batch = Arc::new(MptWriteBatch {
            base: Arc::clone(&self.kv.read()),
            overlay: Mutex::new(BTreeMap::new()),
        });

        let mut trie = Trie::new(Arc::clone(&batch), root_before, self.full_state);
        let stage_start = Instant::now();
        for change in changes {
            match change {
                MptChange::Put { key, value } => trie.put(key, value)?,
                MptChange::Delete { key } => {
                    // C# ignores the `Trie.Delete` return value: deleting
                    // a key that is already absent is a no-op.
                    let _ = trie.delete(key)?;
                }
            }
        }
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::MutateChanges,
            elapsed_us(stage_start),
        );

        // C# reads `Trie.Root.Hash` (well-defined even for the empty
        // root, which hashes its single sentinel byte).
        let stage_start = Instant::now();
        let new_root = trie.root().try_hash()?;
        StateRootApplyMetrics::record_stage(StateRootApplyStage::RootHash, elapsed_us(stage_start));
        let stage_start = Instant::now();
        trie.commit()?;
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::TrieCommit,
            elapsed_us(stage_start),
        );
        drop(trie);

        // The local (unwitnessed) state-root record and the current
        // local root index advance in the same published generation
        // (C# `StateSnapshot.AddLocalStateRoot` + snapshot commit), so
        // a reader can never observe the new root record without the
        // trie nodes that back it.
        let stage_start = Instant::now();
        let root_record = StateRoot::new_current(block_index, new_root);
        let overlay = {
            let mut overlay = batch.overlay.lock();
            overlay.insert(
                Keys::state_root(block_index),
                Some(Self::encode_state_root(&root_record)),
            );
            overlay.insert(
                Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                Some(block_index.to_le_bytes().to_vec()),
            );
            std::mem::take(&mut *overlay)
        };
        let overlay_entries = overlay.len() as u64;
        let overlay_puts = overlay.values().filter(|value| value.is_some()).count() as u64;
        let overlay_deletes = overlay_entries.saturating_sub(overlay_puts);
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::OverlayEntries,
            overlay_entries,
        );
        StateRootApplyMetrics::record_count(StateRootApplyCountKind::OverlayPuts, overlay_puts);
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::OverlayDeletes,
            overlay_deletes,
        );
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::OverlayPrepare,
            elapsed_us(stage_start),
        );
        // Release the batch's base Arc before publishing so that, with
        // no reader snapshots outstanding, `make_mut` updates in place
        // instead of cloning the map.
        drop(batch);

        let stage_start = Instant::now();
        let backing_result = self.commit_overlay_to_backing(&overlay);
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::BackingCommit,
            elapsed_us(stage_start),
        );
        backing_result?;

        let stage_start = Instant::now();
        let mut kv = self.kv.write();
        let map = Arc::make_mut(&mut *kv);
        for (key, value) in overlay {
            match value {
                Some(value) => {
                    map.insert(key, value);
                }
                None => {
                    map.remove(&key);
                }
            }
        }
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::PublishGeneration,
            elapsed_us(stage_start),
        );
        Ok(new_root)
    }

    fn commit_overlay_to_backing(
        &self,
        overlay: &BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    ) -> MptResult<()> {
        let Some(backing) = self.backing.as_ref() else {
            return Ok(());
        };
        if overlay.is_empty() {
            return Ok(());
        }

        let mut snapshot = backing.snapshot();
        let writer = Arc::get_mut(&mut snapshot).ok_or_else(|| {
            MptError::storage("unable to obtain mutable state-service backing snapshot for commit")
        })?;
        for (key, value) in overlay {
            match value {
                Some(value) => writer.put(key.clone(), value.clone()),
                None => writer.delete(key.clone()),
            }
            .map_err(|err| {
                MptError::storage(format!("state-service backing write failed: {err}"))
            })?;
        }
        writer
            .try_commit()
            .map_err(|err| MptError::storage(format!("state-service backing commit failed: {err}")))
    }

    /// Returns the state-root record persisted for `index`, if any.
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        let map = Arc::clone(&self.kv.read());
        Self::read_state_root(&map, index)
    }

    /// Returns the current local root index, if a block has been
    /// applied (C# `StateSnapshot.CurrentLocalRootIndex`).
    pub fn current_local_root_index(&self) -> Option<u32> {
        let map = Arc::clone(&self.kv.read());
        Self::read_current_local_root_index(&map)
    }

    /// Returns the current local root hash, if a block has been
    /// applied (C# `StateSnapshot.CurrentLocalRootHash`).
    ///
    /// Callers that also walk the trie at the returned root must use
    /// [`MptStore::snapshot`] and read both from the same frozen view.
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        let map = Arc::clone(&self.kv.read());
        let index = Self::read_current_local_root_index(&map)?;
        Some(*Self::read_state_root(&map, index)?.root_hash())
    }

    /// Decodes the state-root record for `index` out of a generation
    /// map (shared by the live accessors and [`MptReadSnapshot`]).
    fn read_state_root(map: &BTreeMap<Vec<u8>, Vec<u8>>, index: u32) -> Option<StateRoot> {
        let bytes = map.get(&Keys::state_root(index))?;
        match Self::decode_state_root(bytes) {
            Some(root) => Some(root),
            None => {
                tracing::warn!(
                    target: "neo.state_service",
                    index,
                    "malformed state-root record in MPT store"
                );
                None
            }
        }
    }

    /// Reads the current local root index out of a generation map.
    fn read_current_local_root_index(map: &BTreeMap<Vec<u8>, Vec<u8>>) -> Option<u32> {
        let bytes = map.get(Keys::CURRENT_LOCAL_ROOT_INDEX)?;
        let arr: [u8; 4] = bytes.as_slice().try_into().ok()?;
        Some(u32::from_le_bytes(arr))
    }

    /// Serializes a state root in the C# `StateRoot` wire format:
    /// unsigned fields (`version u8 + index u32 LE + root_hash`)
    /// followed by a var-int witness count of `0` (local roots carry
    /// no witness; C# writes `WriteVarInt(0)` for a null witness).
    fn encode_state_root(root: &StateRoot) -> Vec<u8> {
        let mut bytes = root.unsigned_bytes();
        bytes.push(0x00);
        bytes
    }

    /// Decodes the unsigned prefix of a C#-format `StateRoot` record;
    /// the trailing witness array (if any) is ignored because the
    /// state service only needs `(version, index, root_hash)` here.
    fn decode_state_root(bytes: &[u8]) -> Option<StateRoot> {
        if bytes.len() < STATE_ROOT_UNSIGNED_LEN {
            return None;
        }
        let version = bytes[0];
        let index = u32::from_le_bytes(bytes[1..5].try_into().ok()?);
        let root_hash = UInt256::from_bytes(&bytes[5..5 + UINT256_SIZE]).ok()?;
        Some(StateRoot::new(version, index, root_hash))
    }
}

fn elapsed_us(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u64::MAX as u128) as u64
}

impl std::fmt::Debug for MptStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Clone the generation pointer once so no lock guard is held
        // across the field reads (parking_lot read locks are not
        // recursive once a writer queues up).
        let map = Arc::clone(&self.kv.read());
        f.debug_struct("MptStore")
            .field("entries", &map.len())
            .field("full_state", &self.full_state)
            .field(
                "local_root_index",
                &Self::read_current_local_root_index(&map),
            )
            .finish()
    }
}

#[cfg(test)]
#[path = "tests/mpt_store.rs"]
mod tests;
