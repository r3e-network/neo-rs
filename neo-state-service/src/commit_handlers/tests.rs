use super::trie_stores::{BlockTrieNodeStore, STATE_ROOT_NODE_CACHE_CAPACITY};
use super::*;
use neo_crypto::mpt_trie::{ProofCache, ProofCacheNamespace};
use neo_protocol::execution::{ApplicationExecuted, Block, BlockHeader};
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::store::Store;

/// Builds a block snapshot carrying the given storage changes.
fn snapshot_with_changes(changes: &[(i32, Vec<u8>, Vec<u8>)]) -> PersistenceDataCache {
    let snapshot = PersistenceDataCache::new(false);
    for (id, key, value) in changes {
        snapshot
            .try_add(
                StorageKey::new(*id, key.clone()),
                StorageItem::from_bytes(value.clone()),
            )
            .expect("seed snapshot change");
    }
    snapshot
}

fn empty_snapshot() -> PersistenceDataCache {
    PersistenceDataCache::new(false)
}

fn pending_root(handlers: &StateServiceCommitHandlers) -> UInt256 {
    handlers
        .pending_state_root
        .lock()
        .unwrap()
        .as_ref()
        .expect("pending state root")
        .root_hash
}

/// Mirrors PersistenceDataCache::clone_cache().commit(): the child changes become
/// visible in the parent StoreCache before the committed callback runs.
fn finish_commit(
    handlers: &StateServiceCommitHandlers,
    index: u32,
    snapshot: &PersistenceDataCache,
) {
    handlers
        .store_cache
        .data_cache()
        .merge_tracked_items(&snapshot.tracked_items());
    snapshot.commit();
    handlers.persist_pending_state_root(index).unwrap();
}

/// Writes everything the handler staged into the backing store, mirroring a
/// real block commit.
fn persist_to_store(handlers: &StateServiceCommitHandlers, store: &Arc<dyn Store>) {
    let mut persisted = store.snapshot();
    let writer = Arc::get_mut(&mut persisted).expect("exclusive store snapshot");
    handlers
        .store_cache
        .data_cache()
        .apply_tracked_to_writer(writer)
        .expect("persist tracked state-service entries");
    writer.try_commit().expect("store commit");
}

/// Persists the local-commit cursor the way `StateRootStore` does.
fn stage_local_cursor(handlers: &StateServiceCommitHandlers, index: u32) {
    let key = StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_LOCAL_ROOT_INDEX]);
    let item = StorageItem::from_bytes(u64::from(index).to_be_bytes().to_vec());
    let result = if handlers.store_cache.get(&key).is_some() {
        handlers.store_cache.data_cache().try_update(key, item)
    } else {
        handlers.store_cache.data_cache().try_add(key, item)
    };
    result.expect("stage local root cursor");
}

/// Builds a record carrying a witness, i.e. a quorum-validated network root.
fn network_root_record(index: u32, root_hash: UInt256) -> StateRoot {
    let mut record = StateRoot::new_current(index, root_hash);
    record.witness = Some(neo_protocol::witness::Witness::new_with_scripts(
        vec![0x00],
        vec![0x51],
    ));
    record
}

#[test]
fn backed_block_snapshot_is_authoritative_for_mpt_absence() {
    let mpt_key = b"cold-node";
    let storage_key = TrieNodeStore::namespaced_key(mpt_key);
    let stored_value = vec![1, 2, 3];
    let mut persisted_cache = StoreCache::new(Arc::new(MemoryStore::new()));
    persisted_cache
        .try_add(
            storage_key.clone(),
            StorageItem::from_bytes(stored_value.clone()),
        )
        .unwrap();
    persisted_cache.try_commit().unwrap();
    let persisted = Arc::new(TrieNodeStore {
        store_cache: Arc::new(persisted_cache),
    });

    let empty_backing = StoreCache::new(Arc::new(MemoryStore::new()));
    let backed = BlockTrieNodeStore {
        persisted: Arc::clone(&persisted),
        snapshot: empty_backing.data_cache().clone(),
    };
    assert_eq!(backed.try_get(mpt_key).unwrap(), None);

    let detached = BlockTrieNodeStore {
        persisted,
        snapshot: PersistenceDataCache::new(false),
    };
    assert_eq!(detached.try_get(mpt_key).unwrap(), Some(stored_value));
}

