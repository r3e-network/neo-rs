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
//! - the key/value namespace is an `Arc<HashMap>` behind an `RwLock`;
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
use neo_crypto::mpt_trie::{MptError, MptResult, MptStoreSnapshot, Node, Trie};
use neo_io::SerializableExtensions;
use neo_primitives::{UINT256_SIZE, UInt256};
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::{Store, StoreSnapshot};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Size of the serialized unsigned `StateRoot` prefix:
/// `version (1) + index (4, LE) + root_hash (32)`.
const STATE_ROOT_UNSIGNED_LEN: usize = 1 + 4 + UINT256_SIZE;
const MPT_NODE_PREFIX: u8 = 0xf0;

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
    kv: RwLock<Arc<HashMap<Vec<u8>, Option<Vec<u8>>>>>,
    /// Serializes block-changeset application.
    write_gate: Mutex<()>,
    /// Whether historical trie nodes are retained (C#
    /// `StateServiceSettings.FullState`, default `false`). With
    /// `false`, applying a block prunes the nodes the change set made
    /// unreachable, so only the *current* root stays resolvable.
    full_state: bool,
    /// Cached current local root `(index, hash)` for hot contiguity checks.
    ///
    /// Durable state-root records remain the source of truth for historical
    /// reads and for opening/reopening stores; this cache mirrors only the
    /// current pointer so block import does not need to open a backing snapshot
    /// just to confirm the previous block root.
    latest_local_root: RwLock<Option<(u32, UInt256)>>,
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
    map: Arc<HashMap<Vec<u8>, Option<Vec<u8>>>>,
    /// Frozen durable snapshot used for entries not present in `map`.
    backing_snapshot: Option<Arc<dyn StoreSnapshot>>,
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
        MptStore::read_state_root(&self.map, self.backing_snapshot.as_deref(), index)
    }

    /// Returns the local root index current as of this snapshot (C#
    /// `StateSnapshot.CurrentLocalRootIndex`).
    pub fn current_local_root_index(&self) -> Option<u32> {
        MptStore::read_current_local_root_index(&self.map, self.backing_snapshot.as_deref())
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
        match self.map.get(key) {
            Some(value) => Ok(value.clone()),
            None => Ok(self
                .backing_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.try_get_bytes(key))),
        }
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
pub(crate) struct MptWriteBatch {
    /// Generation the block builds on.
    base: Arc<HashMap<Vec<u8>, Option<Vec<u8>>>>,
    /// Durable snapshot taken before this write batch starts.
    backing_snapshot: Option<Arc<dyn StoreSnapshot>>,
    /// Staged mutations: `Some(value)` = put, `None` = delete.
    overlay: Mutex<HashMap<Vec<u8>, Option<Vec<u8>>>>,
    /// Fast-path guard for reads before `Trie::commit` stages node writes.
    overlay_has_entries: AtomicBool,
}

impl MptWriteBatch {
    fn new(
        base: Arc<HashMap<Vec<u8>, Option<Vec<u8>>>>,
        backing_snapshot: Option<Arc<dyn StoreSnapshot>>,
        overlay_capacity: usize,
    ) -> Self {
        Self {
            base,
            backing_snapshot,
            overlay: Mutex::new(HashMap::with_capacity(overlay_capacity)),
            overlay_has_entries: AtomicBool::new(false),
        }
    }

    fn overlay_contains_entries(&self) -> bool {
        self.overlay_has_entries.load(Ordering::Acquire)
    }

    fn mark_overlay_non_empty(&self) {
        self.overlay_has_entries.store(true, Ordering::Release);
    }
}

