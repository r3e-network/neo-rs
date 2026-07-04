use super::*;
use neo_storage::persistence::store::OnNewSnapshotDelegate;
use neo_storage::persistence::{
    RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric, SeekDirection, Store, StoreSnapshot,
    WriteStore,
};
use neo_storage::types::{StorageItem, StorageKey};
use parking_lot::Mutex as ParkingMutex;

#[derive(Debug)]
struct BorrowOnlySnapshot {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for BorrowOnlySnapshot {
    fn try_get(&self, _key: &Vec<u8>) -> Option<Vec<u8>> {
        panic!("MPT backing reads must use try_get_bytes, not owned Vec lookup")
    }

    fn find(
        &self,
        _key_prefix: Option<&Vec<u8>>,
        _direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        Box::new(std::iter::empty())
    }
}

impl RawReadOnlyStore for BorrowOnlySnapshot {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        (key == self.key).then(|| self.value.clone())
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for BorrowOnlySnapshot {
    fn delete(&mut self, _key: Vec<u8>) -> neo_storage::StorageResult<()> {
        Ok(())
    }

    fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> neo_storage::StorageResult<()> {
        Ok(())
    }
}

impl StoreSnapshot for BorrowOnlySnapshot {
    fn store(&self) -> Arc<dyn Store> {
        panic!("test snapshot is never committed")
    }

    fn try_commit(&mut self) -> Result<(), neo_storage::StorageError> {
        Ok(())
    }
}

struct RecordingRawOverlayStore {
    inner: MemoryStore,
    entries: ParkingMutex<Vec<(Vec<u8>, Option<Vec<u8>>)>>,
    commit_count: ParkingMutex<usize>,
}

impl std::fmt::Debug for RecordingRawOverlayStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingRawOverlayStore").finish_non_exhaustive()
    }
}

impl RecordingRawOverlayStore {
    fn new() -> Self {
        Self {
            inner: MemoryStore::new(),
            entries: ParkingMutex::new(Vec::new()),
            commit_count: ParkingMutex::new(0),
        }
    }

    fn entries(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        self.entries.lock().clone()
    }

    fn commit_count(&self) -> usize {
        *self.commit_count.lock()
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RecordingRawOverlayStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        self.inner.find(key_prefix, direction)
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RecordingRawOverlayStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        self.inner.find(key_prefix, direction)
    }
}

