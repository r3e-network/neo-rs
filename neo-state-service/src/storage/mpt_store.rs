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
//! build can keep the namespace in process memory for tests or place it
//! behind the workspace [`Store`] provider boundary for production
//! backends such as MDBX. The byte layout is the C# one, so changing the
//! physical backend is a storage swap, not a format migration.
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
use crate::state_root::{CURRENT_VERSION, StateRoot};
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

/// Borrowed storage changes for one block in an ordered MPT batch.
pub(crate) struct MptBlockChanges<'a> {
    /// Block height these changes belong to.
    pub(crate) block_index: u32,
    /// Projected non-Ledger storage mutations for the block.
    pub(crate) changes: &'a [MptChange],
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct OverlayCounts {
    puts: u64,
    deletes: u64,
}

impl OverlayCounts {
    fn record(&mut self, value: Option<&Vec<u8>>) {
        if value.is_some() {
            self.record_put();
        } else {
            self.record_delete();
        }
    }

    fn record_put(&mut self) {
        self.puts += 1;
    }

    fn record_delete(&mut self) {
        self.deletes += 1;
    }

    fn entries(self) -> u64 {
        self.puts + self.deletes
    }
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

/// Lazily opens the write trie only when a block has at least one effective
/// StateService MPT mutation.
pub(crate) struct LazyMptChangeSink<'a> {
    store: &'a MptStore,
    root_before: Option<UInt256>,
    overlay_capacity: usize,
    batch: Option<Arc<MptWriteBatch>>,
    trie: Option<Trie<MptWriteBatch>>,
}

impl<'a> LazyMptChangeSink<'a> {
    fn new(store: &'a MptStore, root_before: Option<UInt256>, overlay_capacity: usize) -> Self {
        Self {
            store,
            root_before,
            overlay_capacity,
            batch: None,
            trie: None,
        }
    }

    fn ensure_trie(&mut self) -> &mut Trie<MptWriteBatch> {
        if self.trie.is_none() {
            let batch = Arc::new(MptWriteBatch::new(
                Arc::clone(&self.store.kv.read()),
                self.store.backing_snapshot(),
                self.overlay_capacity,
            ));
            self.trie = Some(Trie::new(
                Arc::clone(&batch),
                self.root_before,
                self.store.full_state,
            ));
            self.batch = Some(batch);
        }
        self.trie.as_mut().expect("lazy MPT trie initialized")
    }

    /// Inserts or updates a key, opening the write trie on first use.
    pub(crate) fn put_with_scratch(
        &mut self,
        key: &[u8],
        value: &[u8],
        path_scratch: &mut Vec<u8>,
    ) -> MptResult<()> {
        self.ensure_trie()
            .put_with_scratch(key, value, path_scratch)
    }