#[test]
fn test_commit_handlers_creation() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));

    let handlers = StateServiceCommitHandlers::new(cache, true);
    assert!(handlers.is_ok());
}

#[test]
fn test_commit_handlers_disabled() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, false).unwrap();

    // When disabled, commit_block should succeed but do nothing
    assert!(handlers.commit_block(0, &empty_snapshot()).is_ok());

    // Pending state root should remain None
    let pending = handlers.pending_state_root.lock().unwrap();
    assert!(pending.is_none());
}

#[test]
fn test_commit_block_computes_real_state_root() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"balance".to_vec())]);
    assert!(handlers.commit_block(100, &snapshot).is_ok());

    // Should have pending state root
    let pending = handlers.pending_state_root.lock().unwrap();
    assert!(pending.is_some());

    let state_root = pending.as_ref().unwrap();
    assert_eq!(state_root.index, 100);
    assert_eq!(
        state_root.version,
        super::super::state_root::CURRENT_VERSION
    );
    // The root is a real MPT hash over the storage changes: non-zero and
    // nothing is persisted before the committed handler runs.
    assert!(!state_root.root_hash.is_zero());
    drop(pending);
    assert!(handlers.get_state_root(100).is_none());
}

#[test]
fn test_state_root_is_deterministic_across_nodes() {
    let changes: &[(i32, Vec<u8>, Vec<u8>)] = &[
        (2, b"alpha".to_vec(), b"one".to_vec()),
        (1, b"shared-prefix-a".to_vec(), b"three".to_vec()),
        (2, b"beta".to_vec(), b"two".to_vec()),
        (1, b"shared-prefix-b".to_vec(), b"four".to_vec()),
    ];
    let mut reversed = changes.to_vec();
    reversed.reverse();

    let handlers_a = StateServiceCommitHandlers::new(
        Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
        true,
    )
    .unwrap();
    let handlers_b = StateServiceCommitHandlers::new(
        Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
        true,
    )
    .unwrap();

    handlers_a
        .commit_block(4, &snapshot_with_changes(changes))
        .unwrap();
    handlers_b
        .commit_block(4, &snapshot_with_changes(&reversed))
        .unwrap();

    // Two independent nodes applying the same changes derive the same root.
    assert_eq!(pending_root(&handlers_a), pending_root(&handlers_b));
}

#[test]
fn ledger_contract_bookkeeping_is_excluded_from_state_root() {
    let contract_changes = &[(1, b"account".to_vec(), b"balance".to_vec())];
    let without_ledger = StateServiceCommitHandlers::new(
        Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
        true,
    )
    .unwrap();
    let with_ledger = StateServiceCommitHandlers::new(
        Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
        true,
    )
    .unwrap();
    without_ledger
        .commit_block(0, &snapshot_with_changes(contract_changes))
        .unwrap();

    let mut changes = contract_changes.to_vec();
    changes.push((-4, vec![0x0c], b"hash-and-index".to_vec()));
    changes.push((-4, vec![0x09, 0, 0, 0, 0], b"block-hash".to_vec()));
    with_ledger
        .commit_block(0, &snapshot_with_changes(&changes))
        .unwrap();

    assert_eq!(pending_root(&without_ledger), pending_root(&with_ledger));
}

#[test]
fn test_consecutive_blocks_produce_different_roots() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let snapshot_a = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
    assert!(handlers.commit_block(1, &snapshot_a).is_ok());
    let root_a = pending_root(&handlers);
    finish_commit(&handlers, 1, &snapshot_a);

    let snapshot_b = snapshot_with_changes(&[(1, b"account".to_vec(), b"v2".to_vec())]);
    assert!(handlers.commit_block(2, &snapshot_b).is_ok());
    let root_b = pending_root(&handlers);
    finish_commit(&handlers, 2, &snapshot_b);

    assert_ne!(root_a, root_b);
    assert_eq!(handlers.current_root_hash(), Some(root_b));
    assert_eq!(handlers.get_state_root(1).unwrap().root_hash, root_a);
    assert_eq!(handlers.get_state_root(2).unwrap().root_hash, root_b);
}