impl RawReadOnlyStore for RecordingRawOverlayStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner.try_get_bytes(key)
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for RecordingRawOverlayStore {
    fn delete(&mut self, key: Vec<u8>) -> neo_storage::StorageResult<()> {
        self.inner.delete(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> neo_storage::StorageResult<()> {
        self.inner.put(key, value)
    }
}

impl ReadOnlyStore for RecordingRawOverlayStore {}

impl Store for RecordingRawOverlayStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        self.inner.snapshot()
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.inner.on_new_snapshot(handler);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_raw_overlay_store(&self) -> Option<&dyn neo_storage::persistence::RawOverlayStore> {
        Some(self)
    }
}

impl neo_storage::persistence::RawOverlayStore for RecordingRawOverlayStore {
    fn try_commit_raw_overlay(
        &self,
        _overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> neo_storage::StorageResult<bool> {
        Ok(false)
    }

    fn try_commit_borrowed_raw_overlay(
        &self,
        visit: &mut dyn FnMut(&mut dyn FnMut(&[u8], Option<&[u8]>)),
    ) -> neo_storage::StorageResult<bool> {
        *self.commit_count.lock() += 1;
        let mut batch = std::collections::BTreeMap::new();
        let mut entries = Vec::new();
        let mut sink = |key: &[u8], value: Option<&[u8]>| {
            batch.insert(key.to_vec(), value.map(<[u8]>::to_vec));
            entries.push((key.to_vec(), value.map(<[u8]>::to_vec)));
        };
        visit(&mut sink);
        self.inner.apply_batch(&batch);
        self.entries.lock().extend(entries);
        Ok(true)
    }
}

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

fn node_entry_count(store: &MptStore) -> usize {
    store
        .kv
        .read()
        .keys()
        .filter(|key| key.len() == 33 && key[0] == NODE_PREFIX)
        .count()
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
fn mpt_store_durable_constructor_surface_is_provider_neutral() {
    let source = include_str!("../../storage/mpt_store.rs");

    assert!(
        !source.contains("fn from_rocksdb_store"),
        "MptStore should accept durable backends through Arc<dyn Store>, not a RocksDB-specific constructor"
    );
}

#[test]
fn write_batch_overlay_is_hash_backed_for_exact_key_staging() {
    let mut base = std::collections::HashMap::new();
    base.insert(vec![0xAA], Some(vec![0x01]));
    let batch = MptWriteBatch::new(Arc::new(base), None, 0);

    assert!(
        !batch.overlay_contains_entries(),
        "fresh write batches should not force reads through the overlay"
    );
    assert_eq!(
        batch.try_get(&[0xAA]).expect("read base before staging"),
        Some(vec![0x01])
    );

    MptStoreSnapshot::put(&batch, vec![0xAA], vec![0x02]).expect("stage override");
    assert!(
        batch.overlay_contains_entries(),
        "staged writes should enable overlay reads"
    );
    MptStoreSnapshot::put(&batch, vec![0xBB], vec![0x03]).expect("stage insert");
    assert_eq!(
        batch.try_get(&[0xAA]).expect("read staged"),
        Some(vec![0x02])
    );
    assert_eq!(
        batch.try_get(&[0xBB]).expect("read staged"),
        Some(vec![0x03])
    );

    MptStoreSnapshot::delete(&batch, vec![0xAA]).expect("stage delete");
    assert_eq!(batch.try_get(&[0xAA]).expect("read delete"), None);
}

#[test]
fn read_snapshot_generation_is_hash_backed_for_exact_key_reads() {
    let mut map = std::collections::HashMap::new();
    map.insert(vec![0xAA], Some(vec![0x01]));
    let snapshot = MptReadSnapshot {
        map: Arc::new(map),
        backing_snapshot: None,
        full_state: true,
    };

    assert_eq!(
        MptStoreSnapshot::try_get(&snapshot, &[0xAA]).expect("read existing"),
        Some(vec![0x01])
    );
    assert_eq!(
        MptStoreSnapshot::try_get(&snapshot, &[0xBB]).expect("read missing"),
        None
    );
}

#[test]
fn backing_snapshot_mpt_reads_use_borrowed_key_lookup() {
    let backing = Arc::new(BorrowOnlySnapshot {
        key: vec![0xCC],
        value: vec![0xDD],
    });
    let snapshot = MptReadSnapshot {
        map: Arc::new(std::collections::HashMap::new()),
        backing_snapshot: Some(backing.clone()),
        full_state: true,
    };
    assert_eq!(
        MptStoreSnapshot::try_get(&snapshot, &[0xCC]).expect("read from backing"),
        Some(vec![0xDD])
    );

    let batch = MptWriteBatch::new(Arc::new(std::collections::HashMap::new()), Some(backing), 0);
    assert_eq!(
        MptStoreSnapshot::try_get(&batch, &[0xCC]).expect("read from batch backing"),
        Some(vec![0xDD])
    );
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
fn current_local_root_returns_index_and_hash_from_one_generation() {
    let (store, _root1, root2) = two_block_store(true);

    assert_eq!(store.current_local_root(), Some((2, root2)));
}

#[test]
fn kv_layout_matches_csharp_key_scheme() {
    let (store, _root1, root2) = two_block_store(true);
    let kv = store.kv.read();

    // 0x01 || u32 BE -> state-root record (C# Keys.StateRoot).
    let record = kv
        .get(&[0x01, 0, 0, 0, 2][..])
        .and_then(Option::as_deref)
        .expect("state-root record for block 2");
    assert_eq!(record[0], crate::state_root::CURRENT_VERSION);
    assert_eq!(&record[1..5], &2u32.to_le_bytes());
    assert_eq!(&record[5..37], &root2.to_bytes());
    // Unwitnessed local root: a single var-int 0 witness count.
    assert_eq!(&record[37..], &[0x00]);

    // 0x02 -> current local root index, little-endian u32.
    assert_eq!(
        kv.get(&[0x02][..]).and_then(Option::as_deref),
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
    assert_eq!(
        trie1.get(&storage_key(5, &[0xAA, 0x03])).expect("get"),
        None
    );

    let mut trie2 = store.open_trie(Some(root2));
    assert_eq!(
        trie2.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
        Some(b"v1-updated".to_vec())
    );
    assert_eq!(
        trie2.get(&storage_key(5, &[0xAA, 0x02])).expect("get"),
        None
    );
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
    let value = Trie::<MptReadSnapshot>::verify_proof(root2, &key, &proof).expect("proof verifies");
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
fn full_state_and_pruning_current_roots_match_across_branch_rewrites() {
    let full = Arc::new(MptStore::new(true));
    let pruned = Arc::new(MptStore::new(false));
    let blocks: &[(u32, &[MptChange])] = &[
        (
            1,
            &[
                put(5, &[0x12], b"prefix"),
                put(5, &[0x12, 0x30], b"left"),
                put(5, &[0x12, 0x34], b"middle"),
                put(5, &[0x12, 0x35], b"right"),
                put(5, &[0x12, 0x35, 0x01], b"deeper"),
            ],
        ),
        (
            2,
            &[
                delete(5, &[0x12]),
                put(5, &[0x12, 0x34], b"middle-updated"),
                put(5, &[0x12, 0x36], b"new-right"),
            ],
        ),
        (
            3,
            &[
                delete(5, &[0x12, 0x30]),
                delete(5, &[0x12, 0x34]),
                put(5, &[0x12, 0x35], b"right-updated"),
            ],
        ),
        (
            4,
            &[
                delete(5, &[0x12, 0x36]),
                put(5, &[0x12, 0x35, 0x02], b"deeper-sibling"),
            ],
        ),
    ];

    let mut full_root = None;
    let mut pruned_root = None;
    for (index, changes) in blocks {
        full_root = Some(
            full.apply_block_changes(*index, full_root, changes)
                .expect("full-state block applies"),
        );
        pruned_root = Some(
            pruned
                .apply_block_changes(*index, pruned_root, changes)
                .expect("pruning block applies"),
        );
        assert_eq!(
            full_root, pruned_root,
            "full-state and pruning modes must publish the same current root at block {index}"
        );
    }

    let final_root = full_root.expect("final root");
    assert_eq!(pruned_root, Some(final_root));
    assert_eq!(full.current_local_root_hash(), Some(final_root));
    assert_eq!(pruned.current_local_root_hash(), Some(final_root));

    for store in [full, pruned] {
        let mut trie = store.open_trie(Some(final_root));
        assert_eq!(trie.get(&storage_key(5, &[0x12])).expect("read"), None);
        assert_eq!(
            trie.get(&storage_key(5, &[0x12, 0x30])).expect("read"),
            None
        );
        assert_eq!(
            trie.get(&storage_key(5, &[0x12, 0x34])).expect("read"),
            None
        );
        assert_eq!(
            trie.get(&storage_key(5, &[0x12, 0x35])).expect("read"),
            Some(b"right-updated".to_vec())
        );
        assert_eq!(
            trie.get(&storage_key(5, &[0x12, 0x35, 0x01]))
                .expect("read"),
            Some(b"deeper".to_vec())
        );
        assert_eq!(
            trie.get(&storage_key(5, &[0x12, 0x35, 0x02]))
                .expect("read"),
            Some(b"deeper-sibling".to_vec())
        );
        assert_eq!(
            trie.get(&storage_key(5, &[0x12, 0x36])).expect("read"),
            None
        );
    }
}

#[test]
fn backed_store_reopens_local_state_root_records() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store =
        Arc::new(MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store"));
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");

    let reopened = MptStore::from_memory_store(Arc::clone(&backing), true).expect("reopen store");
    assert_eq!(reopened.current_local_root_index(), Some(1));
    assert_eq!(reopened.current_local_root_hash(), Some(root1));
    assert_eq!(
        reopened
            .get_state_root(1)
            .expect("reopened state root")
            .root_hash(),
        &root1
    );
}

#[test]
fn backed_reopen_hydrates_latest_root_cache_for_hot_current_root_reads() {
    use neo_storage::persistence::Store;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let backing = Arc::new(MemoryStore::new());
    let store =
        Arc::new(MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store"));
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");
    let root2 = store
        .apply_block_changes(2, Some(root1), &[put(5, &[0xAA, 0x02], b"v2")])
        .expect("block 2 applies");

    let snapshots = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&snapshots);
    backing.on_new_snapshot(Box::new(move |_, _| {
        seen.fetch_add(1, Ordering::Relaxed);
    }));
    let reopened = MptStore::from_memory_store(Arc::clone(&backing), true).expect("reopen store");
    assert_eq!(
        snapshots.load(Ordering::Relaxed),
        1,
        "reopen should hydrate the latest-root cache from one backing snapshot"
    );

    snapshots.store(0, Ordering::Relaxed);
    assert_eq!(reopened.current_local_root(), Some((2, root2)));
    assert_eq!(
        snapshots.load(Ordering::Relaxed),
        0,
        "hot current-root reads after reopen should use the hydrated cache"
    );
}

#[test]
fn concrete_backing_open_does_not_scan_entire_namespace() {
    let source = include_str!("../../storage/mpt_store.rs");
    let open_body = source
        .split("pub fn from_store")
        .nth(1)
        .and_then(|tail| tail.split("pub fn from_memory_store").next())
        .expect("from_store body source");

    assert!(
        !open_body.contains(".find(None"),
        "StateService MPT reopen must not scan the full durable namespace at startup"
    );
    assert!(
        !open_body.contains(".collect()"),
        "StateService MPT reopen should load persisted entries on demand"
    );
}

#[test]
fn publish_overlay_does_not_rescan_overlay_for_metrics() {
    let source = include_str!("../../storage/mpt_store.rs");
    let publish_body = source
        .split("fn publish_overlay")
        .nth(1)
        .and_then(|tail| tail.split("fn local_root_overlay").next())
        .expect("publish_overlay body source");

    assert!(
        !publish_body.contains("overlay.values()"),
        "MPT publish should count overlay metrics while consuming the overlay, not with a separate values scan"
    );
    assert!(
        !publish_body.contains("for (_, value) in overlay"),
        "durable-backed MPT publish must reuse backing-commit counts instead of scanning the overlay again"
    );
}

#[test]
fn durable_backing_overlay_is_committed_in_key_order() {
    let backing = Arc::new(RecordingRawOverlayStore::new());
    let store = MptStore::from_store(backing.clone(), true).expect("mpt store");

    store
        .apply_block_changes(
            1,
            None,
            &[
                put(5, &[0xCC], b"third"),
                put(5, &[0xAA], b"first"),
                put(5, &[0xBB], b"second"),
            ],
        )
        .expect("block applies");

    let entries = backing.entries();
    let keys = entries
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort();

    assert_eq!(
        keys, sorted,
        "StateService MPT durable backing commits must visit raw keys in byte order"
    );
}

#[test]
fn batch_apply_commits_multiple_state_roots_with_one_backing_overlay() {
    let backing = Arc::new(RecordingRawOverlayStore::new());
    let store = MptStore::from_store(backing.clone(), true).expect("mpt store");
    let blocks = [
        MptBlockChanges {
            block_index: 0,
            changes: &[put(5, &[0xA0], b"v0")],
        },
        MptBlockChanges {
            block_index: 1,
            changes: &[put(5, &[0xA1], b"v1")],
        },
        MptBlockChanges {
            block_index: 2,
            changes: &[put(5, &[0xA2], b"v2")],
        },
    ];

    let roots = store
        .apply_block_changes_batch(None, &blocks)
        .expect("batch applies");

    assert_eq!(roots.len(), 3);
    assert_eq!(
        backing.commit_count(),
        1,
        "queued StateService MPT blocks should share one durable overlay commit"
    );
    assert_eq!(store.current_local_root(), Some((2, roots[2])));
    for (index, root) in roots.iter().enumerate() {
        assert_eq!(
            store
                .get_state_root(index as u32)
                .expect("state root record")
                .root_hash(),
            root
        );
    }
}

#[test]
fn batch_apply_matches_sequential_roots_and_reads() {
    let sequential = MptStore::new(true);
    let batched = MptStore::new(true);
    let changes0 = vec![
        put(5, &[0xA0], b"v0"),
        put(5, &[0xA1], b"v1"),
        put(7, &[0xB0], b"other"),
    ];
    let changes1 = vec![
        put(5, &[0xA0], b"v0-updated"),
        delete(5, &[0xA1]),
        put(5, &[0xA2], b"v2"),
    ];
    let changes2 = Vec::new();
    let changes3 = vec![put(7, &[0xB0], b"other-updated")];
    let block_changes = [
        (0, changes0.as_slice()),
        (1, changes1.as_slice()),
        (2, changes2.as_slice()),
        (3, changes3.as_slice()),
    ];

    let mut sequential_roots = Vec::new();
    let mut previous = None;
    for (index, changes) in block_changes {
        let root = sequential
            .apply_block_changes(index, previous, changes)
            .expect("sequential block applies");
        previous = Some(root);
        sequential_roots.push(root);
    }

    let batched_roots = batched
        .apply_block_changes_batch(
            None,
            &[
                MptBlockChanges {
                    block_index: 0,
                    changes: &changes0,
                },
                MptBlockChanges {
                    block_index: 1,
                    changes: &changes1,
                },
                MptBlockChanges {
                    block_index: 2,
                    changes: &changes2,
                },
                MptBlockChanges {
                    block_index: 3,
                    changes: &changes3,
                },
            ],
        )
        .expect("batch applies");

    assert_eq!(batched_roots, sequential_roots);
    assert_eq!(
        batched.current_local_root(),
        sequential.current_local_root()
    );
    for (index, expected_root) in sequential_roots.iter().enumerate() {
        assert_eq!(
            batched
                .get_state_root(index as u32)
                .expect("batched state root record")
                .root_hash(),
            expected_root
        );
    }

    let final_root = *batched_roots.last().expect("final root");
    let mut trie = batched.open_trie(Some(final_root));
    assert_eq!(
        trie.get(&storage_key(5, &[0xA0])).expect("read updated"),
        Some(b"v0-updated".to_vec())
    );
    assert_eq!(
        trie.get(&storage_key(5, &[0xA1])).expect("read deleted"),
        None
    );
    assert_eq!(
        trie.get(&storage_key(5, &[0xA2])).expect("read inserted"),
        Some(b"v2".to_vec())
    );
    assert_eq!(
        trie.get(&storage_key(7, &[0xB0]))
            .expect("read updated other"),
        Some(b"other-updated".to_vec())
    );
}

#[test]
fn batch_apply_rejects_non_contiguous_or_invalid_start_without_advancing_root() {
    let missing_genesis = MptStore::new(true);
    let err = missing_genesis
        .apply_block_changes_batch(
            None,
            &[MptBlockChanges {
                block_index: 5,
                changes: &[put(5, &[0xA0], b"v0")],
            }],
        )
        .expect_err("empty store cannot batch-start after genesis");
    assert!(err.to_string().contains("non-contiguous"));
    assert_eq!(missing_genesis.current_local_root(), None);

    let skipped = MptStore::new(true);
    let err = skipped
        .apply_block_changes_batch(
            None,
            &[
                MptBlockChanges {
                    block_index: 0,
                    changes: &[put(5, &[0xA0], b"v0")],
                },
                MptBlockChanges {
                    block_index: 2,
                    changes: &[put(5, &[0xA2], b"v2")],
                },
            ],
        )
        .expect_err("batch cannot skip a block height");
    assert!(err.to_string().contains("non-contiguous"));
    assert_eq!(skipped.current_local_root(), None);

    let reversed = MptStore::new(true);
    let err = reversed
        .apply_block_changes_batch(
            None,
            &[
                MptBlockChanges {
                    block_index: 2,
                    changes: &[put(5, &[0xA2], b"v2")],
                },
                MptBlockChanges {
                    block_index: 1,
                    changes: &[put(5, &[0xA1], b"v1")],
                },
            ],
        )
        .expect_err("batch cannot move backward");
    assert!(err.to_string().contains("non-contiguous"));
    assert_eq!(reversed.current_local_root(), None);
}

#[test]
fn batch_apply_pruning_mode_matches_sequential_current_root_and_pruning() {
    let sequential = MptStore::new(false);
    let batched = MptStore::new(false);
    let changes0 = vec![
        put(5, &[0xA0], b"v0"),
        put(5, &[0xA1], b"v1"),
        put(7, &[0xB0], b"other"),
    ];
    let changes1 = vec![
        put(5, &[0xA0], b"v0-updated"),
        delete(5, &[0xA1]),
        put(5, &[0xA2], b"v2"),
    ];
    let changes2 = Vec::new();
    let changes3 = vec![put(7, &[0xB0], b"other-updated")];

    let mut sequential_roots = Vec::new();
    let mut previous = None;
    for (index, changes) in [
        (0, changes0.as_slice()),
        (1, changes1.as_slice()),
        (2, changes2.as_slice()),
        (3, changes3.as_slice()),
    ] {
        let root = sequential
            .apply_block_changes(index, previous, changes)
            .expect("sequential pruning block applies");
        previous = Some(root);
        sequential_roots.push(root);
    }

    let batched_roots = batched
        .apply_block_changes_batch(
            None,
            &[
                MptBlockChanges {
                    block_index: 0,
                    changes: &changes0,
                },
                MptBlockChanges {
                    block_index: 1,
                    changes: &changes1,
                },
                MptBlockChanges {
                    block_index: 2,
                    changes: &changes2,
                },
                MptBlockChanges {
                    block_index: 3,
                    changes: &changes3,
                },
            ],
        )
        .expect("batched pruning blocks apply");

    assert_eq!(batched_roots, sequential_roots);
    assert_eq!(
        batched.current_local_root(),
        sequential.current_local_root()
    );
    let final_root = *batched_roots.last().expect("final root");
    for store in [&sequential, &batched] {
        let mut trie = store.open_trie(Some(final_root));
        assert_eq!(
            trie.get(&storage_key(5, &[0xA0])).expect("read updated"),
            Some(b"v0-updated".to_vec())
        );
        assert_eq!(
            trie.get(&storage_key(5, &[0xA1])).expect("read deleted"),
            None
        );
        assert_eq!(
            trie.get(&storage_key(5, &[0xA2])).expect("read inserted"),
            Some(b"v2".to_vec())
        );
        assert_eq!(
            trie.get(&storage_key(7, &[0xB0]))
                .expect("read updated other"),
            Some(b"other-updated".to_vec())
        );
        let old_root = sequential_roots[0];
        let mut old_trie = store.open_trie(Some(old_root));
        assert!(
            old_trie.get(&storage_key(5, &[0xA0])).is_err(),
            "pruning mode should not keep the first batch root live after later roots"
        );
    }
}

#[test]
fn batch_apply_durable_full_state_reopens_all_historical_roots() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store =
        Arc::new(MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store"));
    let changes0 = vec![put(5, &[0xA0], b"v0"), put(5, &[0xA1], b"v1")];
    let changes1 = vec![put(5, &[0xA0], b"v0-updated")];
    let changes2 = vec![delete(5, &[0xA1]), put(7, &[0xB0], b"other")];

    let roots = store
        .apply_block_changes_batch(
            None,
            &[
                MptBlockChanges {
                    block_index: 0,
                    changes: &changes0,
                },
                MptBlockChanges {
                    block_index: 1,
                    changes: &changes1,
                },
                MptBlockChanges {
                    block_index: 2,
                    changes: &changes2,
                },
            ],
        )
        .expect("durable full-state batch applies");

    let reopened = MptStore::from_memory_store(Arc::clone(&backing), true).expect("reopen store");
    assert_eq!(reopened.current_local_root(), Some((2, roots[2])));
    for (index, root) in roots.iter().enumerate() {
        assert_eq!(
            reopened
                .get_state_root(index as u32)
                .expect("reopened state root")
                .root_hash(),
            root
        );
    }

    let mut trie0 = reopened.open_trie(Some(roots[0]));
    assert_eq!(
        trie0.get(&storage_key(5, &[0xA0])).expect("root0 read"),
        Some(b"v0".to_vec())
    );
    assert_eq!(
        trie0.get(&storage_key(5, &[0xA1])).expect("root0 read"),
        Some(b"v1".to_vec())
    );

    let mut trie1 = reopened.open_trie(Some(roots[1]));
    assert_eq!(
        trie1.get(&storage_key(5, &[0xA0])).expect("root1 read"),
        Some(b"v0-updated".to_vec())
    );
    assert_eq!(
        trie1.get(&storage_key(5, &[0xA1])).expect("root1 read"),
        Some(b"v1".to_vec())
    );

    let mut trie2 = reopened.open_trie(Some(roots[2]));
    assert_eq!(
        trie2.get(&storage_key(5, &[0xA0])).expect("root2 read"),
        Some(b"v0-updated".to_vec())
    );
    assert_eq!(
        trie2.get(&storage_key(5, &[0xA1])).expect("root2 read"),
        None
    );
    assert_eq!(
        trie2.get(&storage_key(7, &[0xB0])).expect("root2 read"),
        Some(b"other".to_vec())
    );
}

#[test]
fn all_empty_batch_after_known_root_bypasses_trie_read_snapshot() {
    use neo_storage::persistence::Store;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let backing = Arc::new(RecordingRawOverlayStore::new());
    let snapshots = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&snapshots);
    backing.on_new_snapshot(Box::new(move |_, _| {
        seen.fetch_add(1, Ordering::Relaxed);
    }));

    let store = MptStore::from_store(backing.clone(), true).expect("open store");
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");
    let previous_entry_count = backing.entries().len();

    let blocks = [
        MptBlockChanges {
            block_index: 2,
            changes: &[],
        },
        MptBlockChanges {
            block_index: 3,
            changes: &[],
        },
        MptBlockChanges {
            block_index: 4,
            changes: &[],
        },
    ];

    snapshots.store(0, Ordering::Relaxed);
    let roots = store
        .apply_block_changes_batch(Some(root1), &blocks)
        .expect("empty batch applies");

    assert_eq!(roots, vec![root1, root1, root1]);
    assert_eq!(
        snapshots.load(Ordering::Relaxed),
        0,
        "known-empty continuation batches should not open trie/backing snapshots"
    );
    assert_eq!(
        backing.commit_count(),
        2,
        "initial non-empty block and empty batch should commit through raw overlay"
    );
    let entries = backing.entries();
    let empty_batch_entries = &entries[previous_entry_count..];
    assert_eq!(
        empty_batch_entries.len(),
        4,
        "three empty state-root records plus one current-index record should be committed"
    );
    assert_eq!(empty_batch_entries[0].0, Keys::state_root(2));
    assert_eq!(empty_batch_entries[1].0, Keys::state_root(3));
    assert_eq!(empty_batch_entries[2].0, Keys::state_root(4));
    assert_eq!(
        empty_batch_entries[3].0,
        Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec()
    );
    let mut sorted = empty_batch_entries
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    sorted.sort();
    assert_eq!(
        empty_batch_entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>(),
        sorted,
        "empty continuation batches should visit durable backing keys in byte order without a sort pass"
    );
    assert_eq!(store.current_local_root(), Some((4, root1)));
    for index in 2..=4 {
        assert_eq!(
            store
                .get_state_root(index)
                .expect("empty-batch state root record")
                .root_hash(),
            &root1
        );
    }
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
fn initial_empty_change_set_uses_normal_commit_path_for_resolvable_root() {
    let store = MptStore::new(true);

    let root0 = store
        .apply_block_changes(0, None, &[])
        .expect("initial empty block applies");

    assert_eq!(store.current_local_root_index(), Some(0));
    assert_eq!(store.current_local_root_hash(), Some(root0));
    assert_eq!(
        store
            .get_state_root(0)
            .expect("block 0 root record")
            .root_hash(),
        &root0
    );

    let mut trie = store.open_trie(Some(root0));
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA])).expect("empty trie read"),
        None
    );
}

#[test]
fn empty_change_set_skips_trie_commit_and_publishes_only_root_records() {
    let (store, _root1, root2) = two_block_store(true);
    let nodes_before = node_entry_count(&store);
    let kv_entries_before = store.kv.read().len();

    let root3 = store
        .apply_block_changes(3, Some(root2), &[])
        .expect("empty block applies");

    assert_eq!(root3, root2);
    assert_eq!(store.current_local_root_index(), Some(3));
    assert_eq!(store.current_local_root_hash(), Some(root2));
    assert_eq!(
        store
            .get_state_root(3)
            .expect("block 3 root record")
            .root_hash(),
        &root2
    );
    assert_eq!(
        node_entry_count(&store),
        nodes_before,
        "empty blocks must not publish new trie-node records"
    );
    assert_eq!(
        store.kv.read().len(),
        kv_entries_before + 1,
        "empty blocks should add only the per-block state-root record; the current-index record is overwritten"
    );
}

#[test]
fn known_empty_change_set_bypasses_trie_read_snapshot() {
    use neo_storage::persistence::Store;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let backing = Arc::new(MemoryStore::new());
    let snapshots = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&snapshots);
    backing.on_new_snapshot(Box::new(move |_, _| {
        seen.fetch_add(1, Ordering::Relaxed);
    }));

    let store = MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store");
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");

    snapshots.store(0, Ordering::Relaxed);
    let root2 = store
        .apply_block_changes(2, Some(root1), &[])
        .expect("empty block applies");

    assert_eq!(root2, root1);
    assert_eq!(
        snapshots.load(Ordering::Relaxed),
        1,
        "known-empty continuation blocks should only snapshot backing for local-root commit"
    );
}

#[test]
fn empty_change_set_reopens_from_backing_store() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store =
        Arc::new(MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store"));
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");
    let root2 = store
        .apply_block_changes(2, Some(root1), &[])
        .expect("empty block 2 applies");

    assert_eq!(root2, root1);

    let reopened = MptStore::from_memory_store(Arc::clone(&backing), true).expect("reopen store");
    assert_eq!(reopened.current_local_root_index(), Some(2));
    assert_eq!(reopened.current_local_root_hash(), Some(root1));
    assert_eq!(
        reopened
            .get_state_root(2)
            .expect("reopened empty-block state root")
            .root_hash(),
        &root1
    );
    let mut trie = reopened.open_trie(Some(root1));
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA, 0x01]))
            .expect("reopened trie read"),
        Some(b"v1".to_vec())
    );
}

