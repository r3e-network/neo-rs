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

use crate::state_root::StateRoot;
use neo_crypto::mpt_trie::{MptError, MptResult, MptStoreSnapshot, Trie};
use neo_primitives::{UInt256, UINT256_SIZE};
use neo_state_types::Keys;
use parking_lot::{Mutex, RwLock};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Size of the serialized unsigned `StateRoot` prefix:
/// `version (1) + index (4, LE) + root_hash (32)`.
const STATE_ROOT_UNSIGNED_LEN: usize = 1 + 4 + UINT256_SIZE;

/// One storage mutation from a block's change set.
///
/// Mirrors the `TrackState` cases the C# `StateStore.
/// UpdateLocalStateRootSnapshot` consumes: `Added` / `Changed` both
/// become a trie `Put`, `Deleted` becomes a trie `Delete` (`None`
/// entries are filtered out by the caller, exactly as the C#
/// `Blockchain_Committing_Handler` filters
/// `p.Value.State != TrackState.None`).
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
        }
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
    /// The caller (the node's block-persist pipeline) is responsible
    /// for the C# `Blockchain_Committing_Handler` filtering: drop
    /// `TrackState.None` entries and every key belonging to the
    /// Ledger native contract (`p.Key.Id != NativeContract.Ledger.Id`)
    /// before handing the change set over.
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

        // Stage every mutation against the current generation. The
        // writer gate guarantees the base cannot change underneath us.
        let batch = Arc::new(MptWriteBatch {
            base: Arc::clone(&self.kv.read()),
            overlay: Mutex::new(BTreeMap::new()),
        });

        let mut trie = Trie::new(Arc::clone(&batch), root_before, self.full_state);
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

        // C# reads `Trie.Root.Hash` (well-defined even for the empty
        // root, which hashes its single sentinel byte).
        let new_root = trie.root().try_hash()?;
        trie.commit()?;
        drop(trie);

        // The local (unwitnessed) state-root record and the current
        // local root index advance in the same published generation
        // (C# `StateSnapshot.AddLocalStateRoot` + snapshot commit), so
        // a reader can never observe the new root record without the
        // trie nodes that back it.
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
        // Release the batch's base Arc before publishing so that, with
        // no reader snapshots outstanding, `make_mut` updates in place
        // instead of cloning the map.
        drop(batch);

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
        Ok(new_root)
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
mod tests {
    use super::*;

    /// `0xf0`, the MPT-node key prefix used by `neo_crypto::mpt_trie`
    /// and the C# `Cache`.
    const NODE_PREFIX: u8 = 0xf0;

    fn storage_key(id: i32, suffix: &[u8]) -> Vec<u8> {
        let mut key = id.to_le_bytes().to_vec();
        key.extend_from_slice(suffix);
        key
    }

    fn put(id: i32, suffix: &[u8], value: &[u8]) -> MptChange {
        MptChange::Put {
            key: storage_key(id, suffix),
            value: value.to_vec(),
        }
    }

    fn delete(id: i32, suffix: &[u8]) -> MptChange {
        MptChange::Delete {
            key: storage_key(id, suffix),
        }
    }

    fn two_block_store(full_state: bool) -> (Arc<MptStore>, UInt256, UInt256) {
        let store = Arc::new(MptStore::new(full_state));
        let root1 = store
            .apply_block_changes(
                1,
                None,
                &[
                    put(5, &[0xAA, 0x01], b"v1"),
                    put(5, &[0xAA, 0x02], b"v2"),
                    put(5, &[0xBB, 0x01], b"other"),
                ],
            )
            .expect("block 1 applies");
        let root2 = store
            .apply_block_changes(
                2,
                Some(root1),
                &[
                    put(5, &[0xAA, 0x01], b"v1-updated"),
                    put(5, &[0xAA, 0x03], b"v3"),
                    delete(5, &[0xAA, 0x02]),
                ],
            )
            .expect("block 2 applies");
        (store, root1, root2)
    }

    #[test]
    fn apply_block_changes_advances_root_and_records() {
        let (store, root1, root2) = two_block_store(true);
        assert_ne!(root1, root2);

        // Per-block records under the C# key scheme.
        let record1 = store.get_state_root(1).expect("block 1 record");
        assert_eq!(*record1.root_hash(), root1);
        assert_eq!(record1.index(), 1);
        let record2 = store.get_state_root(2).expect("block 2 record");
        assert_eq!(*record2.root_hash(), root2);

        assert_eq!(store.current_local_root_index(), Some(2));
        assert_eq!(store.current_local_root_hash(), Some(root2));
        assert!(store.get_state_root(3).is_none());
    }