#[test]
fn store_scoped_node_cache_matches_uncached_and_cold_restart_roots() {
    let cached_store = Arc::new(MemoryStore::new());
    let cached =
        StateServiceCommitHandlers::new(Arc::new(StoreCache::new(cached_store.clone())), true)
            .unwrap();
    let uncached = StateServiceCommitHandlers::new(
        Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
        true,
    )
    .unwrap();
    assert!(uncached.node_cache_stats().enabled);
    uncached.set_node_cache_enabled(false);
    assert!(!uncached.node_cache_stats().enabled);

    for (height, value) in [
        (1u32, b"one".as_slice()),
        (2, b"two".as_slice()),
        (3, b"three".as_slice()),
    ] {
        let changes = [
            (1, b"account".to_vec(), value.to_vec()),
            (2, b"other".to_vec(), value.to_vec()),
        ];
        let cached_snapshot = snapshot_with_changes(&changes);
        let uncached_snapshot = snapshot_with_changes(&changes);
        cached.commit_block(height, &cached_snapshot).unwrap();
        uncached.commit_block(height, &uncached_snapshot).unwrap();
        assert_eq!(pending_root(&cached), pending_root(&uncached));
        finish_commit(&cached, height, &cached_snapshot);
        finish_commit(&uncached, height, &uncached_snapshot);
    }

    let node_cache = cached.node_cache.lock().unwrap().clone().unwrap();
    // Warm blocks reuse the committed root directly and need not consult
    // this fallback cache. The recovered handler below must still be cold.
    assert!(node_cache.len() <= STATE_ROOT_NODE_CACHE_CAPACITY);
    let uncached_stats = uncached.node_cache_stats();
    assert_eq!((uncached_stats.hits, uncached_stats.misses), (0, 0));
    assert_eq!(uncached_stats.entries, 0);

    let mut persisted = cached_store.snapshot();
    let writer = Arc::get_mut(&mut persisted).expect("new memory-store snapshot");
    cached
        .store_cache
        .data_cache()
        .apply_tracked_to_writer(writer)
        .unwrap();
    writer.try_commit().unwrap();
    let recovered =
        StateServiceCommitHandlers::new(Arc::new(StoreCache::new(cached_store)), true).unwrap();
    assert!(
        recovered
            .node_cache
            .lock()
            .unwrap()
            .clone()
            .unwrap()
            .is_empty()
    );
    assert_eq!(recovered.recover_from_storage().unwrap(), Some(3));

    let changes = [(1, b"account".to_vec(), b"four".to_vec())];
    let cached_snapshot = snapshot_with_changes(&changes);
    let recovered_snapshot = snapshot_with_changes(&changes);
    cached.commit_block(4, &cached_snapshot).unwrap();
    recovered.commit_block(4, &recovered_snapshot).unwrap();
    assert_eq!(pending_root(&cached), pending_root(&recovered));
    let (_, misses, _) = recovered
        .node_cache
        .lock()
        .unwrap()
        .clone()
        .unwrap()
        .stats();
    assert!(misses > 0, "a restart must begin with a cold cache");
}

#[test]
fn test_empty_block_without_prior_root_is_rejected() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    // An empty state trie has no representable root: fail closed.
    let result = handlers.commit_block(1, &empty_snapshot());
    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("empty"));
    assert!(handlers.pending_state_root.lock().unwrap().is_none());
}

#[test]
fn test_empty_block_after_prior_root_keeps_previous_root() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
    assert!(handlers.commit_block(1, &snapshot).is_ok());
    finish_commit(&handlers, 1, &snapshot);
    let root = handlers.current_root_hash().unwrap();

    // A block with no storage changes keeps the state (and root) unchanged.
    assert!(handlers.commit_block(2, &empty_snapshot()).is_ok());
    assert_eq!(pending_root(&handlers), root);
}

