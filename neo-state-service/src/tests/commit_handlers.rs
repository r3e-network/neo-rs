use super::*;
use crate::state_root::StateRoot;
use neo_primitives::UInt256;
use neo_storage::{StorageItem, StorageKey};

#[test]
fn committing_updates_mpt_root() {
    let store = Arc::new(StateStore::with_mpt(false));
    let handlers = StateServiceCommitHandlers::new(Arc::clone(&store));
    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );

    assert!(handlers.on_committing(0, &snapshot));
    let mpt = store.mpt().expect("mpt backend");
    assert_eq!(mpt.current_local_root_index(), Some(0));
    assert!(mpt.current_local_root_hash().is_some());
    assert!(mpt.get_state_root(0).is_some());
}

#[test]
fn committing_is_noop_without_mpt_backend() {
    let store = Arc::new(StateStore::new());
    let handlers = StateServiceCommitHandlers::new(Arc::clone(&store));
    let snapshot = DataCache::new(false);
    assert!(handlers.on_committing(1, &snapshot));
    assert!(store.mpt().is_none());
    assert_eq!(store.candidate_count(), 0);
}

#[test]
fn reverting_discards_root() {
    let store = Arc::new(StateStore::new());
    let handlers = StateServiceCommitHandlers::new(Arc::clone(&store));
    let root = StateRoot::new_current(5, UInt256::from([0x11; 32]));
    assert!(store.try_add_state_root(root));
    assert_eq!(store.candidate_count(), 1);
    handlers.on_reverting(5, 5);
    assert_eq!(store.candidate_count(), 0);
}
