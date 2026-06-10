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

use crate::state_root::StateRoot;
use neo_crypto::mpt_trie::{MptResult, MptStoreSnapshot, Trie};
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
/// Thread-safe: reads take a shared lock on the key/value namespace,
/// trie commits and state-root writes go through
/// [`MptStore::apply_block_changes`], which serializes writers behind
/// a dedicated gate (the C# plugin achieves the same single-writer
/// discipline by running `StateStore` as an actor).
pub struct MptStore {
    /// Flat key/value namespace shared by MPT nodes and state-root
    /// records (the C# `IStore` equivalent).
    kv: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
    /// Serializes block-changeset application.
    write_gate: Mutex<()>,
    /// Whether historical trie nodes are retained (C#
    /// `StateServiceSettings.FullState`, default `false`). With
    /// `false`, applying a block prunes the nodes the change set made
    /// unreachable, so only the *current* root stays resolvable.
    full_state: bool,
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
            kv: RwLock::new(BTreeMap::new()),
            write_gate: Mutex::new(()),
            full_state,
        }
    }

    /// Returns whether historical trie versions are retained.
    pub fn full_state(&self) -> bool {
        self.full_state
    }

    /// Opens a read view of the trie rooted at `root` (`None` for the
    /// empty trie). The returned [`Trie`] resolves nodes from this
    /// store; read operations never write back.
    pub fn open_trie(self: &Arc<Self>, root: Option<UInt256>) -> Trie<Self> {
        Trie::new(Arc::clone(self), root, self.full_state)
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
        self: &Arc<Self>,
        block_index: u32,
        root_before: Option<UInt256>,
        changes: &[MptChange],
    ) -> MptResult<UInt256> {
        let _writer = self.write_gate.lock();

        let mut trie = Trie::new(Arc::clone(self), root_before, self.full_state);
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

        let root_record = StateRoot::new_current(block_index, new_root);
        self.put_local_state_root(&root_record);
        Ok(new_root)
    }

    /// Writes a local (unwitnessed) state-root record and advances the
    /// current local root index, mirroring the C#
    /// `StateSnapshot.AddLocalStateRoot`.
    fn put_local_state_root(&self, root: &StateRoot) {
        let mut kv = self.kv.write();
        kv.insert(Keys::state_root(root.index()), Self::encode_state_root(root));
        kv.insert(
            Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
            root.index().to_le_bytes().to_vec(),
        );
    }

    /// Returns the state-root record persisted for `index`, if any.
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        let bytes = self.kv.read().get(&Keys::state_root(index)).cloned()?;
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

    /// Returns the current local root index, if a block has been
    /// applied (C# `StateSnapshot.CurrentLocalRootIndex`).
    pub fn current_local_root_index(&self) -> Option<u32> {
        let bytes = self
            .kv
            .read()
            .get(Keys::CURRENT_LOCAL_ROOT_INDEX)
            .cloned()?;
        let arr: [u8; 4] = bytes.as_slice().try_into().ok()?;
        Some(u32::from_le_bytes(arr))
    }

    /// Returns the current local root hash, if a block has been
    /// applied (C# `StateSnapshot.CurrentLocalRootHash`).
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        let index = self.current_local_root_index()?;
        Some(*self.get_state_root(index)?.root_hash())
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
        // Take each lock-guarded value before formatting so no guard
        // is held across a re-entrant `kv.read()` (parking_lot read
        // locks are not recursive once a writer queues up).
        let entries = self.kv.read().len();
        let local_root_index = self.current_local_root_index();
        f.debug_struct("MptStore")
            .field("entries", &entries)
            .field("full_state", &self.full_state)
            .field("local_root_index", &local_root_index)
            .finish()
    }
}

impl MptStoreSnapshot for MptStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self.kv.read().get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.kv.write().insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.kv.write().remove(&key);
        Ok(())
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
            Trie::<MptStore>::verify_proof(root2, &key, &proof).expect("proof verifies");
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