#[test]
fn test_deletions_change_the_state_root() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let key_x = StorageKey::new(3, b"key-x".to_vec());
    let key_y = StorageKey::new(3, b"key-y".to_vec());

    let snapshot_one = snapshot_with_changes(&[
        (3, b"key-x".to_vec(), b"v1".to_vec()),
        (3, b"key-y".to_vec(), b"v2".to_vec()),
    ]);
    handlers.commit_block(1, &snapshot_one).unwrap();
    finish_commit(&handlers, 1, &snapshot_one);
    let root_one = handlers.current_root_hash().unwrap();

    // Block two deletes key-x (tracked change with no value).
    let snapshot_two = PersistenceDataCache::new(false);
    snapshot_two
        .try_add(key_x.clone(), StorageItem::from_bytes(b"v1".to_vec()))
        .unwrap();
    snapshot_two
        .try_add(key_y.clone(), StorageItem::from_bytes(b"v2".to_vec()))
        .unwrap();
    // Establish the entries as the snapshot's committed baseline. Adding
    // and deleting in the same uncommitted change set cancels the entry,
    // whereas deleting this baseline records TrackState::Deleted.
    snapshot_two.commit();
    snapshot_two.try_delete(&key_x).unwrap();

    handlers.commit_block(2, &snapshot_two).unwrap();
    let root_two = pending_root(&handlers);
    assert_ne!(root_one, root_two);
    finish_commit(&handlers, 2, &snapshot_two);

    // The surviving key is still readable (and the deleted key is gone)
    // under the new root through the persisted node store.
    let mut trie =
        Trie::new_with_node_cache(handlers.node_store.clone(), Some(root_two), true, None);
    assert_eq!(trie.get(&key_y.to_array()).unwrap(), Some(b"v2".to_vec()));
    assert_eq!(trie.get(&key_x.to_array()).unwrap(), None);
}

#[test]
fn test_rollback_clears_pending() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    // Commit and verify pending state root exists
    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
    assert!(handlers.commit_block(50, &snapshot).is_ok());
    assert!(handlers.pending_state_root.lock().unwrap().is_some());

    // Rollback should clear it
    assert!(handlers.rollback().is_ok());
    assert!(handlers.pending_state_root.lock().unwrap().is_none());
    assert!(handlers.pending_root_node.lock().unwrap().is_none());
}

#[test]
fn warm_root_matches_cold_reload_and_rollback_preserves_committed_baseline() {
    let warm = StateServiceCommitHandlers::new(
        Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
        true,
    )
    .unwrap();
    let cold = StateServiceCommitHandlers::new(
        Arc::new(StoreCache::new(Arc::new(MemoryStore::new()))),
        true,
    )
    .unwrap();

    let first = [(1, b"account".to_vec(), b"v1".to_vec())];
    let warm_snapshot = snapshot_with_changes(&first);
    let cold_snapshot = snapshot_with_changes(&first);
    warm.commit_block(1, &warm_snapshot).unwrap();
    cold.commit_block(1, &cold_snapshot).unwrap();
    assert_eq!(pending_root(&warm), pending_root(&cold));
    finish_commit(&warm, 1, &warm_snapshot);
    finish_commit(&cold, 1, &cold_snapshot);
    let committed_root = warm.current_root_hash();
    *cold.current_root_node.lock().unwrap() = None;

    // This candidate mutates the shared path but is never made durable.
    let aborted = [(1, b"account".to_vec(), b"aborted".to_vec())];
    let aborted_snapshot = snapshot_with_changes(&aborted);
    warm.commit_block(2, &aborted_snapshot).unwrap();
    warm.rollback().unwrap();
    assert_eq!(warm.current_root_hash(), committed_root);

    let second = [
        (1, b"account".to_vec(), b"v2".to_vec()),
        (2, b"other".to_vec(), b"v2".to_vec()),
    ];
    let warm_snapshot = snapshot_with_changes(&second);
    let cold_snapshot = snapshot_with_changes(&second);
    warm.commit_block(2, &warm_snapshot).unwrap();
    cold.commit_block(2, &cold_snapshot).unwrap();
    assert_eq!(pending_root(&warm), pending_root(&cold));
    finish_commit(&warm, 2, &warm_snapshot);
    finish_commit(&cold, 2, &cold_snapshot);
    *cold.current_root_node.lock().unwrap() = None;

    let third = [
        (1, b"account".to_vec(), b"v3".to_vec()),
        (2, b"other".to_vec(), b"v3".to_vec()),
    ];
    let warm_snapshot = snapshot_with_changes(&third);
    let cold_snapshot = snapshot_with_changes(&third);
    warm.commit_block(3, &warm_snapshot).unwrap();
    cold.commit_block(3, &cold_snapshot).unwrap();
    assert_eq!(pending_root(&warm), pending_root(&cold));
}

