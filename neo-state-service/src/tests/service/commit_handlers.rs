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
fn async_committing_flush_applies_queued_mpt_roots_in_order() {
    let store = Arc::new(StateStore::with_mpt(false));
    let handlers = StateServiceCommitHandlers::new_async(Arc::clone(&store));

    let snapshot0 = DataCache::new(false);
    snapshot0.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let snapshot1 = DataCache::new(false);
    snapshot1.update(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x02]),
    );

    assert!(handlers.on_committing_deferred(0, &snapshot0));
    assert!(handlers.on_committing_deferred(1, &snapshot1));
    assert!(handlers.flush());

    let mpt = store.mpt().expect("mpt backend");
    assert_eq!(mpt.current_local_root_index(), Some(1));
    assert!(mpt.get_state_root(0).is_some());
    assert!(mpt.get_state_root(1).is_some());
    assert!(
        crate::StateRootApplyMetrics::state_root_apply_stage_stats()
            .iter()
            .any(|stat| stat.stage == "queue_wait" && stat.calls > 0),
        "async StateService MPT apply should expose queue wait as a hotspot stage"
    );
    assert!(
        crate::StateRootApplyMetrics::state_root_apply_stage_stats()
            .iter()
            .any(|stat| stat.stage == "enqueue_blocking" && stat.calls > 0),
        "async StateService MPT apply should expose producer enqueue blocking separately"
    );
    assert!(
        crate::StateRootApplyMetrics::state_root_apply_count_stats()
            .iter()
            .any(|stat| stat.kind == "batch_blocks" && stat.samples > 0 && stat.total >= 1),
        "async StateService MPT apply should expose worker batch size"
    );
}

#[test]
fn async_live_committing_waits_for_mpt_apply_before_returning() {
    let store = Arc::new(StateStore::with_mpt(false));
    let handlers = StateServiceCommitHandlers::new_async_with_capacity(Arc::clone(&store), 4);
    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xAC]),
        StorageItem::from_bytes(vec![0x01]),
    );

    assert!(handlers.on_committing(0, &snapshot));

    let mpt = store.mpt().expect("mpt backend");
    assert_eq!(
        mpt.current_local_root_index(),
        Some(0),
        "live async committing must not return before the local MPT root is applied"
    );
}

#[test]
fn async_committing_recycles_projection_buffers_without_aliasing_queued_blocks() {
    let store = Arc::new(StateStore::with_mpt(false));
    let handlers = StateServiceCommitHandlers::new_async_with_capacity(Arc::clone(&store), 4);

    let snapshot0 = DataCache::new(false);
    snapshot0.add(
        StorageKey::new(5, vec![0xA0]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let snapshot1 = DataCache::new(false);
    snapshot1.add(
        StorageKey::new(5, vec![0xA1]),
        StorageItem::from_bytes(vec![0x02]),
    );
    let snapshot2 = DataCache::new(false);
    snapshot2.add(
        StorageKey::new(5, vec![0xA2]),
        StorageItem::from_bytes(vec![0x03]),
    );

    assert!(handlers.on_committing_deferred(0, &snapshot0));
    assert!(handlers.on_committing_deferred(1, &snapshot1));
    assert!(handlers.flush());
    let recycled_after_first_flush = handlers.recycled_change_buffer_count();
    assert!(
        recycled_after_first_flush > 0,
        "async projection buffers should be returned after worker apply"
    );

    assert!(handlers.on_committing_deferred(2, &snapshot2));
    assert!(handlers.flush());
    assert!(
        handlers.recycled_change_buffer_count() >= recycled_after_first_flush,
        "later applies should keep reusing the projection buffer pool"
    );

    let mpt = store.mpt().expect("mpt backend");
    assert_eq!(mpt.current_local_root_index(), Some(2));
    assert!(mpt.get_state_root(0).is_some());
    assert!(mpt.get_state_root(1).is_some());
    assert!(mpt.get_state_root(2).is_some());
    assert_ne!(
        mpt.get_state_root(0).unwrap().root_hash(),
        mpt.get_state_root(1).unwrap().root_hash(),
        "recycled buffers must not overwrite earlier queued block changes"
    );
    assert_ne!(
        mpt.get_state_root(1).unwrap().root_hash(),
        mpt.get_state_root(2).unwrap().root_hash(),
        "recycled buffers must preserve distinct later block changes"
    );
}

#[test]
fn async_projection_uses_zero_copy_tracked_item_visit() {
    let source = include_str!("../../service/commit_handlers.rs");
    let body = source
        .split("fn on_committing_async")
        .nth(1)
        .and_then(|tail| tail.split("pub fn on_reverting").next())
        .expect("on_committing_async body source");

    assert!(
        !body.contains("tracked_items()"),
        "async StateService MPT projection must not materialize DataCache tracked item snapshots"
    );
    assert!(
        body.contains("project_mpt_changes_into"),
        "async StateService MPT projection should fill a reusable owned MptChange buffer"
    );
}

#[test]
fn async_committing_drop_waits_for_queued_mpt_roots() {
    let store = Arc::new(StateStore::with_mpt(false));
    {
        let handlers = StateServiceCommitHandlers::new_async(Arc::clone(&store));
        let snapshot = DataCache::new(false);
        snapshot.add(
            StorageKey::new(5, vec![0xAA]),
            StorageItem::from_bytes(vec![0x01]),
        );

        assert!(handlers.on_committing_deferred(0, &snapshot));
    }

    let mpt = store.mpt().expect("mpt backend");
    assert_eq!(mpt.current_local_root_index(), Some(0));
    assert!(mpt.get_state_root(0).is_some());
}

#[test]
fn async_reverting_is_ordered_after_queued_mpt_applies() {
    let store = Arc::new(StateStore::with_mpt(true));
    let handlers = StateServiceCommitHandlers::new_async_with_capacity(Arc::clone(&store), 1);
    let snapshot0 = DataCache::new(false);
    snapshot0.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    let snapshot1 = DataCache::new(false);
    snapshot1.update(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x02]),
    );

    assert!(handlers.on_committing_deferred(0, &snapshot0));
    assert!(handlers.on_committing_deferred(1, &snapshot1));
    handlers.on_reverting(1, 1);
    assert!(handlers.flush());

    let mpt = store.mpt().expect("mpt backend");
    assert_eq!(mpt.current_local_root_index(), Some(0));
    assert!(mpt.get_state_root(0).is_some());
    assert!(mpt.get_state_root(1).is_none());
}

#[test]
fn async_reverting_discards_candidate_state_roots() {
    let store = Arc::new(StateStore::with_mpt(true));
    let handlers = StateServiceCommitHandlers::new_async_with_capacity(Arc::clone(&store), 1);
    let root = StateRoot::new_current(7, UInt256::from([0x22; 32]));
    assert!(store.try_add_state_root(root));
    assert_eq!(store.candidate_count(), 1);

    handlers.on_reverting(7, 7);
    assert!(handlers.flush());

    assert_eq!(
        store.candidate_count(),
        0,
        "async revert must discard candidate StateRoot records like sync revert"
    );
    assert!(
        store
            .get_state_root(crate::state_store::StateStoreLookup::ByBlockIndex(7))
            .is_none(),
        "reverted candidate state root must be removed from all lookup indexes"
    );
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