#[test]
fn reopened_backed_full_state_serves_historical_roots() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store =
        Arc::new(MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store"));
    let root1 = store
        .apply_block_changes(
            1,
            None,
            &[put(5, &[0xAA, 0x01], b"v1"), put(5, &[0xAA, 0x02], b"v2")],
        )
        .expect("block 1 applies");
    let root2 = store
        .apply_block_changes(2, Some(root1), &[put(5, &[0xAA, 0x01], b"v1-updated")])
        .expect("block 2 applies");

    let reopened = MptStore::from_memory_store(Arc::clone(&backing), true).expect("reopen store");
    let mut trie1 = reopened.open_trie(Some(root1));
    assert_eq!(
        trie1.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
        Some(b"v1".to_vec())
    );
    let mut trie2 = reopened.open_trie(Some(root2));
    assert_eq!(
        trie2.get(&storage_key(5, &[0xAA, 0x01])).expect("get"),
        Some(b"v1-updated".to_vec())
    );
}

#[test]
fn reopened_backed_snapshot_preserves_old_root_during_pruning_apply() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let original =
        Arc::new(MptStore::from_memory_store(Arc::clone(&backing), false).expect("open store"));
    let key = storage_key(5, &[0xAA, 0x01]);
    let root1 = original
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");

    let reopened = MptStore::from_memory_store(Arc::clone(&backing), false).expect("reopen store");
    let snapshot = reopened.snapshot();
    assert_eq!(snapshot.current_local_root_hash(), Some(root1));

    let root2 = reopened
        .apply_block_changes(2, Some(root1), &[put(5, &[0xAA, 0x01], b"v2")])
        .expect("block 2 applies");
    assert_ne!(root1, root2);

    let mut snapshot_trie = snapshot.open_trie(Some(root1));
    assert_eq!(
        snapshot_trie.get(&key).expect("snapshot read"),
        Some(b"v1".to_vec()),
        "reopened durable snapshots must preserve old pruning generations"
    );

    let mut current_trie = reopened.open_trie(Some(root2));
    assert_eq!(
        current_trie.get(&key).expect("current read"),
        Some(b"v2".to_vec())
    );
}