#[test]
fn test_persist_pending_state_root() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
    assert!(handlers.commit_block(200, &snapshot).is_ok());
    let expected_root = pending_root(&handlers);

    handlers
        .store_cache
        .data_cache()
        .merge_tracked_items(&snapshot.tracked_items());
    snapshot.commit();

    // The committed callback consumes the pending root; the block snapshot
    // has already made the record visible through the parent cache.
    assert!(matches!(
        handlers.persist_pending_state_root(200),
        Ok(Some(ref persisted)) if persisted.root_hash == expected_root
    ));
    assert!(handlers.pending_state_root.lock().unwrap().is_none());
    assert_eq!(handlers.current_root_hash(), Some(expected_root));

    let record = handlers.get_state_root(200).expect("persisted record");
    assert_eq!(record.index, 200);
    assert_eq!(record.root_hash, expected_root);
    assert!(record.witness.is_none());
}

#[test]
fn test_persist_index_mismatch() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
    assert!(handlers.commit_block(300, &snapshot).is_ok());

    // Try to finalize with the wrong index without committing the snapshot.
    assert!(handlers.persist_pending_state_root(301).unwrap().is_none());
    assert!(handlers.pending_state_root.lock().unwrap().is_none());
    assert!(handlers.get_state_root(300).is_none());
    assert!(handlers.get_state_root(301).is_none());
}

#[test]
fn test_state_root_supports_mpt_proofs() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let storage_key = StorageKey::new(7, b"proof-key".to_vec());
    let snapshot = snapshot_with_changes(&[(7, b"proof-key".to_vec(), b"proof-value".to_vec())]);
    assert!(handlers.commit_block(10, &snapshot).is_ok());
    let root = pending_root(&handlers);
    finish_commit(&handlers, 10, &snapshot);

    // Generate an inclusion proof from the persisted node store...
    let mut trie = Trie::new_with_node_cache(handlers.node_store.clone(), Some(root), true, None);
    let proof_set = trie
        .try_get_proof(&storage_key.to_array())
        .expect("generate proof")
        .expect("key present in trie");
    assert!(!proof_set.is_empty());

    // ...and validate it: the proof resolves the value under the root.
    assert_eq!(
        Trie::<TrieNodeStore>::verify_proof(root, &storage_key.to_array(), &proof_set)
            .expect("verify proof"),
        b"proof-value".to_vec()
    );

    // The same proof must not verify for a key that was never written.
    let missing = StorageKey::new(8, b"missing".to_vec());
    assert!(Trie::<TrieNodeStore>::verify_proof(root, &missing.to_array(), &proof_set).is_err());
}