    /// Deletes a key, opening the write trie on first use.
    pub(crate) fn delete_with_scratch(
        &mut self,
        key: &[u8],
        path_scratch: &mut Vec<u8>,
    ) -> MptResult<bool> {
        self.ensure_trie().delete_with_scratch(key, path_scratch)
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
            let mut path_scratch = Vec::new();
            for change in changes {
                effective_change_count += 1;
                match change {
                    MptChange::Put { key, value } => {
                        trie.put_with_scratch(key, value, &mut path_scratch)?
                    }
                    MptChange::Delete { key } => {
                        // C# ignores the `Trie.Delete` return value: deleting
                        // a key that is already absent is a no-op.
                        let _ = trie.delete_with_scratch(key, &mut path_scratch)?;
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

    pub(crate) fn apply_block_changes_lazy<F>(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        change_capacity_hint: usize,
        mutate: F,
    ) -> MptResult<UInt256>
    where
        F: FnOnce(&mut LazyMptChangeSink<'_>) -> MptResult<usize>,
    {
        let _writer = self.write_gate.lock();
        let mut sink = LazyMptChangeSink::new(
            self,
            root_before,
            change_capacity_hint.saturating_mul(2).saturating_add(2),
        );

        let stage_start = Instant::now();
        let effective_change_count = mutate(&mut sink)?;
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::Changes,
            effective_change_count as u64,
        );
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::MutateChanges,
            elapsed_us(stage_start),
        );

        let Some(mut trie) = sink.trie.take() else {
            let stage_start = Instant::now();
            let new_root = match root_before {
                Some(root_before) => root_before,
                None => Node::new().try_hash()?,
            };
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::RootHash,
                elapsed_us(stage_start),
            );

            let mut overlay = Self::local_root_overlay(block_index, new_root);
            Self::ensure_empty_root_record(&mut overlay, new_root)?;
            return self.publish_overlay(block_index, new_root, overlay);
        };
        let batch = sink.batch.take().expect("lazy MPT trie has a write batch");

        let stage_start = Instant::now();
        let new_root = trie.root().try_hash()?;
        StateRootApplyMetrics::record_stage(StateRootApplyStage::RootHash, elapsed_us(stage_start));
        if !(effective_change_count == 0 && root_before.is_some()) {
            let stage_start = Instant::now();
            trie.commit()?;
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::TrieCommit,
                elapsed_us(stage_start),
            );
        }
        drop(trie);

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
        drop(batch);

        self.publish_overlay(block_index, new_root, overlay)
    }

    pub(crate) fn apply_block_changes_batch(
        &self,
        root_before: Option<UInt256>,
        blocks: &[MptBlockChanges<'_>],
    ) -> MptResult<Vec<UInt256>> {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }

        let _writer = self.write_gate.lock();
        self.validate_ordered_batch(root_before, blocks)?;
        if let Some(root_before) = root_before
            && blocks.iter().all(|block| block.changes.is_empty())
        {
            let samples = blocks.len() as u64;
            Self::record_count_samples(StateRootApplyCountKind::Changes, 0, samples);
            Self::record_stage_samples(StateRootApplyStage::MutateChanges, 0, samples);
            Self::record_stage_samples(StateRootApplyStage::RootHash, 0, samples);

            self.publish_empty_root_batch(blocks, root_before)?;
            return Ok(vec![root_before; blocks.len()]);
        }

        let overlay_capacity = blocks
            .iter()
            .map(|block| block.changes.len().saturating_mul(2).saturating_add(2))
            .sum();
        let batch = Arc::new(MptWriteBatch::new(
            Arc::clone(&self.kv.read()),
            self.backing_snapshot(),
            overlay_capacity,
        ));
        let mut trie = Trie::new(Arc::clone(&batch), root_before, self.full_state);
        let mut roots = Vec::with_capacity(blocks.len());
        let mut current_root = root_before;
        let mut path_scratch = Vec::new();

        for block in blocks {
            let stage_start = Instant::now();
            let mut effective_change_count = 0usize;
            for change in block.changes {
                effective_change_count += 1;
                match change {
                    MptChange::Put { key, value } => {
                        trie.put_with_scratch(key, value, &mut path_scratch)?
                    }
                    MptChange::Delete { key } => {
                        let _ = trie.delete_with_scratch(key, &mut path_scratch)?;
                    }
                }
            }
            StateRootApplyMetrics::record_count(
                StateRootApplyCountKind::Changes,
                effective_change_count as u64,
            );
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::MutateChanges,
                elapsed_us(stage_start),
            );

            let stage_start = Instant::now();
            let new_root = trie.root().try_hash()?;
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::RootHash,
                elapsed_us(stage_start),
            );

            if !(effective_change_count == 0 && current_root.is_some()) {
                let stage_start = Instant::now();
                trie.commit()?;
                StateRootApplyMetrics::record_stage(
                    StateRootApplyStage::TrieCommit,
                    elapsed_us(stage_start),
                );
            }

            let stage_start = Instant::now();
            {
                let mut overlay = batch.overlay.lock();
                Self::insert_local_root_records(&mut overlay, block.block_index, new_root);
                Self::ensure_empty_root_record(&mut overlay, new_root)?;
            }
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::OverlayPrepare,
                elapsed_us(stage_start),
            );