#[test]
fn reopened_backed_store_proof_and_find_use_lazy_snapshot_reads() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store =
        Arc::new(MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store"));
    let root1 = store
        .apply_block_changes(
            1,
            None,
            &[
                put(5, &[0xAA, 0x01], b"v1"),
                put(5, &[0xAA, 0x02], b"v2"),
                put(5, &[0xAA, 0x03], b"v3"),
            ],
        )
        .expect("block 1 applies");

    let reopened = MptStore::from_memory_store(Arc::clone(&backing), true).expect("reopen store");
    let key = storage_key(5, &[0xAA, 0x03]);
    let mut trie = reopened.open_trie(Some(root1));
    let proof = trie
        .try_get_proof(&key)
        .expect("proof query")
        .expect("proof exists");
    let value = Trie::<MptReadSnapshot>::verify_proof(root1, &key, &proof).expect("proof verifies");
    assert_eq!(value, b"v3".to_vec());

    let mut trie = reopened.open_trie(Some(root1));
    let entries = trie.find(&storage_key(5, &[0xAA]), None).expect("find");
    let keys: Vec<Vec<u8>> = entries.iter().map(|entry| entry.key.clone()).collect();
    assert_eq!(
        keys,
        vec![
            storage_key(5, &[0xAA, 0x01]),
            storage_key(5, &[0xAA, 0x02]),
            storage_key(5, &[0xAA, 0x03]),
        ]
    );
}