#[test]
fn test_committed_root_proof_cache_survives_head_advance() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let storage_key = StorageKey::new(7, b"cached-proof-key".to_vec());
    let first_snapshot =
        snapshot_with_changes(&[(7, b"cached-proof-key".to_vec(), b"value-1".to_vec())]);
    assert!(handlers.commit_block(11, &first_snapshot).is_ok());
    let first_root = pending_root(&handlers);
    finish_commit(&handlers, 11, &first_snapshot);

    // A second block advances the head; the first root stays provable.
    let second_snapshot =
        snapshot_with_changes(&[(7, b"cached-proof-key".to_vec(), b"value-2".to_vec())]);
    assert!(handlers.commit_block(12, &second_snapshot).is_ok());
    let second_root = pending_root(&handlers);
    finish_commit(&handlers, 12, &second_snapshot);
    assert_ne!(first_root, second_root);
    assert_eq!(handlers.current_root_hash(), Some(second_root));

    // Proofs are cached per committed root, so history keeps working while
    // the current root moves on.
    let proof_cache = ProofCache::with_defaults();
    let namespace = ProofCacheNamespace::new(u64::from(STATE_ROOT_STORAGE_ID.unsigned_abs()));
    let mut trie =
        Trie::new_with_node_cache(handlers.node_store.clone(), Some(first_root), true, None);
    let committed = trie
        .committed_root()
        .expect("resolved root node is immutable");
    assert_eq!(committed.hash(), first_root);

    let key_bytes = storage_key.to_array();
    let first = trie
        .get_proof_cached(&proof_cache, committed, namespace, &key_bytes)
        .expect("cached proof")
        .expect("key present");
    let replay = trie
        .get_proof_cached(&proof_cache, committed, namespace, &key_bytes)
        .expect("cached proof replay")
        .expect("key present");
    assert!(Arc::ptr_eq(&first, &replay));
    assert_eq!(proof_cache.stats().hits, 1);
    assert_eq!(proof_cache.stats().constructions, 1);
    assert_eq!(
        Trie::<TrieNodeStore>::verify_proof(first_root, &key_bytes, &first.to_hash_set())
            .expect("verify cached proof"),
        b"value-1".to_vec()
    );

    // The cache is keyed by the committed root and does not follow the
    // head, so the entry stays valid after the head advanced.
    assert_eq!(handlers.current_root_hash(), Some(second_root));
    assert_eq!(proof_cache.len(), 1);
}

#[test]
fn test_recover_from_storage_seeds_baseline() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let handlers =
        StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store.clone())), true).unwrap();

    // Commit two real blocks: root records, trie nodes and the local cursor
    // are all persisted by the commit path.
    let mut committed_root = None;
    for (height, value) in [(3u32, b"three".as_slice()), (5, b"five".as_slice())] {
        let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), value.to_vec())]);
        handlers.commit_block(height, &snapshot).unwrap();
        committed_root = Some(pending_root(&handlers));
        finish_commit(&handlers, height, &snapshot);
    }
    stage_local_cursor(&handlers, 5);
    persist_to_store(&handlers, &store);
    let root = committed_root.expect("committed root");

    // A fresh handler over a fresh cache recovers the locally committed root.
    let cache = Arc::new(StoreCache::new(store));
    let recovered = StateServiceCommitHandlers::new(cache, true).unwrap();
    assert_eq!(recovered.current_root_hash(), None);
    assert_eq!(recovered.recover_from_storage().unwrap(), Some(5));
    assert_eq!(recovered.current_root_hash(), Some(root));
    assert_eq!(recovered.get_state_root(5).unwrap().root_hash, root);
    assert!(recovered.get_state_root(4).is_none());

    // The recovered baseline extends the hydrated trie, so the next block
    // derives the same root as the original handler would.
    let changes = [(1, b"account".to_vec(), b"six".to_vec())];
    handlers
        .commit_block(6, &snapshot_with_changes(&changes))
        .unwrap();
    recovered
        .commit_block(6, &snapshot_with_changes(&changes))
        .unwrap();
    assert_eq!(pending_root(&handlers), pending_root(&recovered));
}

