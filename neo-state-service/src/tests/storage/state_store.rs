use super::*;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::{
    RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric, SeekDirection, Store, WriteStore,
};
use neo_storage::{StorageItem, StorageKey};
use std::sync::atomic::{AtomicUsize, Ordering};

fn root(index: u32, byte: u8) -> StateRoot {
    StateRoot::new_current(index, UInt256::from([byte; 32]))
}

struct SnapshotCountingRawOverlayStore {
    inner: MemoryStore,
    snapshot_count: AtomicUsize,
    commit_count: AtomicUsize,
}

impl std::fmt::Debug for SnapshotCountingRawOverlayStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotCountingRawOverlayStore")
            .finish_non_exhaustive()
    }
}

impl SnapshotCountingRawOverlayStore {
    fn new() -> Self {
        Self {
            inner: MemoryStore::new(),
            snapshot_count: AtomicUsize::new(0),
            commit_count: AtomicUsize::new(0),
        }
    }

    fn snapshot_count(&self) -> usize {
        self.snapshot_count.load(Ordering::Relaxed)
    }

    fn reset_snapshot_count(&self) {
        self.snapshot_count.store(0, Ordering::Relaxed);
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for SnapshotCountingRawOverlayStore {
    type FindIterator<'a> =
        <MemoryStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        self.inner.find(key_prefix, direction)
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for SnapshotCountingRawOverlayStore {
    type FindIterator<'a> =
        <MemoryStore as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::FindIterator<'a>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        self.inner.find(key_prefix, direction)
    }
}

impl RawReadOnlyStore for SnapshotCountingRawOverlayStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner.try_get_bytes(key)
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for SnapshotCountingRawOverlayStore {
    fn delete(&mut self, key: Vec<u8>) -> neo_storage::StorageResult<()> {
        self.inner.delete(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> neo_storage::StorageResult<()> {
        self.inner.put(key, value)
    }
}

impl ReadOnlyStore for SnapshotCountingRawOverlayStore {}

impl Store for SnapshotCountingRawOverlayStore {
    type Snapshot = <MemoryStore as Store>::Snapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot> {
        self.snapshot_count.fetch_add(1, Ordering::Relaxed);
        self.inner.snapshot()
    }

    fn try_commit_raw_overlay(
        &self,
        _overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> neo_storage::StorageResult<bool> {
        Ok(false)
    }

    fn try_commit_borrowed_raw_overlay<O>(
        &self,
        overlay_source: &mut O,
    ) -> neo_storage::StorageResult<bool>
    where
        O: neo_storage::persistence::RawOverlaySource + ?Sized,
    {
        self.commit_count.fetch_add(1, Ordering::Relaxed);
        let mut batch = std::collections::BTreeMap::new();
        let mut sink = |key: &[u8], value: Option<&[u8]>| {
            batch.insert(key.to_vec(), value.map(<[u8]>::to_vec));
        };
        overlay_source.visit_raw_overlay(&mut sink);
        self.inner.apply_batch(&batch);
        Ok(true)
    }
}

#[test]
fn try_add_and_get_by_index() {
    let store = StateStore::new();
    assert!(store.try_add_state_root(root(1, 0x11)));
    let got = store.get_state_root(StateStoreLookup::ByBlockIndex(1));
    assert!(got.is_some());
}

#[test]
fn try_add_rejects_duplicate_root_hash() {
    let store = StateStore::new();
    let r1 = root(1, 0xAB);
    let r2 = root(2, 0xAB);
    assert!(store.try_add_state_root(r1));
    assert!(!store.try_add_state_root(r2));
}

#[test]
fn commit_moves_candidate_to_validated() {
    let store = StateStore::new();
    let r = root(1, 0xCC);
    assert!(store.try_add_state_root(r.clone()));
    assert_eq!(store.candidate_count(), 1);
    store.commit_validated_state_roots(&[r]);
    assert_eq!(store.candidate_count(), 0);
    assert!(
        store
            .get_state_root(StateStoreLookup::ByBlockIndex(1))
            .is_some()
    );
}

#[test]
fn discard_removes_state_root() {
    let store = StateStore::new();
    let r = root(1, 0xDD);
    assert!(store.try_add_state_root(r.clone()));
    let removed = store.discard(r.root_hash());
    assert!(removed.is_some());
    assert!(
        store
            .get_state_root(StateStoreLookup::ByBlockIndex(1))
            .is_none()
    );
}

#[test]
fn same_height_candidates_keep_index_mapping_after_discard() {
    let store = StateStore::new();
    let rejected = root(7, 0xA1);
    let accepted = root(7, 0xB2);

    assert!(store.try_add_state_root(rejected.clone()));
    assert!(store.try_add_state_root(accepted.clone()));
    assert_eq!(store.candidate_count(), 2);
    assert_eq!(
        store
            .get_state_root(StateStoreLookup::ByRootHash(*rejected.root_hash()))
            .as_ref()
            .map(StateRoot::root_hash),
        Some(rejected.root_hash())
    );
    assert_eq!(
        store
            .get_state_root(StateStoreLookup::ByRootHash(*accepted.root_hash()))
            .as_ref()
            .map(StateRoot::root_hash),
        Some(accepted.root_hash())
    );

    let removed = store.discard(rejected.root_hash());

    assert_eq!(
        removed.as_ref().map(StateRoot::root_hash),
        Some(rejected.root_hash())
    );
    assert_eq!(
        store
            .get_state_root(StateStoreLookup::ByRootHash(*accepted.root_hash()))
            .as_ref()
            .map(StateRoot::root_hash),
        Some(accepted.root_hash())
    );
    assert_eq!(
        store
            .get_state_root(StateStoreLookup::ByBlockIndex(7))
            .as_ref()
            .map(StateRoot::root_hash),
        Some(accepted.root_hash()),
        "discarding one same-height candidate must not remove the surviving height mapping"
    );
}

#[test]
fn transaction_captures_candidate_snapshot() {
    let store = StateStore::new();
    store.try_add_state_root(root(1, 0x10));
    store.try_add_state_root(root(2, 0x20));
    let tx = store.begin_transaction();
    assert_eq!(tx.candidates().len(), 2);
}

#[test]
fn transaction_commit_updates_original_candidate_set() {
    let store = StateStore::new();
    let r = root(1, 0x30);
    assert!(store.try_add_state_root(r.clone()));

    let tx = store.begin_transaction();
    tx.commit(&[r]);

    assert_eq!(store.candidate_count(), 0);
}

#[test]
fn mpt_changes_filter_ledger_and_project_track_states() {
    let snapshot = DataCache::new(false);
    let changed_key = StorageKey::new(5, vec![0xAA]);
    let deleted_key = StorageKey::new(6, vec![0xBB]);
    let ledger_key = StorageKey::new(LEDGER_CONTRACT_ID, vec![0xCC]);

    snapshot.add(deleted_key.clone(), StorageItem::from_bytes(vec![0x00]));
    snapshot.commit();
    snapshot.add(changed_key.clone(), StorageItem::from_bytes(vec![0x01]));
    snapshot.delete(&deleted_key);
    snapshot.add(ledger_key, StorageItem::from_bytes(vec![0x02]));

    let changes = StateStore::<MemoryStore>::mpt_changes_from_snapshot(&snapshot);
    assert_eq!(changes.len(), 2);
    assert!(
        changes.capacity() >= snapshot.pending_change_count(),
        "MPT projection should reserve for the DataCache change-set size"
    );
    assert!(changes.contains(&MptChange::Put {
        key: changed_key.to_array(),
        value: vec![0x01],
    }));
    assert!(changes.contains(&MptChange::Delete {
        key: deleted_key.to_array(),
    }));
}

#[test]
fn mpt_changes_project_into_reuses_capacity_and_preserves_projection() {
    let snapshot = DataCache::new(false);
    let changed_key = StorageKey::new(5, vec![0xAA]);
    let deleted_key = StorageKey::new(6, vec![0xBB]);
    let ledger_key = StorageKey::new(LEDGER_CONTRACT_ID, vec![0xCC]);

    snapshot.add(deleted_key.clone(), StorageItem::from_bytes(vec![0x00]));
    snapshot.commit();
    snapshot.add(changed_key.clone(), StorageItem::from_bytes(vec![0x01]));
    snapshot.delete(&deleted_key);
    snapshot.add(ledger_key, StorageItem::from_bytes(vec![0x02]));

    let expected = StateStore::<MemoryStore>::mpt_changes_from_snapshot(&snapshot);
    let mut changes = Vec::with_capacity(32);
    changes.push(MptChange::Delete { key: vec![0xFF] });
    let capacity = changes.capacity();

    StateStore::<MemoryStore>::project_mpt_changes_into(&snapshot, &mut changes);

    assert_eq!(changes, expected);
    assert_eq!(
        changes.capacity(),
        capacity,
        "projection into a reusable buffer should not discard capacity"
    );
}

#[test]
fn apply_snapshot_changes_updates_mpt_when_backend_exists() {
    let store = StateStore::with_mpt(false);
    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );

    let root = store
        .apply_snapshot_changes(0, &snapshot)
        .expect("MPT apply succeeds")
        .expect("MPT backend returns a root");
    let mpt = store.mpt().expect("MPT backend");
    assert_eq!(mpt.current_local_root_hash(), Some(root));
    assert!(mpt.get_state_root(0).is_some());
}

#[test]
fn apply_snapshot_changes_advances_root_record_for_ledger_only_block() {
    let store = StateStore::with_mpt(true);

    let block0 = DataCache::new(false);
    let user_key = StorageKey::new(5, vec![0xAA]);
    block0.add(user_key.clone(), StorageItem::from_bytes(vec![0x01]));
    let root0 = store
        .apply_snapshot_changes(0, &block0)
        .expect("block 0 applies")
        .expect("MPT backend returns a root");

    let ledger_only = DataCache::new(false);
    ledger_only.add(
        StorageKey::new(LEDGER_CONTRACT_ID, vec![0xCC]),
        StorageItem::from_bytes(vec![0x02]),
    );
    let root1 = store
        .apply_snapshot_changes(1, &ledger_only)
        .expect("ledger-only block applies")
        .expect("MPT backend returns a root");

    assert_eq!(root1, root0);
    let mpt = store.mpt().expect("MPT backend");
    assert_eq!(mpt.current_local_root_index(), Some(1));
    assert_eq!(mpt.current_local_root_hash(), Some(root0));
    assert_eq!(
        mpt.get_state_root(1)
            .expect("ledger-only block root record")
            .root_hash(),
        &root0
    );
    let mut trie = mpt.open_trie(Some(root0));
    assert_eq!(
        trie.get(&user_key.to_array()).expect("read user key"),
        Some(vec![0x01])
    );
}

#[test]
fn apply_snapshot_changes_reuses_previous_root_for_ledger_only_block_without_opening_trie_snapshot()
{
    let backing = Arc::new(SnapshotCountingRawOverlayStore::new());
    let store = StateStore::with_mpt_store(true, backing.clone()).expect("state store");

    let block0 = DataCache::new(false);
    block0.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let root0 = store
        .apply_snapshot_changes(0, &block0)
        .expect("block 0 applies")
        .expect("MPT backend returns a root");

    let ledger_only = DataCache::new(false);
    ledger_only.add(
        StorageKey::new(LEDGER_CONTRACT_ID, vec![0xCC]),
        StorageItem::from_bytes(vec![0x02]),
    );

    backing.reset_snapshot_count();
    let root1 = store
        .apply_snapshot_changes(1, &ledger_only)
        .expect("ledger-only block applies")
        .expect("MPT backend returns a root");

    assert_eq!(root1, root0);
    assert_eq!(
        backing.snapshot_count(),
        0,
        "ledger-only continuation blocks should not open an MPT backing snapshot"
    );
    let mpt = store.mpt().expect("MPT backend");
    assert_eq!(mpt.current_local_root(), Some((1, root0)));
    assert_eq!(
        mpt.get_state_root(1)
            .expect("ledger-only state root record")
            .root_hash(),
        &root0
    );
}

#[test]
fn apply_snapshot_changes_visits_tracked_items_once() {
    let source = include_str!("../../storage/state_store.rs");
    let body = source
        .split("fn apply_snapshot_mpt_changes_with_root")
        .nth(1)
        .and_then(|tail| tail.split("fn mpt_changes_from_snapshot").next())
        .expect("apply_snapshot_mpt_changes_with_root body source");

    assert_eq!(
        body.matches("visit_tracked_items").count(),
        1,
        "state-service sync apply should project and mutate from one DataCache visit"
    );
    assert!(
        !body.contains("mpt_change_count"),
        "state-service sync apply must not add a separate counting pass over DataCache"
    );
}

#[test]
fn apply_snapshot_changes_rejects_non_contiguous_mpt_updates() {
    let store = StateStore::with_mpt(false);
    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );

    let err = store
        .apply_snapshot_changes(5, &snapshot)
        .expect_err("empty MPT cannot jump directly to block 5");
    assert!(
        err.to_string().contains("non-contiguous"),
        "unexpected error: {err}"
    );
    let mpt = store.mpt().expect("MPT backend");
    assert_eq!(mpt.current_local_root_index(), None);
    assert!(mpt.get_state_root(5).is_none());
}

#[test]
fn state_store_mpt_constructor_surface_is_provider_neutral() {
    let source = include_str!("../../storage/state_store.rs");

    assert!(
        !source.contains("fn with_mpt_mdbx"),
        "StateStore should accept durable MPT backends through generic S: Store, not a backend-specific constructor"
    );
    assert!(
        !source.contains("fn with_mpt_memory_store"),
        "StateStore should accept durable MPT backends through generic S: Store, not a concrete memory-store constructor"
    );
    assert!(
        source.contains("pub struct StateStore<S: Store = MemoryStore>"),
        "StateStore must keep its backing MPT backend generic instead of erasing it behind erased Store trait object"
    );
}

#[test]
fn contiguous_root_before_uses_single_current_root_lookup() {
    let source = include_str!("../../storage/state_store.rs");
    let helper = source
        .split("fn contiguous_root_before")
        .nth(1)
        .and_then(|tail| tail.split("fn mpt_changes_from_snapshot").next())
        .expect("contiguous_root_before helper source");

    assert!(
        helper.contains("current_local_root()"),
        "the per-block StateService MPT path should read current index and root from one snapshot"
    );
    assert!(
        !helper.contains("current_local_root_hash()"),
        "separate current hash lookup adds a second map snapshot on every block"
    );
}

#[test]
fn apply_snapshot_changes_streams_mpt_projection_without_vec_materialization() {
    let source = include_str!("../../storage/state_store.rs");
    let body = source
        .split("pub fn apply_snapshot_changes")
        .nth(1)
        .and_then(|tail| tail.split("pub(crate) fn project_mpt_changes").next())
        .expect("apply_snapshot_changes body source");

    assert!(
        !body.contains("mpt_changes_from_snapshot(snapshot)"),
        "the per-block StateService MPT path should stream DataCache changes into the trie instead of materializing Vec<MptChange>"
    );
}

#[test]
fn apply_snapshot_changes_is_noop_without_mpt_backend() {
    let store = StateStore::new();
    let snapshot = DataCache::new(false);
    assert_eq!(store.apply_snapshot_changes(1, &snapshot).unwrap(), None);
}

#[test]
fn with_mpt_store_accepts_mdbx_backend_through_provider_neutral_store() {
    use neo_storage::mdbx::MdbxStoreProvider;
    use neo_storage::persistence::storage::StorageConfig;

    let path = std::env::temp_dir().join(format!(
        "neo-state-service-mdbx-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    let backing = Arc::new(
        MdbxStoreProvider::new(StorageConfig {
            path: path.clone(),
            ..Default::default()
        })
        .get_mdbx_store("")
        .expect("open MDBX"),
    );

    let store = StateStore::with_mpt_store(true, backing).expect("state store");
    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xCC]),
        StorageItem::from_bytes(vec![0x99]),
    );

    assert!(
        store
            .apply_snapshot_changes(0, &snapshot)
            .unwrap()
            .is_some()
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn with_mpt_store_accepts_memory_backend_through_provider_neutral_store() {
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    let backing = Arc::new(MemoryStore::new());
    let store = StateStore::with_mpt_store(true, backing.clone()).expect("state store");
    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xDD]),
        StorageItem::from_bytes(vec![0x42]),
    );

    assert!(
        store
            .apply_snapshot_changes(0, &snapshot)
            .unwrap()
            .is_some()
    );

    let reopened = MptStore::from_store(backing, true).expect("reopen");
    assert_eq!(reopened.current_local_root_index(), Some(0));
}