#[test]
fn backed_publish_refreshes_live_generation_after_commit() {
    use neo_storage::persistence::Store;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let backing = Arc::new(MemoryStore::new());
    let snapshots = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&snapshots);
    backing.on_new_snapshot(Box::new(move |_, _| {
        seen.fetch_add(1, Ordering::Relaxed);
    }));

    let store = MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store");
    let root = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");

    snapshots.store(0, Ordering::Relaxed);
    assert_eq!(store.current_local_root(), Some((1, root)));
    assert_eq!(
        snapshots.load(Ordering::Relaxed),
        0,
        "hot current-root reads should use the latest-root cache, not open a backing snapshot"
    );
    assert_eq!(
        store.get_state_root(1).expect("state root").root_hash(),
        &root
    );
    let mut trie = store.open_trie(Some(root));
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA, 0x01]))
            .expect("current trie read"),
        Some(b"v1".to_vec())
    );
}

#[test]
fn backed_publish_drops_durable_overlay_from_live_generation() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store = MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store");
    let root = store
        .apply_block_changes(
            1,
            None,
            &[
                put(5, &[0xAA, 0x01], b"v1"),
                put(5, &[0xAA, 0x02], b"v2"),
                put(5, &[0xAA, 0x03], b"v3"),
            ],
        )
        .expect("block 1 applies");

    assert_eq!(
        store.kv.read().len(),
        0,
        "durable backed stores should not retain committed trie/state-root overlays in RAM"
    );
    assert_eq!(store.current_local_root(), Some((1, root)));
    let mut trie = store.open_trie(Some(root));
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA, 0x03]))
            .expect("read from backing snapshot"),
        Some(b"v3".to_vec())
    );
}