    #[test]
    fn kv_layout_matches_csharp_key_scheme() {
        let (store, _root1, root2) = two_block_store(true);
        let kv = store.kv.read();

        // 0x01 || u32 BE -> state-root record (C# Keys.StateRoot).
        let record = kv
            .get(&[0x01, 0, 0, 0, 2][..])
            .expect("state-root record for block 2");
        assert_eq!(record[0], crate::state_root::CURRENT_VERSION);
        assert_eq!(&record[1..5], &2u32.to_le_bytes());
        assert_eq!(&record[5..37], &root2.to_bytes());
        // Unwitnessed local root: a single var-int 0 witness count.
        assert_eq!(&record[37..], &[0x00]);

        // 0x02 -> current local root index, little-endian u32.
        assert_eq!(
            kv.get(&[0x02][..]).map(Vec::as_slice),
            Some(&2u32.to_le_bytes()[..])
        );

        // MPT nodes live under 0xf0 || node hash.
        assert!(
            kv.keys()
                .any(|key| key.len() == 33 && key[0] == NODE_PREFIX),
            "trie nodes must be persisted under the 0xf0 prefix"
        );
    }

    #[test]
    fn full_state_serves_both_historical_roots() {
        let (store, root1, root2) = two_block_store(true);

        let mut trie1 = store.open_trie(Some(root1));
        assert_eq!(
            trie1.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
            Some(b"v1".to_vec())
        );
        assert_eq!(
            trie1.get(&storage_key(5, &[0xAA, 0x02])).expect("get"),
            Some(b"v2".to_vec())
        );
        assert_eq!(trie1.get(&storage_key(5, &[0xAA, 0x03])).expect("get"), None);

        let mut trie2 = store.open_trie(Some(root2));
        assert_eq!(
            trie2.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
            Some(b"v1-updated".to_vec())
        );
        assert_eq!(trie2.get(&storage_key(5, &[0xAA, 0x02])).expect("get"), None);
        assert_eq!(
            trie2.get(&storage_key(5, &[0xAA, 0x03])).expect("get"),
            Some(b"v3".to_vec())
        );
    }

    #[test]
    fn pruning_mode_keeps_only_current_root() {
        let (store, root1, root2) = two_block_store(false);

        // The current root stays fully resolvable.
        let mut trie2 = store.open_trie(Some(root2));
        assert_eq!(
            trie2.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
            Some(b"v1-updated".to_vec())
        );

        // Nodes superseded by block 2 were pruned, so the old root can
        // no longer resolve the rewritten path.
        let mut trie1 = store.open_trie(Some(root1));
        assert!(
            trie1.get(&storage_key(5, &[0xAA, 0x01])).is_err(),
            "block-1 path through pruned nodes must fail to resolve"
        );
    }

    #[test]
    fn proof_round_trips_through_verify() {
        let (store, _root1, root2) = two_block_store(true);
        let key = storage_key(5, &[0xAA, 0x03]);

        let mut trie = store.open_trie(Some(root2));
        let proof = trie
            .try_get_proof(&key)
            .expect("proof query")
            .expect("proof exists");
        let value =
            Trie::<MptReadSnapshot>::verify_proof(root2, &key, &proof).expect("proof verifies");
        assert_eq!(value, b"v3".to_vec());
    }

    #[test]
    fn deterministic_roots_across_stores() {
        let (_, root1_a, root2_a) = two_block_store(true);
        let (_, root1_b, root2_b) = two_block_store(true);
        assert_eq!(root1_a, root1_b);
        assert_eq!(root2_a, root2_b);
        // Insertion into the trie is order-independent for a set of
        // distinct keys, and pruning mode must not change the root.
        let (_, root1_c, root2_c) = two_block_store(false);
        assert_eq!(root1_a, root1_c);
        assert_eq!(root2_a, root2_c);
    }

    #[test]
    fn empty_change_set_preserves_root() {
        let (store, _root1, root2) = two_block_store(true);
        let root3 = store
            .apply_block_changes(3, Some(root2), &[])
            .expect("empty block applies");
        assert_eq!(root3, root2);
        assert_eq!(store.current_local_root_index(), Some(3));
        assert_eq!(store.current_local_root_hash(), Some(root2));
    }

