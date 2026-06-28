use super::*;
use neo_storage::{StorageItem, StorageKey};

fn root(index: u32, byte: u8) -> StateRoot {
    StateRoot::new_current(index, UInt256::from([byte; 32]))
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

    let changes = StateStore::mpt_changes_from_snapshot(&snapshot);
    assert_eq!(changes.len(), 2);
    assert!(changes.contains(&MptChange::Put {
        key: changed_key.to_array(),
        value: vec![0x01],
    }));
    assert!(changes.contains(&MptChange::Delete {
        key: deleted_key.to_array(),
    }));
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
fn apply_snapshot_changes_is_noop_without_mpt_backend() {
    let store = StateStore::new();
    let snapshot = DataCache::new(false);
    assert_eq!(store.apply_snapshot_changes(1, &snapshot).unwrap(), None);
}