#[test]
fn rocksdb_fast_sync_backing_keeps_buffered_overlay_readable_before_flush() {
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::rocksdb::RocksDBStoreProvider;

    let path = std::env::temp_dir().join(format!(
        "neo-state-service-mpt-fast-sync-buffered-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    let backing = Arc::new(
        RocksDBStoreProvider::new(StorageConfig {
            path: path.clone(),
            ..Default::default()
        })
        .get_rocksdb_store("")
        .expect("open rocksdb"),
    );
    backing.enable_fast_sync_mode();

    let store = MptStore::from_store(backing.clone(), true).expect("open store");
    let root = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");

    assert!(
        !store.kv.read().is_empty(),
        "buffered RocksDB writes must remain in the live generation until flush"
    );
    assert_eq!(store.current_local_root(), Some((1, root)));
    let mut trie = store.open_trie(Some(root));
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA, 0x01]))
            .expect("read buffered write"),
        Some(b"v1".to_vec())
    );

    backing.disable_fast_sync_mode();
    let root2 = store
        .apply_block_changes(2, Some(root), &[put(5, &[0xAA, 0x02], b"v2")])
        .expect("block 2 applies after flush");

    assert_eq!(
        store.kv.read().len(),
        0,
        "after RocksDB fast-sync writes are flushed, new durable overlays should be served lazily from backing snapshots"
    );
    assert_eq!(store.current_local_root(), Some((2, root2)));
    let mut trie = store.open_trie(Some(root2));
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA, 0x01]))
            .expect("read first durable write"),
        Some(b"v1".to_vec())
    );
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA, 0x02]))
            .expect("read second durable write"),
        Some(b"v2".to_vec())
    );

    drop(store);
    drop(backing);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn revert_local_roots_rewinds_current_root_in_full_state_mode() {
    let (store, root1, root2) = two_block_store(true);
    assert_ne!(root1, root2);

    store
        .revert_local_roots(2, 2)
        .expect("full-state revert succeeds");

    assert_eq!(store.current_local_root_index(), Some(1));
    assert_eq!(store.current_local_root_hash(), Some(root1));
    assert!(store.get_state_root(2).is_none());

    let mut trie = store.open_trie(Some(root1));
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA, 0x01]))
            .expect("read root1"),
        Some(b"v1".to_vec())
    );
}

