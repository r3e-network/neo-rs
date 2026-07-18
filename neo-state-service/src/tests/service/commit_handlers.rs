use super::*;
use crate::state_root::StateRoot;
use neo_primitives::UInt256;
use neo_storage::persistence::{RawOverlaySource, RawReadOnlyStore};
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
fn coordinated_handlers_require_a_durable_mpt_backing() {
    let no_mpt = Arc::new(StateStore::new());
    assert!(StateServiceCommitHandlers::try_new_coordinated(no_mpt).is_err());

    let memory_only_mpt = Arc::new(StateStore::with_mpt(false));
    assert!(StateServiceCommitHandlers::try_new_coordinated(memory_only_mpt).is_err());
}

#[test]
fn coordinated_handlers_queue_blocks_until_external_commit() {
    let backing = Arc::new(MemoryStore::new());
    let store = Arc::new(
        StateStore::with_mpt_store(false, Arc::clone(&backing)).expect("state store with backing"),
    );
    let handlers =
        StateServiceCommitHandlers::try_new_coordinated(Arc::clone(&store)).expect("handlers");
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

    assert!(handlers.is_coordinated());
    assert!(!handlers.is_async());
    assert!(handlers.on_committing_deferred(0, &snapshot0));
    assert!(handlers.on_committing_deferred(1, &snapshot1));
    assert!(
        handlers.pending_coordinated_projected_changes()
            >= snapshot0.pending_change_count() + snapshot1.pending_change_count(),
        "queued projected change count must be visible for deferred work-budget flushes"
    );
    assert_eq!(
        store.mpt().expect("MPT").current_local_root(),
        None,
        "queued projections must not become visible before the external transaction"
    );
    assert!(handlers.flush_durable_result().is_err());

    let roots = handlers
        .commit_pending_coordinated(|backing, prepared| {
            assert!(backing.try_commit_borrowed_raw_overlay(prepared)?);
            Ok(())
        })
        .expect("coordinated commit")
        .expect("queued roots");

    assert_eq!(roots.len(), 2);
    assert_eq!(handlers.pending_coordinated_projected_changes(), 0);
    assert_eq!(
        store.mpt().expect("MPT").current_local_root(),
        Some((1, roots[1]))
    );
    assert!(
        handlers
            .commit_pending_coordinated(|_, _| panic!("empty batch must not invoke callback"))
            .expect("empty coordinated commit")
            .is_none()
    );
}

#[test]
fn coordinated_handler_failure_keeps_previous_visible_root() {
    let backing = Arc::new(MemoryStore::new());
    let store = Arc::new(
        StateStore::with_mpt_store(false, Arc::clone(&backing)).expect("state store with backing"),
    );
    let handlers =
        StateServiceCommitHandlers::try_new_coordinated(Arc::clone(&store)).expect("handlers");
    let snapshot = DataCache::new(false);
    snapshot.add(
        StorageKey::new(5, vec![0xAA]),
        StorageItem::from_bytes(vec![0x01]),
    );
    assert!(handlers.on_committing(0, &snapshot));

    let error = handlers
        .commit_pending_coordinated(|_backing, prepared| {
            prepared.visit_raw_overlay(&mut |_key: &[u8], _value: Option<&[u8]>| {});
            Err(neo_storage::StorageError::CommitFailed(
                "injected canonical failure".to_string(),
            ))
        })
        .expect_err("external failure must propagate");

    assert!(error.contains("injected canonical failure"));
    assert_eq!(store.mpt().expect("MPT").current_local_root(), None);
    assert_eq!(
        backing.try_get_bytes(crate::Keys::CURRENT_LOCAL_ROOT_INDEX),
        None
    );
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
    let mutation_counts = crate::StateRootApplyMetrics::state_root_apply_count_stats();
    for kind in [
        "put_node_cached_calls",
        "serialized_payload_bytes",
        "hash_computations",
        "max_recursion_depth",
        "overlay_working_set_entries",
    ] {
        assert!(
            mutation_counts
                .iter()
                .any(|stat| stat.kind == kind && stat.samples > 0 && stat.total > 0),
            "StateService MPT apply should expose non-zero {kind}"
        );
    }
    assert!(
        mutation_counts
            .iter()
            .any(|stat| { stat.kind == "repeated_ancestor_finalizations" && stat.samples > 0 }),
        "StateService MPT apply should sample repeated ancestor finalization"
    );
    for kind in [
        "trie_resolve_cache_hits",
        "trie_resolve_store_hits",
        "trie_resolve_store_misses",
        "deferred_finalization_read_bytes",
        "deferred_finalization_minor_faults",
        "deferred_finalization_major_faults",
        "finalization_cache_hits",
        "finalization_memory_hits",
        "finalization_memory_misses",
        "finalization_backing_hits",
        "finalization_backing_misses",
        "finalization_lookup_errors",
    ] {
        assert!(
            mutation_counts
                .iter()
                .any(|stat| stat.kind == kind && stat.samples > 0),
            "StateService MPT apply should sample {kind}"
        );
    }
}