#[test]
fn recovery_never_adopts_a_network_root_above_the_local_cursor() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let handlers =
        StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store.clone())), true).unwrap();

    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"local".to_vec())]);
    handlers.commit_block(7, &snapshot).unwrap();
    let local_root = pending_root(&handlers);
    finish_commit(&handlers, 7, &snapshot);
    stage_local_cursor(&handlers, 7);

    // A quorum-validated root was persisted for a height whose block does
    // not exist locally yet. It must not become the trie baseline.
    let network = network_root_record(42, UInt256::from([0xAB; 32]));
    handlers
        .store_cache
        .data_cache()
        .try_add(
            state_root_record_key(42),
            StorageItem::from_bytes(serialize_state_root_record(&network)),
        )
        .unwrap();
    persist_to_store(&handlers, &store);

    let recovered =
        StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store)), true).unwrap();
    assert_eq!(recovered.recover_from_storage().unwrap(), Some(7));
    assert_eq!(recovered.current_root_hash(), Some(local_root));
}

#[test]
fn recovery_without_a_cursor_ignores_network_only_records() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let mut writer = StoreCache::new(store.clone());
    let network = network_root_record(9, UInt256::from([0xCD; 32]));
    writer
        .data_cache()
        .try_add(
            state_root_record_key(9),
            StorageItem::from_bytes(serialize_state_root_record(&network)),
        )
        .unwrap();
    writer.commit();

    // No local cursor and no unsigned record: this node has no locally
    // committed state, so recovery must start from an empty trie.
    let handlers = StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store)), true).unwrap();
    assert_eq!(handlers.recover_from_storage().unwrap(), None);
    assert_eq!(handlers.current_root_hash(), None);
}

#[test]
fn recovery_refuses_a_cursor_whose_trie_node_is_missing() {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let mut writer = StoreCache::new(store.clone());
    let root = UInt256::from([0x5A; 32]);
    writer
        .data_cache()
        .try_add(
            state_root_record_key(11),
            StorageItem::from_bytes(serialize_state_root_record(&StateRoot::new_current(
                11, root,
            ))),
        )
        .unwrap();
    writer
        .data_cache()
        .try_add(
            StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_LOCAL_ROOT_INDEX]),
            StorageItem::from_bytes(11u64.to_be_bytes().to_vec()),
        )
        .unwrap();
    writer.commit();

    let handlers = StateServiceCommitHandlers::new(Arc::new(StoreCache::new(store)), true).unwrap();
    let error = handlers
        .recover_from_storage()
        .expect_err("a root without its trie must not be adopted");
    assert!(
        error.message.contains("missing from local storage"),
        "unexpected error: {}",
        error.message
    );
    assert_eq!(handlers.current_root_hash(), None);
}

#[test]
fn test_recover_from_storage_without_records() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    assert_eq!(handlers.recover_from_storage().unwrap(), None);
    assert_eq!(handlers.current_root_hash(), None);
}

#[test]
fn test_state_service_storage_keys_are_namespaced() {
    let key = state_root_record_key(42);
    assert_eq!(key.id, STATE_ROOT_STORAGE_ID);
    assert_eq!(key.key(), &[PREFIX_STATE_ROOT, 0, 0, 0, 42][..]);

    // The dedicated id cannot collide with native contracts (small
    // negative ids) or deployed contracts (positive ids).
    assert!(STATE_ROOT_STORAGE_ID < -100);

    // Root-record and trie-node keys share the id but differ in prefix.
    let node_key = TrieNodeStore::namespaced_key(&[0u8; 33]);
    assert_eq!(node_key.id, STATE_ROOT_STORAGE_ID);
    assert_eq!(node_key.key()[0], PREFIX_TRIE_NODE);
    assert_ne!(node_key.key()[0], PREFIX_STATE_ROOT);
}