#[test]
fn revert_local_roots_rejects_pruning_mode_rewind() {
    let (store, _root1, root2) = two_block_store(false);
    assert_eq!(store.current_local_root_index(), Some(2));

    let err = store
        .revert_local_roots(2, 2)
        .expect_err("pruning mode cannot safely rewind local roots");

    assert!(
        err.to_string().contains("pruning"),
        "error should explain why rollback is unsafe: {err}"
    );
    assert_eq!(store.current_local_root_index(), Some(2));
    assert_eq!(store.current_local_root_hash(), Some(root2));
    assert!(store.get_state_root(2).is_some());
}

#[test]
fn revert_local_roots_rewinds_current_root_across_empty_block() {
    let (store, _root1, root2) = two_block_store(true);
    let root3 = store
        .apply_block_changes(3, Some(root2), &[])
        .expect("empty block applies");
    assert_eq!(root3, root2);

    store
        .revert_local_roots(3, 3)
        .expect("full-state empty-block revert succeeds");

    assert_eq!(store.current_local_root_index(), Some(2));
    assert_eq!(store.current_local_root_hash(), Some(root2));
    assert!(store.get_state_root(3).is_none());
}

#[test]
fn backed_revert_first_local_root_clears_latest_root_cache() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store = MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store");
    let root0 = store
        .apply_block_changes(0, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 0 applies");
    assert_eq!(store.current_local_root(), Some((0, root0)));

    store
        .revert_local_roots(0, 0)
        .expect("full-state backed revert to genesis succeeds");

    assert_eq!(store.current_local_root(), None);
    assert_eq!(store.current_local_root_index(), None);
    assert_eq!(store.current_local_root_hash(), None);
    assert!(store.get_state_root(0).is_none());
}