            roots.push(new_root);
            current_root = Some(new_root);
        }

        let overlay = {
            let mut overlay = batch.overlay.lock();
            std::mem::take(&mut *overlay)
        };
        drop(trie);
        drop(batch);

        self.publish_overlay_with_samples(
            blocks.last().expect("non-empty batch").block_index,
            *roots.last().expect("non-empty roots"),
            overlay,
            blocks.len(),
        )?;
        Ok(roots)
    }

    fn validate_ordered_batch(
        &self,
        root_before: Option<UInt256>,
        blocks: &[MptBlockChanges<'_>],
    ) -> MptResult<()> {
        let first = blocks
            .first()
            .expect("batch validation is only called for non-empty batches");
        match (self.current_local_root(), root_before) {
            (None, None) if first.block_index == 0 => {}
            (None, None) => {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: no local root exists before block {}",
                    first.block_index
                )));
            }
            (Some((previous_index, current_root)), Some(root_before))
                if previous_index.checked_add(1) == Some(first.block_index)
                    && current_root == root_before => {}
            (Some((previous_index, current_root)), Some(root_before)) => {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: current local root is ({previous_index}, {current_root}), requested previous root is {root_before} before block {}",
                    first.block_index
                )));
            }
            (None, Some(root_before)) => {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: requested previous root {root_before} but no local root exists before block {}",
                    first.block_index
                )));
            }
            (Some((previous_index, current_root)), None) => {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: current local root is ({previous_index}, {current_root}) but no previous root was supplied before block {}",
                    first.block_index
                )));
            }
        }

        for pair in blocks.windows(2) {
            if pair[0].block_index.checked_add(1) != Some(pair[1].block_index) {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: block {} followed by {}",
                    pair[0].block_index, pair[1].block_index
                )));
            }
        }
        Ok(())
    }

    fn publish_overlay(
        &self,
        block_index: u32,
        new_root: UInt256,
        overlay: HashMap<Vec<u8>, Option<Vec<u8>>>,
    ) -> MptResult<UInt256> {
        self.publish_overlay_with_samples(block_index, new_root, overlay, 1)
    }

    fn publish_overlay_with_samples(
        &self,
        block_index: u32,
        new_root: UInt256,
        overlay: HashMap<Vec<u8>, Option<Vec<u8>>>,
        samples: usize,
    ) -> MptResult<UInt256> {
        let samples = samples.max(1) as u64;
        let stage_start = Instant::now();
        let backing_result = self.commit_overlay_to_backing(&overlay);
        Self::record_stage_samples(
            StateRootApplyStage::BackingCommit,
            elapsed_us(stage_start),
            samples,
        );
        let backing_counts = backing_result?;

        let stage_start = Instant::now();
        let mut overlay_counts = OverlayCounts::default();
        if self.should_publish_live_overlay() {
            let mut kv = self.kv.write();
            let map = Arc::make_mut(&mut *kv);
            for (key, value) in overlay {
                overlay_counts.record(value.as_ref());
                map.insert(key, value);
            }
        } else {
            overlay_counts = backing_counts;
            self.clear_live_overlay_if_needed();
        }
        Self::record_overlay_counts(overlay_counts, samples);
        *self.latest_local_root.write() = Some((block_index, new_root));
        Self::record_stage_samples(
            StateRootApplyStage::PublishGeneration,
            elapsed_us(stage_start),
            samples,
        );
        Ok(new_root)
    }

    fn publish_empty_root_batch(
        &self,
        blocks: &[MptBlockChanges<'_>],
        root_hash: UInt256,
    ) -> MptResult<UInt256> {
        let last_index = blocks
            .last()
            .expect("empty-root batch is only called for non-empty batches")
            .block_index;
        let samples = blocks.len().max(1) as u64;

        let stage_start = Instant::now();
        let empty_root = Node::new();
        let empty_root_record = if root_hash == empty_root.try_hash()? {
            Some((
                Self::mpt_node_key_bytes(root_hash),
                empty_root.to_array().map_err(MptError::from)?,
            ))
        } else {
            None
        };
        Self::record_stage_samples(
            StateRootApplyStage::OverlayPrepare,
            elapsed_us(stage_start),
            samples,
        );

        let stage_start = Instant::now();
        let backing_counts =
            self.commit_empty_root_batch_to_backing(blocks, root_hash, empty_root_record.as_ref())?;
        Self::record_stage_samples(
            StateRootApplyStage::BackingCommit,
            elapsed_us(stage_start),
            samples,
        );

        let stage_start = Instant::now();
        let overlay_counts = if self.should_publish_live_overlay() {
            let mut counts = OverlayCounts::default();
            let mut kv = self.kv.write();
            let map = Arc::make_mut(&mut *kv);
            for block in blocks {
                map.insert(
                    Keys::state_root(block.block_index),
                    Some(Self::encode_state_root_fields(block.block_index, root_hash)),
                );
                counts.record_put();
            }
            map.insert(
                Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                Some(last_index.to_le_bytes().to_vec()),
            );
            counts.record_put();
            if let Some((key, value)) = empty_root_record {
                map.insert(key.to_vec(), Some(value));
                counts.record_put();
            }
            counts
        } else {
            self.clear_live_overlay_if_needed();
            backing_counts
        };
        Self::record_overlay_counts(overlay_counts, samples);
        *self.latest_local_root.write() = Some((last_index, root_hash));
        Self::record_stage_samples(
            StateRootApplyStage::PublishGeneration,
            elapsed_us(stage_start),
            samples,
        );
        Ok(root_hash)
    }

    fn commit_empty_root_batch_to_backing(
        &self,
        blocks: &[MptBlockChanges<'_>],
        root_hash: UInt256,
        empty_root_record: Option<&([u8; 1 + UINT256_SIZE], Vec<u8>)>,
    ) -> MptResult<OverlayCounts> {
        match self.backing.as_ref() {
            None => Ok(OverlayCounts::default()),
            Some(backing) => {
                let current_index = blocks
                    .last()
                    .expect("empty-root backing commit requires non-empty blocks")
                    .block_index;
                let current_index_value = current_index.to_le_bytes();
                let mut counts = OverlayCounts::default();
                let mut visit = |sink: &mut dyn FnMut(&[u8], Option<&[u8]>)| {
                    for block in blocks {
                        let key = Self::state_root_key_bytes(block.block_index);
                        let value = Self::encode_state_root_fields(block.block_index, root_hash);
                        counts.record_put();
                        sink(&key, Some(&value));
                    }
                    counts.record_put();
                    sink(
                        Keys::CURRENT_LOCAL_ROOT_INDEX,
                        Some(current_index_value.as_slice()),
                    );
                    if let Some((key, value)) = empty_root_record {
                        counts.record_put();
                        sink(key.as_slice(), Some(value.as_slice()));
                    }
                };
                let committed = backing
                    .try_commit_borrowed_raw_overlay(&mut visit)
                    .map_err(|err| {
                        MptError::storage(format!(
                            "state-service empty-batch backing commit failed: {err}"
                        ))
                    })?;
                if committed {
                    return Ok(counts);
                }

                let mut counts = OverlayCounts::default();
                let mut snapshot = backing.snapshot();
                let writer = Arc::get_mut(&mut snapshot).ok_or_else(|| {
                    MptError::storage(
                        "unable to obtain mutable state-service backing snapshot for empty-batch commit",
                    )
                })?;
                for block in blocks {
                    counts.record_put();
                    writer
                        .put(
                            Keys::state_root(block.block_index),
                            Self::encode_state_root_fields(block.block_index, root_hash),
                        )
                        .map_err(|err| {
                            MptError::storage(format!(
                                "state-service empty-batch backing write failed: {err}"
                            ))
                        })?;
                }
                counts.record_put();
                writer
                    .put(
                        Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                        current_index_value.to_vec(),
                    )
                    .map_err(|err| {
                        MptError::storage(format!(
                            "state-service empty-batch backing write failed: {err}"
                        ))
                    })?;
                if let Some((key, value)) = empty_root_record {
                    counts.record_put();
                    writer.put(key.to_vec(), value.clone()).map_err(|err| {
                        MptError::storage(format!(
                            "state-service empty-batch backing write failed: {err}"
                        ))
                    })?;
                }
                writer.try_commit().map_err(|err| {
                    MptError::storage(format!(
                        "state-service empty-batch backing commit failed: {err}"
                    ))
                })?;
                Ok(counts)
            }
        }
    }

    fn clear_live_overlay_if_needed(&self) {
        if self.kv.read().is_empty() {
            return;
        }

        let mut kv = self.kv.write();
        if !kv.is_empty() {
            *kv = Arc::new(HashMap::new());
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
        overlay.insert(
            Keys::state_root(block_index),
            Some(Self::encode_state_root_fields(block_index, root_hash)),
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
        Self::mpt_node_key_bytes(root_hash).to_vec()
    }

    fn mpt_node_key_bytes(root_hash: UInt256) -> [u8; 1 + UINT256_SIZE] {
        let mut key = [0u8; 1 + UINT256_SIZE];
        key[0] = MPT_NODE_PREFIX;
        key[1..].copy_from_slice(&root_hash.to_array());
        key
    }

    fn state_root_key_bytes(index: u32) -> [u8; 5] {
        let mut key = [0u8; 5];
        key[0] = 0x01;
        key[1..].copy_from_slice(&index.to_be_bytes());
        key
    }

    fn state_root_record_len() -> usize {
        STATE_ROOT_UNSIGNED_LEN + 1
    }

    fn encode_state_root_fields(index: u32, root_hash: UInt256) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::state_root_record_len());
        bytes.push(CURRENT_VERSION);
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&root_hash.to_bytes());
        bytes.push(0x00);
        bytes
    }

    fn record_overlay_counts(counts: OverlayCounts, samples: u64) {
        let samples = samples.max(1);
        Self::record_count_samples(
            StateRootApplyCountKind::OverlayEntries,
            counts.entries(),
            samples,
        );
        Self::record_count_samples(StateRootApplyCountKind::OverlayPuts, counts.puts, samples);
        Self::record_count_samples(
            StateRootApplyCountKind::OverlayDeletes,
            counts.deletes,
            samples,
        );
    }

    fn record_stage_samples(stage: StateRootApplyStage, elapsed_us: u64, samples: u64) {
        let samples = samples.max(1);
        let base = elapsed_us / samples;
        let remainder = elapsed_us % samples;
        for index in 0..samples {
            let sample = base + u64::from(index < remainder);
            StateRootApplyMetrics::record_stage(stage, sample);
        }
    }

    fn record_count_samples(kind: StateRootApplyCountKind, count: u64, samples: u64) {
        let samples = samples.max(1);
        let base = count / samples;
        let remainder = count % samples;
        for index in 0..samples {
            let sample = base + u64::from(index < remainder);
            StateRootApplyMetrics::record_count(kind, sample);
        }
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

        if self.should_publish_live_overlay() {
            let mut kv = self.kv.write();
            let map = Arc::make_mut(&mut *kv);
            map.extend(overlay);
        } else {
            self.clear_live_overlay_if_needed();
        }
        *self.latest_local_root.write() = rewound_latest_root;
        Ok(())
    }

    fn commit_overlay_to_backing(
        &self,
        overlay: &HashMap<Vec<u8>, Option<Vec<u8>>>,
    ) -> MptResult<OverlayCounts> {
        match self.backing.as_ref() {
            None => Ok(OverlayCounts::default()),
            Some(backing) => {
                let mut entries = overlay.iter().collect::<Vec<_>>();
                entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
                let mut counts = OverlayCounts::default();
                let mut visit = |sink: &mut dyn FnMut(&[u8], Option<&[u8]>)| {
                    for (key, value) in &entries {
                        counts.record(value.as_ref());
                        sink(key, value.as_deref());
                    }
                };
                let committed = backing
                    .try_commit_borrowed_raw_overlay(&mut visit)
                    .map_err(|err| {
                        MptError::storage(format!("state-service backing commit failed: {err}"))
                    })?;
                if committed {
                    return Ok(counts);
                }

                let mut counts = OverlayCounts::default();
                let mut snapshot = backing.snapshot();
                let writer = Arc::get_mut(&mut snapshot).ok_or_else(|| {
                    MptError::storage(
                        "unable to obtain mutable state-service backing snapshot for commit",
                    )
                })?;
                for (key, value) in entries {
                    counts.record(value.as_ref());
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
                })?;
                Ok(counts)
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