    /// Cross-pinned against the official C# implementation: the vector
    /// below was produced by the published `Neo.Cryptography.MPT` 3.9.2
    /// package (the NuGet build of `Neo.Cryptography.MPTTrie`, compiled
    /// against `Neo` 3.9.1 — the reference version vendored under
    /// `neo_csharp/`; the MPTTrie project itself is not vendored).
    /// A `MemoryStore`-backed `Trie` applied exactly the
    /// [`two_block_store`] change sets and dumped `Trie.Root.Hash`
    /// after each block plus `Trie.TryGetProof` for `(5, 0xAA03)`
    /// under the block-2 root.
    #[test]
    fn roots_and_proof_match_csharp_reference_vector() {
        const CSHARP_ROOT1: &str =
            "0xe70c4472181cd5be21ca64895f59c36556d6c4da225e261fb0c9cba9dd23b13a";
        const CSHARP_ROOT2: &str =
            "0x0d62b916694bab0b56983f65ef2cf175851029905698db802de1727861f7b338";
        const CSHARP_PROOF_NODES: [&str; 5] = [
            "0004033e0c12347ed09c36b37110a651a9711a8896b5fc318f2205b15d21d7948cd3e404034f77a7c5f2a8bb02df3a9e0ca5357e2748ba585e9f5df589e89d15c9afadebe504040404040404040404040404",
            "0004040404040404040404036c7955a84481137bb548b266e4b840675691dda220ebcc44625b491516f32639039b7f19f121765d6b6e80128cb6273503af1042384b119b89e85d7628924df29e0404040404",
            "01020a0003eb4cad27f7ba97ade5e2119a0464e4047823e85caae54fb4fff3e1addc07751d",
            "0108000500000000000003f5d20f5a796a7c89ee22dadf440cc1180b853cc167576636e096af9649bc629e",
            "02027633",
        ];

        let (store, root1, root2) = two_block_store(true);
        assert_eq!(
            root1,
            UInt256::parse(CSHARP_ROOT1).expect("pinned root1 parses"),
            "block-1 root must match the C# reference"
        );
        assert_eq!(
            root2,
            UInt256::parse(CSHARP_ROOT2).expect("pinned root2 parses"),
            "block-2 root must match the C# reference"
        );

        // The C#-emitted proof verifies through the Rust verifier and
        // yields the C#-verified value.
        let key = storage_key(5, &[0xAA, 0x03]);
        let csharp_proof: std::collections::HashSet<Vec<u8>> = CSHARP_PROOF_NODES
            .iter()
            .map(|node| hex::decode(node).expect("pinned node hex"))
            .collect();
        let value = Trie::<MptReadSnapshot>::verify_proof(root2, &key, &csharp_proof)
            .expect("C# proof verifies in Rust");
        assert_eq!(value, b"v3".to_vec());

        // And the Rust prover emits the identical node set.
        let mut trie = store.open_trie(Some(root2));
        let rust_proof = trie
            .try_get_proof(&key)
            .expect("proof query")
            .expect("proof exists");
        let mut rust_nodes: Vec<String> = rust_proof.iter().map(hex::encode).collect();
        rust_nodes.sort_unstable();
        assert_eq!(rust_nodes, CSHARP_PROOF_NODES, "proof node set must match");
    }

    #[test]
    fn snapshot_preserves_pruned_generation_for_in_flight_readers() {
        // Pruning mode: applying a block deletes the nodes the change
        // set superseded. A snapshot taken before the apply must keep
        // resolving the old root (the C# immutable store snapshot).
        let store = Arc::new(MptStore::new(false));
        let key = storage_key(5, &[0xAA, 0x01]);
        let root1 = store
            .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
            .expect("block 1 applies");

        let snapshot = store.snapshot();
        assert_eq!(snapshot.current_local_root_index(), Some(1));
        assert_eq!(snapshot.current_local_root_hash(), Some(root1));

        let root2 = store
            .apply_block_changes(2, Some(root1), &[put(5, &[0xAA, 0x01], b"v2")])
            .expect("block 2 applies");
        assert_ne!(root1, root2);

        // The frozen view still serves the pre-apply generation...
        let mut snapshot_trie = snapshot.open_trie(Some(root1));
        assert_eq!(
            snapshot_trie.get(&key).expect("snapshot read"),
            Some(b"v1".to_vec()),
            "snapshot must keep resolving the generation it captured"
        );
        // ...and still reports the index/hash it captured.
        assert_eq!(snapshot.current_local_root_index(), Some(1));

        // While the live store has pruned the superseded nodes.
        let mut live_trie = store.open_trie(Some(root1));
        assert!(
            live_trie.get(&key).is_err(),
            "live store must have pruned the block-1 path"
        );
        let mut current_trie = store.open_trie(Some(root2));
        assert_eq!(
            current_trie.get(&key).expect("current read"),
            Some(b"v2".to_vec())
        );
    }