#[test]
fn backed_revert_drops_durable_tombstones_from_live_generation() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store = MptStore::from_memory_store(Arc::clone(&backing), true).expect("open store");
    let root1 = store
        .apply_block_changes(1, None, &[put(5, &[0xAA, 0x01], b"v1")])
        .expect("block 1 applies");
    let root2 = store
        .apply_block_changes(2, Some(root1), &[put(5, &[0xAA, 0x02], b"v2")])
        .expect("block 2 applies");
    assert_ne!(root1, root2);
    assert_eq!(store.kv.read().len(), 0);

    store
        .revert_local_roots(2, 2)
        .expect("full-state backed revert succeeds");

    assert_eq!(
        store.kv.read().len(),
        0,
        "durable backed reverts should not retain tombstones in the live generation"
    );
    assert_eq!(store.current_local_root_index(), Some(1));
    assert_eq!(store.current_local_root_hash(), Some(root1));
    assert!(store.get_state_root(2).is_none());

    let mut trie = store.open_trie(Some(root1));
    assert_eq!(
        trie.get(&storage_key(5, &[0xAA, 0x01]))
            .expect("read root1"),
        Some(b"v1".to_vec())
    );
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
    const CSHARP_ROOT1: &str = "0xe70c4472181cd5be21ca64895f59c36556d6c4da225e261fb0c9cba9dd23b13a";
    const CSHARP_ROOT2: &str = "0x0d62b916694bab0b56983f65ef2cf175851029905698db802de1727861f7b338";
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
        vec![storage_key(5, &[0xAA, 0x01]), storage_key(5, &[0xAA, 0x03]),]
    );

    // Resume strictly after the first key.
    let entries = trie
        .find(&prefix, Some(&storage_key(5, &[0xAA, 0x01])))
        .expect("find with from");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, storage_key(5, &[0xAA, 0x03]));
    assert_eq!(entries[0].value, b"v3".to_vec());
}