impl MptStoreSnapshot for MptWriteBatch {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        if self.overlay_contains_entries() {
            if let Some(staged) = self.overlay.lock().get(key) {
                return Ok(staged.clone());
            }
        }
        match self.base.get(key) {
            Some(value) => Ok(value.clone()),
            None => Ok(self
                .backing_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.try_get_bytes(key))),
        }
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        let mut overlay = self.overlay.lock();
        self.mark_overlay_non_empty();
        overlay.insert(key, Some(value));
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        let mut overlay = self.overlay.lock();
        self.mark_overlay_non_empty();
        overlay.insert(key, None);
        Ok(())
    }

    fn apply_overlay(&self, overlay: Vec<(Vec<u8>, Option<Vec<u8>>)>) -> MptResult<()> {
        if overlay.is_empty() {
            return Ok(());
        }
        let mut staged = self.overlay.lock();
        self.mark_overlay_non_empty();
        staged.extend(overlay);
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
            kv: RwLock::new(Arc::new(HashMap::new())),
            write_gate: Mutex::new(()),
            full_state,
            latest_local_root: RwLock::new(None),
            backing: None,
        }
    }

    /// Opens a store over an existing durable byte namespace.
    pub fn from_store(backing: Arc<dyn Store>, full_state: bool) -> MptResult<Self> {
        let latest_local_root = Self::load_latest_local_root_from_backing(backing.as_ref());
        Ok(Self {
            kv: RwLock::new(Arc::new(HashMap::new())),
            write_gate: Mutex::new(()),
            full_state,
            latest_local_root: RwLock::new(latest_local_root),
            backing: Some(backing),
        })
    }

    /// Opens a store over a concrete in-memory backend.
    ///
    /// This is primarily for tests and ephemeral nodes that need persistence
    /// across `MptStore` instances without erasing the backend behind
    /// `dyn Store`.
    pub fn from_memory_store(backing: Arc<MemoryStore>, full_state: bool) -> MptResult<Self> {
        Self::from_store(backing, full_state)
    }

    /// Returns whether historical trie versions are retained.
    pub fn full_state(&self) -> bool {
        self.full_state
    }

    fn backing_snapshot(&self) -> Option<Arc<dyn StoreSnapshot>> {
        self.backing.as_ref().map(|backing| backing.snapshot())
    }

    fn load_latest_local_root_from_backing(backing: &dyn Store) -> Option<(u32, UInt256)> {
        let snapshot = backing.snapshot();
        let index = Self::read_current_local_root_index(&HashMap::new(), Some(snapshot.as_ref()))?;
        let root =
            *Self::read_state_root(&HashMap::new(), Some(snapshot.as_ref()), index)?.root_hash();
        Some((index, root))
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
            backing_snapshot: self.backing_snapshot(),
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
        self.apply_block_changes_with_len(block_index, root_before, changes.len(), |trie| {
            let mut effective_change_count = 0usize;
            for change in changes {
                effective_change_count += 1;
                match change {
                    MptChange::Put { key, value } => trie.put(key, value)?,
                    MptChange::Delete { key } => {
                        // C# ignores the `Trie.Delete` return value: deleting
                        // a key that is already absent is a no-op.
                        let _ = trie.delete(key)?;
                    }
                }
            }
            Ok(effective_change_count)
        })
    }

    pub(crate) fn apply_block_changes_with_len<F>(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        change_count: usize,
        mutate: F,
    ) -> MptResult<UInt256>
    where
        F: FnOnce(&mut Trie<MptWriteBatch>) -> MptResult<usize>,
    {
        let _writer = self.write_gate.lock();
        if change_count == 0
            && let Some(root_before) = root_before
        {
            StateRootApplyMetrics::record_count(StateRootApplyCountKind::Changes, 0);
            let overlay = Self::local_root_overlay(block_index, root_before);
            return self.publish_overlay(block_index, root_before, overlay);
        }

        // Stage every mutation against the current generation. The
        // writer gate guarantees the base cannot change underneath us.
        let batch = Arc::new(MptWriteBatch::new(
            Arc::clone(&self.kv.read()),
            self.backing_snapshot(),
            change_count.saturating_mul(2) + 2,
        ));

        let mut trie = Trie::new(Arc::clone(&batch), root_before, self.full_state);
        let stage_start = Instant::now();
        let effective_change_count = mutate(&mut trie)?;
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::Changes,
            effective_change_count as u64,
        );
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::MutateChanges,
            elapsed_us(stage_start),
        );

        // C# reads `Trie.Root.Hash` (well-defined even for the empty
        // root, which hashes its single sentinel byte).
        let stage_start = Instant::now();
        let new_root = trie.root().try_hash()?;
        StateRootApplyMetrics::record_stage(StateRootApplyStage::RootHash, elapsed_us(stage_start));
        if effective_change_count == 0 && root_before.is_some() {
            drop(trie);
            drop(batch);
            let overlay = Self::local_root_overlay(block_index, new_root);
            return self.publish_overlay(block_index, new_root, overlay);
        }

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
        let overlay = {
            let mut overlay = batch.overlay.lock();
            Self::insert_local_root_records(&mut overlay, block_index, new_root);
            Self::ensure_empty_root_record(&mut overlay, new_root)?;
            std::mem::take(&mut *overlay)
        };
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::OverlayPrepare,
            elapsed_us(stage_start),
        );
        // Release the batch's base Arc before publishing so that, with
        // no reader snapshots outstanding, `make_mut` updates in place
        // instead of cloning the map.
        drop(batch);

        self.publish_overlay(block_index, new_root, overlay)
    }

    fn publish_overlay(
        &self,
        block_index: u32,
        new_root: UInt256,
        overlay: HashMap<Vec<u8>, Option<Vec<u8>>>,
    ) -> MptResult<UInt256> {
        let stage_start = Instant::now();
        let backing_result = self.commit_overlay_to_backing(&overlay);
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::BackingCommit,
            elapsed_us(stage_start),
        );
        backing_result?;

        let stage_start = Instant::now();
        let mut kv = self.kv.write();
        let mut overlay_puts = 0u64;
        let mut overlay_deletes = 0u64;
        if self.should_publish_live_overlay() {
            let map = Arc::make_mut(&mut *kv);
            for (key, value) in overlay {
                Self::count_overlay_entry(value.as_ref(), &mut overlay_puts, &mut overlay_deletes);
                map.insert(key, value);
            }
        } else {
            for (_, value) in overlay {
                Self::count_overlay_entry(value.as_ref(), &mut overlay_puts, &mut overlay_deletes);
            }
            *kv = Arc::new(HashMap::new());
        }
        Self::record_overlay_counts(overlay_puts, overlay_deletes);
        *self.latest_local_root.write() = Some((block_index, new_root));
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::PublishGeneration,
            elapsed_us(stage_start),
        );
        Ok(new_root)
    }

    fn count_overlay_entry(
        value: Option<&Vec<u8>>,
        overlay_puts: &mut u64,
        overlay_deletes: &mut u64,
    ) {
        if value.is_some() {
            *overlay_puts += 1;
        } else {
            *overlay_deletes += 1;
        }
    }

    fn local_root_overlay(
        block_index: u32,
        root_hash: UInt256,
    ) -> HashMap<Vec<u8>, Option<Vec<u8>>> {
        let mut overlay = HashMap::with_capacity(2);
        Self::insert_local_root_records(&mut overlay, block_index, root_hash);
        StateRootApplyMetrics::record_stage(StateRootApplyStage::OverlayPrepare, 0);
        overlay
    }

    fn should_publish_live_overlay(&self) -> bool {
        match self.backing.as_ref() {
            None => true,
            Some(backing) => backing.has_pending_fast_sync_writes(),
        }
    }

    fn insert_local_root_records(
        overlay: &mut HashMap<Vec<u8>, Option<Vec<u8>>>,
        block_index: u32,
        root_hash: UInt256,
    ) {
        let root_record = StateRoot::new_current(block_index, root_hash);
        overlay.insert(
            Keys::state_root(block_index),
            Some(Self::encode_state_root(&root_record)),
        );
        overlay.insert(
            Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
            Some(block_index.to_le_bytes().to_vec()),
        );
    }

    fn ensure_empty_root_record(
        overlay: &mut HashMap<Vec<u8>, Option<Vec<u8>>>,
        root_hash: UInt256,
    ) -> MptResult<()> {
        let empty = Node::new();
        if root_hash != empty.try_hash()? {
            return Ok(());
        }
        overlay.insert(
            Self::mpt_node_key(root_hash),
            Some(empty.to_array().map_err(MptError::from)?),
        );
        Ok(())
    }

    fn mpt_node_key(root_hash: UInt256) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + UINT256_SIZE);
        key.push(MPT_NODE_PREFIX);
        key.extend_from_slice(&root_hash.to_array());
        key
    }

    fn record_overlay_counts(overlay_puts: u64, overlay_deletes: u64) {
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::OverlayEntries,
            overlay_puts + overlay_deletes,
        );
        StateRootApplyMetrics::record_count(StateRootApplyCountKind::OverlayPuts, overlay_puts);
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::OverlayDeletes,
            overlay_deletes,
        );
    }

    /// Rewinds local state-root records for an inclusive reverted block range.
    ///
    /// Full-state mode retains historical trie nodes, so rewinding the current
    /// local root pointer to the block before `from_index` leaves the trie
    /// resolvable. Pruning mode may have deleted those historical nodes while
    /// applying later blocks, so it refuses rewinds that would move the current
    /// root backward.
    pub fn revert_local_roots(&self, from_index: u32, to_index: u32) -> MptResult<()> {
        if from_index > to_index {
            return Ok(());
        }

        let _writer = self.write_gate.lock();
        let current_index = self.current_local_root_index();
        let rewinds_current = current_index.is_some_and(|index| index >= from_index);
        if rewinds_current && !self.full_state {
            return Err(MptError::invalid(
                "cannot rewind pruning-mode StateService MPT local roots because historical trie nodes may have been pruned",
            ));
        }

        let mut overlay = HashMap::with_capacity((to_index - from_index + 1) as usize + 1);
        for index in from_index..=to_index {
            overlay.insert(Keys::state_root(index), None);
        }

        let mut rewound_latest_root = *self.latest_local_root.read();
        if rewinds_current {
            match from_index.checked_sub(1) {
                Some(previous_index) => {
                    let map = Arc::clone(&self.kv.read());
                    let backing_snapshot = self.backing_snapshot();
                    let previous_root =
                        Self::read_state_root(&map, backing_snapshot.as_deref(), previous_index)
                            .ok_or_else(|| {
                                MptError::invalid(format!(
                                    "cannot rewind StateService MPT local root to missing block {previous_index}"
                                ))
                            })?;
                    rewound_latest_root = Some((previous_index, *previous_root.root_hash()));
                    overlay.insert(
                        Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                        Some(previous_index.to_le_bytes().to_vec()),
                    );
                }
                None => {
                    rewound_latest_root = None;
                    overlay.insert(Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(), None);
                }
            }
        }

        self.commit_overlay_to_backing(&overlay)?;

        let mut kv = self.kv.write();
        if self.should_publish_live_overlay() {
            let map = Arc::make_mut(&mut *kv);
            map.extend(overlay);
        } else {
            *kv = Arc::new(HashMap::new());
        }
        *self.latest_local_root.write() = rewound_latest_root;
        Ok(())
    }

    fn commit_overlay_to_backing(
        &self,
        overlay: &HashMap<Vec<u8>, Option<Vec<u8>>>,
    ) -> MptResult<()> {
        match self.backing.as_ref() {
            None => Ok(()),
            Some(backing) => {
                let mut entries = overlay.iter().collect::<Vec<_>>();
                entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
                let mut visit = |sink: &mut dyn FnMut(&[u8], Option<&[u8]>)| {
                    for (key, value) in &entries {
                        sink(key, value.as_deref());
                    }
                };
                let committed = backing
                    .try_commit_borrowed_raw_overlay(&mut visit)
                    .map_err(|err| {
                        MptError::storage(format!("state-service backing commit failed: {err}"))
                    })?;
                if committed {
                    return Ok(());
                }

                let mut snapshot = backing.snapshot();
                let writer = Arc::get_mut(&mut snapshot).ok_or_else(|| {
                    MptError::storage(
                        "unable to obtain mutable state-service backing snapshot for commit",
                    )
                })?;
                for (key, value) in entries {
                    match value {
                        Some(value) => writer.put(key.clone(), value.clone()),
                        None => writer.delete(key.clone()),
                    }
                    .map_err(|err| {
                        MptError::storage(format!("state-service backing write failed: {err}"))
                    })?;
                }
                writer.try_commit().map_err(|err| {
                    MptError::storage(format!("state-service backing commit failed: {err}"))
                })
            }
        }
    }

    /// Returns the state-root record persisted for `index`, if any.
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        let map = Arc::clone(&self.kv.read());
        let backing_snapshot = self.backing_snapshot();
        Self::read_state_root(&map, backing_snapshot.as_deref(), index)
    }

    /// Returns the current local root index, if a block has been
    /// applied (C# `StateSnapshot.CurrentLocalRootIndex`).
    pub fn current_local_root_index(&self) -> Option<u32> {
        self.current_local_root().map(|(index, _)| index)
    }

    /// Returns the current local root hash, if a block has been
    /// applied (C# `StateSnapshot.CurrentLocalRootHash`).
    ///
    /// Callers that also walk the trie at the returned root must use
    /// [`MptStore::snapshot`] and read both from the same frozen view.
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        self.current_local_root().map(|(_, root)| root)
    }

    /// Returns the current local root index and hash from one generation.
    ///
    /// This keeps hot-path contiguity checks from taking separate snapshots for
    /// the index and hash.
    pub fn current_local_root(&self) -> Option<(u32, UInt256)> {
        *self.latest_local_root.read()
    }

    /// Decodes the state-root record for `index` out of a generation
    /// map (shared by the live accessors and [`MptReadSnapshot`]).
    fn read_state_root(
        map: &HashMap<Vec<u8>, Option<Vec<u8>>>,
        backing_snapshot: Option<&dyn StoreSnapshot>,
        index: u32,
    ) -> Option<StateRoot> {
        let key = Keys::state_root(index);
        let bytes = match map.get(&key) {
            Some(Some(bytes)) => bytes.clone(),
            Some(None) => return None,
            None => backing_snapshot?.try_get(&key)?,
        };
        match Self::decode_state_root(&bytes) {
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
    fn read_current_local_root_index(
        map: &HashMap<Vec<u8>, Option<Vec<u8>>>,
        backing_snapshot: Option<&dyn StoreSnapshot>,
    ) -> Option<u32> {
        let key = Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec();
        let bytes = match map.get(&key) {
            Some(Some(bytes)) => bytes.clone(),
            Some(None) => return None,
            None => backing_snapshot?.try_get(&key)?,
        };
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
            .field("local_root_index", &self.current_local_root_index())
            .finish()
    }
}

#[cfg(test)]
#[path = "../tests/storage/mpt_store.rs"]
mod tests;