    #[test]
    fn read_snapshot_rejects_writes() {
        let (store, _root1, root2) = two_block_store(true);
        let snapshot = store.snapshot();

        // Direct store-surface writes are refused...
        assert!(MptStoreSnapshot::put(&*snapshot, vec![0x01], vec![0x02]).is_err());
        assert!(MptStoreSnapshot::delete(&*snapshot, vec![0x01]).is_err());

        // ...and a trie commit over the snapshot fails instead of
        // silently mutating a frozen view.
        let mut trie = snapshot.open_trie(Some(root2));
        trie.put(&storage_key(5, &[0xEE]), b"nope")
            .expect("puts stage in the trie cache");
        assert!(trie.commit().is_err(), "snapshot commit must be rejected");
    }

    #[test]
    fn concurrent_apply_and_snapshot_reads_stay_consistent() {
        // Writer applies pruning-mode blocks that rewrite the same key
        // set each time; readers snapshot, then verify every key under
        // the snapshot's own current root carries that block's value.
        // Without snapshot isolation the pruning writer deletes nodes
        // out from under the readers' walks.
        const BLOCKS: u32 = 50;
        const KEYS: u8 = 16;

        let store = Arc::new(MptStore::new(false));
        let value_for = |block: u32| block.to_le_bytes().to_vec();

        let writer = {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                let mut root = None;
                for block in 1..=BLOCKS {
                    let changes: Vec<MptChange> = (0..KEYS)
                        .map(|i| put(5, &[0xAA, i], &value_for(block)))
                        .collect();
                    let new_root = store
                        .apply_block_changes(block, root, &changes)
                        .expect("block applies");
                    root = Some(new_root);
                }
            })
        };

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || {
                    let mut observed = 0u32;
                    while observed < BLOCKS {
                        let snapshot = store.snapshot();
                        let Some(index) = snapshot.current_local_root_index() else {
                            std::thread::yield_now();
                            continue;
                        };
                        let root = *snapshot
                            .get_state_root(index)
                            .expect("snapshot has its own root record")
                            .root_hash();
                        let mut trie = snapshot.open_trie(Some(root));
                        for i in 0..KEYS {
                            let value = trie
                                .get(&storage_key(5, &[0xAA, i]))
                                .expect("snapshot walk must never lose nodes")
                                .expect("key present in every block");
                            assert_eq!(
                                value,
                                value_for(index),
                                "all keys in one snapshot must carry the same block's value"
                            );
                        }
                        observed = observed.max(index);
                    }
                })
            })
            .collect();

        writer.join().expect("writer thread");
        for reader in readers {
            reader.join().expect("reader thread");
        }

        assert_eq!(store.current_local_root_index(), Some(BLOCKS));
    }

    #[test]
    fn find_enumerates_prefix_in_order() {
        let (store, _root1, root2) = two_block_store(true);
        let mut trie = store.open_trie(Some(root2));
        let prefix = storage_key(5, &[0xAA]);
        let entries = trie.find(&prefix, None).expect("find");
        let keys: Vec<Vec<u8>> = entries.iter().map(|e| e.key.clone()).collect();
        assert_eq!(
            keys,
            vec![
                storage_key(5, &[0xAA, 0x01]),
                storage_key(5, &[0xAA, 0x03]),
            ]
        );

        // Resume strictly after the first key.
        let entries = trie
            .find(&prefix, Some(&storage_key(5, &[0xAA, 0x01])))
            .expect("find with from");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, storage_key(5, &[0xAA, 0x03]));
        assert_eq!(entries[0].value, b"v3".to_vec());
    }
}