#[test]
fn test_state_root_record_serialization_round_trip() {
    let root_hash = UInt256::from([7u8; 32]);

    // Unsigned record.
    let state_root = StateRoot::new_current(12, root_hash);
    let bytes = serialize_state_root_record(&state_root);
    assert_eq!(bytes.len(), 32 + 1 + 1);
    let decoded = deserialize_state_root_record(12, &bytes).unwrap();
    assert_eq!(decoded.index, 12);
    assert_eq!(decoded.version, super::super::state_root::CURRENT_VERSION);
    assert_eq!(decoded.root_hash, root_hash);
    assert!(decoded.witness.is_none());

    // Signed record.
    let invocation = vec![0x01u8, 0x02, 0x03];
    let verification = vec![0x04u8, 0x05];
    let mut signed = StateRoot::new_current(13, root_hash);
    signed.witness = Some(neo_protocol::witness::Witness::new_with_scripts(
        invocation.clone(),
        verification.clone(),
    ));
    let bytes = serialize_state_root_record(&signed);
    let decoded = deserialize_state_root_record(13, &bytes).unwrap();
    let witness = decoded.witness.expect("witness round trip");
    assert_eq!(witness.invocation_script(), invocation);
    assert_eq!(witness.verification_script(), verification);

    // Malformed inputs are rejected (fail-closed).
    assert!(deserialize_state_root_record(1, &[]).is_none());
    assert!(deserialize_state_root_record(1, &bytes[..40]).is_none());
}

#[test]
fn test_committing_handler() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store.clone()));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    // Create minimal block
    let header = BlockHeader::default();
    let mut block = Block::new(header, vec![]);
    block.header.index = 42;

    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
    let app_executed: Vec<ApplicationExecuted> = vec![];

    // Call committing hook (no runtime context: direct-embedder fallback)
    handlers.blockchain_committing_handler(None, &block, &snapshot, &app_executed);

    // Should have computed state root
    let pending = handlers.pending_state_root.lock().unwrap();
    assert!(pending.is_some());
    assert_eq!(pending.as_ref().unwrap().index, 42);
}

#[test]
fn test_committed_handler_persists_and_announces() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));
    let captured: Arc<Mutex<Vec<ValidatedRootPersisted>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_capture = captured.clone();
    let handlers = StateServiceCommitHandlers::new(cache, true)
        .unwrap()
        .with_event_sink(Arc::new(move |event| {
            sink_capture.lock().unwrap().push(event);
        }));

    // First compute state root
    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
    assert!(handlers.commit_block(99, &snapshot).is_ok());
    let expected_root = pending_root(&handlers);

    // Create minimal block
    let header = BlockHeader::default();
    let mut block = Block::new(header, vec![]);
    block.header.index = 99;

    // Commit the child snapshot into its parent before the committed callback.
    handlers
        .store_cache
        .data_cache()
        .merge_tracked_items(&snapshot.tracked_items());
    snapshot.commit();

    handlers.blockchain_committed_handler(None, &block);

    // Should have persisted (cleared pending), stored the record and
    // announced the ValidatedRootPersisted event through the sink.
    assert!(handlers.pending_state_root.lock().unwrap().is_none());
    assert_eq!(handlers.current_root_hash(), Some(expected_root));
    assert_eq!(
        handlers.get_state_root(99).unwrap().root_hash,
        expected_root
    );

    let events = captured.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].index, 99);
}

#[test]
fn test_run_during_fast_sync() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store));

    let enabled_handlers = StateServiceCommitHandlers::new(Arc::clone(&cache), true).unwrap();
    let disabled_handlers = StateServiceCommitHandlers::new(cache, false).unwrap();

    assert!(enabled_handlers.run_during_fast_sync());
    assert!(!disabled_handlers.run_during_fast_sync());
}

#[test]
fn test_try_committing_handler() {
    let store = Arc::new(MemoryStore::new());
    let cache = Arc::new(StoreCache::new(store.clone()));
    let handlers = StateServiceCommitHandlers::new(cache, true).unwrap();

    let header = BlockHeader::default();
    let mut block = Block::new(header, vec![]);
    block.header.index = 77;

    let snapshot = snapshot_with_changes(&[(1, b"account".to_vec(), b"v1".to_vec())]);
    let app_executed: Vec<ApplicationExecuted> = vec![];

    // Call fallible version
    let result = handlers.try_blockchain_committing_handler(None, &block, &snapshot, &app_executed);

    assert!(result.is_ok());

    // Should have computed state root
    let pending = handlers.pending_state_root.lock().unwrap();
    assert!(pending.is_some());
    assert_eq!(pending.as_ref().unwrap().index, 77);
}