#[test]
fn async_deferred_committing_coalesces_short_gap_applies() {
    let store = Arc::new(StateStore::with_mpt(false));
    let handlers = StateServiceCommitHandlers::new_async_with_capacity(Arc::clone(&store), 8);
    let snapshot0 = DataCache::new(false);
    let snapshot1 = DataCache::new(false);

    assert!(handlers.on_committing_deferred(0, &snapshot0));
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(handlers.on_committing_deferred(1, &snapshot1));
    assert!(handlers.flush());

    assert_eq!(
        handlers.applied_batch_sizes(),
        vec![2],
        "deferred bulk-sync applies separated by a tiny scheduler gap should still use one explicit MPT batch"
    );
    let mpt = store.mpt().expect("mpt backend");
    assert_eq!(mpt.current_local_root_index(), Some(1));
    assert!(mpt.get_state_root(0).is_some());
    assert!(mpt.get_state_root(1).is_some());
}

#[test]
fn async_apply_limit_is_independent_from_queue_backpressure_capacity() {
    let store = Arc::new(StateStore::with_mpt(false));
    let handlers = StateServiceCommitHandlers::new_async_with_limits(Arc::clone(&store), 8, 2);
    let snapshots = (0u8..8)
        .map(|index| {
            let snapshot = DataCache::new(false);
            snapshot.add(
                StorageKey::new(5, vec![index]),
                StorageItem::from_bytes(vec![index]),
            );
            snapshot
        })
        .collect::<Vec<_>>();

    for (index, snapshot) in snapshots.iter().enumerate() {
        assert!(handlers.on_committing_deferred(index as u32, snapshot));
    }
    assert!(handlers.flush());

    assert_eq!(handlers.async_queue_capacity(), Some(8));
    assert_eq!(handlers.async_apply_batch_limit(), Some(2));
    assert_eq!(handlers.applied_batch_sizes(), vec![2, 2, 2, 2]);
    assert_eq!(
        store.mpt().expect("MPT backend").current_local_root_index(),
        Some(7)
    );
}

#[test]
fn async_worker_coalesces_continuous_work_to_apply_ceiling() {
    let store = Arc::new(StateStore::with_mpt(false));
    let handlers = StateServiceCommitHandlers::new_async_with_limits(Arc::clone(&store), 16, 8);
    let snapshots = (0u8..8)
        .map(|index| {
            let snapshot = DataCache::new(false);
            snapshot.add(
                StorageKey::new(5, vec![index]),
                StorageItem::from_bytes(vec![index]),
            );
            snapshot
        })
        .collect::<Vec<_>>();

    for (index, snapshot) in snapshots.iter().enumerate() {
        assert!(handlers.on_committing_deferred(index as u32, snapshot));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(handlers.flush());

    assert_eq!(handlers.async_apply_batch_limit(), Some(8));
    assert_eq!(handlers.applied_batch_sizes(), vec![8]);
    assert_eq!(
        store.mpt().expect("MPT backend").current_local_root_index(),
        Some(7)
    );
}

#[test]
fn async_worker_splits_batches_at_projected_change_limit() {
    fn request(block_index: u32, change_count: usize) -> AsyncApplyRequest {
        AsyncApplyRequest {
            block_index,
            changes: (0..change_count)
                .map(|index| crate::mpt_store::MptChange::Put {
                    key: vec![block_index as u8, index as u8],
                    value: vec![index as u8],
                })
                .collect(),
            project_us: 0,
            queued_at: std::time::Instant::now(),
            total_start: std::time::Instant::now(),
        }
    }

    let (tx, rx) = std::sync::mpsc::sync_channel(3);
    tx.send(AsyncCommand::Apply(request(0, 5))).unwrap();
    tx.send(AsyncCommand::Apply(request(1, 4))).unwrap();
    tx.send(AsyncCommand::Apply(request(2, 4))).unwrap();
    drop(tx);

    let AsyncCommand::Apply(first) = rx.recv().unwrap() else {
        panic!("expected first apply request");
    };
    let mut first_batch = vec![first];
    let mut pending = None;
    collect_apply_batch(&rx, &mut pending, &mut first_batch, 4, 8);
    assert_eq!(
        first_batch
            .iter()
            .map(|request| request.block_index)
            .collect::<Vec<_>>(),
        vec![0]
    );

    let Some(AsyncCommand::Apply(second)) = pending.take() else {
        panic!("change limit should defer the next apply request");
    };
    let mut second_batch = vec![second];
    collect_apply_batch(&rx, &mut pending, &mut second_batch, 4, 8);
    assert_eq!(
        second_batch
            .iter()
            .map(|request| request.block_index)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(pending.is_none());
}

#[test]
fn async_worker_accepts_one_oversized_projected_block() {
    let mut batch = vec![AsyncApplyRequest {
        block_index: 0,
        changes: (0..9)
            .map(|index| crate::mpt_store::MptChange::Put {
                key: vec![index],
                value: vec![index],
            })
            .collect(),
        project_us: 0,
        queued_at: std::time::Instant::now(),
        total_start: std::time::Instant::now(),
    }];
    let mut batch_changes = 9;
    let next = AsyncApplyRequest {
        block_index: 1,
        changes: Vec::new(),
        project_us: 0,
        queued_at: std::time::Instant::now(),
        total_start: std::time::Instant::now(),
    };

    let deferred = try_push_apply_request(&mut batch, &mut batch_changes, next, 8)
        .expect_err("an oversized block must form a batch by itself");
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].changes.len(), 9);
    assert_eq!(deferred.block_index, 1);
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
