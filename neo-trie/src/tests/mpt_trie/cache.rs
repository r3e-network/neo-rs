use super::*;
use crate::mpt::cache::{ProcessResourceSnapshot, proc_io_counter, proc_stat_faults};

// ============================================================================
// UT_Cache.cs Tests (12 tests)
// ============================================================================

#[test]
fn test_resolve_leaf() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let leaf = Node::new_leaf(vec![1, 2, 3]);
    let hash = leaf.hash();
    cache.put_node(leaf.clone()).unwrap();

    let resolved = cache.resolve(&hash).unwrap();
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap().value, vec![1, 2, 3]);
}

#[test]
fn test_resolve_branch() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let branch = Node::new_branch();
    let hash = branch.hash();
    cache.put_node(branch.clone()).unwrap();

    let resolved = cache.resolve(&hash).unwrap();
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap().node_type, NodeType::BranchNode);
}

#[test]
fn test_resolve_extension() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let leaf = Node::new_leaf(vec![1, 2, 3]);
    let ext = Node::new_extension(vec![0x01, 0x02], leaf).unwrap();
    let hash = ext.hash();
    cache.put_node(ext.clone()).unwrap();

    let resolved = cache.resolve(&hash).unwrap();
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap().node_type, NodeType::ExtensionNode);
}

#[test]
fn test_get_and_changed_branch() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let branch = Node::new_branch();
    let hash = branch.hash();
    cache.put_node(branch.clone()).unwrap();

    let mut resolved = cache.resolve(&hash).unwrap().unwrap();
    resolved.set_child(0, Node::new_leaf(vec![1, 2, 3]));
    // Verify the modified node has a different hash
    let new_hash = resolved.hash();
    assert_ne!(hash, new_hash);
    cache.put_node(resolved).unwrap();
}

#[test]
fn test_get_and_changed_extension() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let leaf = Node::new_leaf(vec![1, 2, 3]);
    let ext = Node::new_extension(vec![0x01], leaf).unwrap();
    let hash = ext.hash();
    cache.put_node(ext.clone()).unwrap();

    let mut resolved = cache.resolve(&hash).unwrap().unwrap();
    resolved.next = Some(Arc::new(Node::new_leaf(vec![4, 5, 6])));
    // Verify the modified node has a different hash
    let new_hash = resolved.hash();
    assert_ne!(hash, new_hash);
    cache.put_node(resolved).unwrap();
}

#[test]
fn test_get_and_changed_leaf() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let leaf = Node::new_leaf(vec![1, 2, 3]);
    let hash = leaf.hash();
    cache.put_node(leaf.clone()).unwrap();

    let mut resolved = cache.resolve(&hash).unwrap().unwrap();
    resolved.value = vec![4, 5, 6];
    resolved.set_dirty();
    // Verify the modified node has a different hash
    let new_hash = resolved.hash();
    assert_ne!(hash, new_hash);
    cache.put_node(resolved).unwrap();
}

#[test]
fn test_put_and_changed_branch() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let mut branch = Node::new_branch();
    branch.set_child(0, Node::new_leaf(vec![1, 2, 3]));
    let hash1 = branch.hash();
    cache.put_node(branch.clone()).unwrap();

    branch.set_child(1, Node::new_leaf(vec![4, 5, 6]));
    branch.set_dirty();
    let hash2 = branch.hash();
    cache.put_node(branch).unwrap();

    assert_ne!(hash1, hash2);
}

#[test]
fn test_put_and_changed_extension() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let leaf1 = Node::new_leaf(vec![1, 2, 3]);
    let ext1 = Node::new_extension(vec![0x01], leaf1).unwrap();
    let hash1 = ext1.hash();
    cache.put_node(ext1).unwrap();

    let leaf2 = Node::new_leaf(vec![4, 5, 6]);
    let ext2 = Node::new_extension(vec![0x01], leaf2).unwrap();
    let hash2 = ext2.hash();
    cache.put_node(ext2).unwrap();

    assert_ne!(hash1, hash2);
}

#[test]
fn test_put_and_changed_leaf() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let leaf1 = Node::new_leaf(vec![1, 2, 3]);
    let hash1 = leaf1.hash();
    cache.put_node(leaf1).unwrap();

    let leaf2 = Node::new_leaf(vec![4, 5, 6]);
    let hash2 = leaf2.hash();
    cache.put_node(leaf2).unwrap();

    assert_ne!(hash1, hash2);
}

#[test]
fn test_cache_reference1() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let leaf = Node::new_leaf(vec![1, 2, 3]);
    cache.put_node(leaf.clone()).unwrap();
    cache.put_node(leaf.clone()).unwrap();

    // Reference counting should handle duplicate puts
    assert_eq!(leaf.reference, 1);
}

#[test]
fn test_cache_reference2() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);

    let leaf = Node::new_leaf(vec![1, 2, 3]);
    let hash = leaf.hash();
    cache.put_node(leaf.clone()).unwrap();

    cache.delete_node(hash).unwrap();

    let resolved = cache.resolve(&hash).unwrap();
    assert!(resolved.as_ref().map(|n| n.is_empty()).unwrap_or(true));
}

#[test]
fn test_cache_commit() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store.clone(), 0xf0);

    let leaf = Node::new_leaf(vec![1, 2, 3]);
    cache.put_node(leaf).unwrap();

    cache.commit().unwrap();

    let data = store.get_data();
    assert!(!data.is_empty());
}

#[test]
fn finalization_stats_separate_cache_hits_from_backing_hits_and_misses() {
    let store = Arc::new(MockStore::new());
    let persisted = Node::new_leaf(vec![1, 2, 3]);
    let transient = Node::new_leaf(vec![4, 5, 6]);

    let mut seed = MptCache::new(Arc::clone(&store), 0xf0);
    seed.put_node(persisted.clone()).unwrap();
    seed.commit().unwrap();

    let mut cache = MptCache::new(store, 0xf0);
    cache.put_node(persisted).unwrap();
    cache.put_node(transient.clone()).unwrap();
    cache.put_node(transient).unwrap();

    let stats = cache.mutation_stats();
    assert_eq!(stats.finalization_cache_hits, 1);
    assert_eq!(stats.finalization_memory_hits, 0);
    assert_eq!(stats.finalization_memory_misses, 0);
    assert_eq!(stats.finalization_backing_hits, 1);
    assert_eq!(stats.finalization_backing_misses, 1);
    assert_eq!(stats.finalization_lookup_errors, 0);
}

#[test]
fn trie_resolution_stats_separate_cache_hits_from_store_hits_and_misses() {
    let store = Arc::new(MockStore::new());
    let persisted = Node::new_leaf(vec![1, 2, 3]);
    let persisted_hash = seed_node(store.as_ref(), 0xf0, &persisted);
    let missing_hash = Node::new_leaf(vec![4, 5, 6]).try_hash().unwrap();

    let mut cache = MptCache::new(store, 0xf0);
    assert!(cache.resolve(&persisted_hash).unwrap().is_some());
    assert!(cache.resolve(&persisted_hash).unwrap().is_some());
    assert!(cache.resolve(&missing_hash).unwrap().is_none());
    assert!(cache.resolve(&missing_hash).unwrap().is_none());

    let stats = cache.mutation_stats();
    assert_eq!(stats.trie_resolve_cache_hits, 2);
    assert_eq!(stats.trie_resolve_store_hits, 1);
    assert_eq!(stats.trie_resolve_store_misses, 1);
}

#[test]
fn single_deferred_node_matches_bulk_and_cache_hit_accounting() {
    let direct_store = Arc::new(MockStore::new());
    let bulk_store = Arc::new(MockStore::new());
    let mut direct = MptCache::new_deferred(direct_store.clone(), 0xf0);
    let mut bulk = MptCache::new_deferred(bulk_store.clone(), 0xf0);
    let node = Node::new_leaf(vec![1, 2, 3]);

    for _ in 0..2 {
        let mut direct_node = node.clone();
        direct.defer_intermediate_node(&mut direct_node).unwrap();

        let mut bulk_node = node.clone();
        let pending = bulk.prepare_node_finalization(&mut bulk_node).unwrap();
        bulk.finalize_prepared_nodes(vec![pending]).unwrap();
    }

    assert_eq!(
        direct.mutation_stats().finalization_cache_hits,
        bulk.mutation_stats().finalization_cache_hits
    );
    direct.commit().unwrap();
    bulk.commit().unwrap();
    assert_eq!(direct_store.get_data(), bulk_store.get_data());
}

#[test]
fn cached_put_materializes_only_when_resolved() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store, 0xf0);
    let mut branch = prepare_mpt_node3();

    cache.put_node_cached(&mut branch).unwrap();
    let hash = branch.try_hash().unwrap();
    assert_eq!(cache.materialized_entry_count(), 0);

    let resolved = cache
        .resolve(&hash)
        .unwrap()
        .expect("cached branch resolves");
    assert_eq!(resolved.node_type, NodeType::BranchNode);
    assert_eq!(resolved.try_hash().unwrap(), hash);
    assert_eq!(cache.materialized_entry_count(), 1);
}

fn cache_key(prefix: u8, hash: &UInt256) -> Vec<u8> {
    let mut key = Vec::with_capacity(33);
    key.push(prefix);
    key.extend_from_slice(&hash.to_array());
    key
}

fn stage_prepared_put<S>(cache: &mut MptCache<S>, node: &Node) -> UInt256
where
    S: MptStoreSnapshot,
{
    let mut node = node.clone();
    let hash = node.try_hash().expect("test node hashes");
    let pending = cache
        .prepare_node_finalization(&mut node)
        .expect("test node prepares");
    cache
        .finalize_prepared_nodes(vec![pending])
        .expect("prepared node stages");
    hash
}

fn seed_node<S>(store: &S, prefix: u8, node: &Node) -> UInt256
where
    S: MptStoreSnapshot,
{
    let hash = node.try_hash().expect("seed node hashes");
    store
        .put(
            cache_key(prefix, &hash),
            node.to_array().expect("seed node serializes"),
        )
        .expect("seed node persists");
    hash
}

fn stored_node(store: &MockStore, prefix: u8, hash: &UInt256) -> Option<Node> {
    store
        .get_data()
        .get(&cache_key(prefix, hash))
        .map(|bytes| deserialize_node(bytes))
}

#[test]
fn deferred_reference_operations_preserve_absent_and_existing_order() {
    const PREFIX: u8 = 0xf0;
    let store = Arc::new(MockStore::new());

    let mut existing_put_delete = Node::new_leaf(b"existing-put-delete".to_vec());
    existing_put_delete.reference = 1;
    let existing_put_delete_hash = seed_node(store.as_ref(), PREFIX, &existing_put_delete);

    let mut existing_delete_put = Node::new_leaf(b"existing-delete-put".to_vec());
    existing_delete_put.reference = 1;
    let existing_delete_put_hash = seed_node(store.as_ref(), PREFIX, &existing_delete_put);

    let absent_cancelled = Node::new_leaf(b"absent-cancelled".to_vec());
    let absent_cancelled_hash = absent_cancelled.try_hash().unwrap();
    let absent_readded = Node::new_leaf(b"absent-readded".to_vec());
    let absent_readded_hash = absent_readded.try_hash().unwrap();

    let mut cache = MptCache::new_deferred(Arc::clone(&store), PREFIX);

    stage_prepared_put(&mut cache, &absent_cancelled);
    cache.delete_node(absent_cancelled_hash).unwrap();

    stage_prepared_put(&mut cache, &absent_readded);
    cache.delete_node(absent_readded_hash).unwrap();
    stage_prepared_put(&mut cache, &absent_readded);

    stage_prepared_put(&mut cache, &existing_put_delete);
    cache.delete_node(existing_put_delete_hash).unwrap();

    cache.delete_node(existing_delete_put_hash).unwrap();
    stage_prepared_put(&mut cache, &existing_delete_put);

    cache.commit().unwrap();

    assert!(stored_node(&store, PREFIX, &absent_cancelled_hash).is_none());
    assert_eq!(
        stored_node(&store, PREFIX, &absent_readded_hash)
            .expect("put-delete-put recreates an absent node")
            .reference,
        1
    );
    assert_eq!(
        stored_node(&store, PREFIX, &existing_put_delete_hash)
            .expect("put-delete retains an existing node")
            .reference,
        1
    );
    assert_eq!(
        stored_node(&store, PREFIX, &existing_delete_put_hash)
            .expect("delete-put recreates an existing node")
            .reference,
        1
    );
}

#[test]
fn deferred_reference_replay_matches_eager_at_signed_overflow_boundary() {
    const PREFIX: u8 = 0xf0;
    let eager_store = Arc::new(MockStore::new());
    let deferred_store = Arc::new(MockStore::new());

    let mut reference_one = Node::new_leaf(b"reference-one".to_vec());
    reference_one.reference = 1;
    let reference_one_hash = seed_node(eager_store.as_ref(), PREFIX, &reference_one);
    seed_node(deferred_store.as_ref(), PREFIX, &reference_one);

    let mut reference_max = Node::new_leaf(b"reference-max".to_vec());
    reference_max.reference = i32::MAX;
    let reference_max_hash = seed_node(eager_store.as_ref(), PREFIX, &reference_max);
    seed_node(deferred_store.as_ref(), PREFIX, &reference_max);

    let mut eager = MptCache::new(Arc::clone(&eager_store), PREFIX);
    let mut deferred = MptCache::new_deferred(Arc::clone(&deferred_store), PREFIX);
    for node in [&reference_one, &reference_max] {
        let hash = node.try_hash().unwrap();
        stage_prepared_put(&mut eager, node);
        eager.delete_node(hash).unwrap();
        stage_prepared_put(&mut deferred, node);
        deferred.delete_node(hash).unwrap();
    }

    eager.commit().unwrap();
    deferred.commit().unwrap();
    assert_eq!(eager_store.get_data(), deferred_store.get_data());
    assert_eq!(
        stored_node(&deferred_store, PREFIX, &reference_one_hash)
            .unwrap()
            .reference,
        1
    );
    assert!(
        stored_node(&deferred_store, PREFIX, &reference_max_hash).is_none(),
        "unchecked C# int overflow makes the following delete remove the node"
    );
}

struct DeferredBatchProbeStore {
    data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    scalar_reads: std::sync::atomic::AtomicUsize,
    batch_reads: std::sync::atomic::AtomicUsize,
    overlay_writes: std::sync::atomic::AtomicUsize,
    fail_next_batch: std::sync::atomic::AtomicBool,
    batches: Mutex<Vec<Vec<Vec<u8>>>>,
}

impl DeferredBatchProbeStore {
    fn new(fail_next_batch: bool) -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
            scalar_reads: std::sync::atomic::AtomicUsize::new(0),
            batch_reads: std::sync::atomic::AtomicUsize::new(0),
            overlay_writes: std::sync::atomic::AtomicUsize::new(0),
            fail_next_batch: std::sync::atomic::AtomicBool::new(fail_next_batch),
            batches: Mutex::new(Vec::new()),
        }
    }

    fn stored_node(&self, prefix: u8, hash: &UInt256) -> Option<Node> {
        self.data
            .lock()
            .get(&cache_key(prefix, hash))
            .map(|bytes| deserialize_node(bytes))
    }
}

impl MptStoreSnapshot for DeferredBatchProbeStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        self.scalar_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(self.data.lock().get(key).cloned())
    }

    fn try_get_nodes_with_source(
        &self,
        keys: &[Vec<u8>],
    ) -> MptResult<Vec<crate::MptStoreLookup<Node>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        self.batch_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.batches.lock().push(keys.to_vec());
        if self
            .fail_next_batch
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            return Err(crate::MptError::storage(
                "injected deferred batch read failure",
            ));
        }

        let data = self.data.lock();
        Ok(keys
            .iter()
            .map(|key| {
                crate::MptStoreLookup::Backing(data.get(key).map(|bytes| deserialize_node(bytes)))
            })
            .collect())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.data.lock().insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.data.lock().remove(&key);
        Ok(())
    }

    fn apply_overlay(&self, overlay: Vec<(Vec<u8>, Option<Vec<u8>>)>) -> MptResult<()> {
        self.overlay_writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut data = self.data.lock();
        for (key, value) in overlay {
            match value {
                Some(value) => {
                    data.insert(key, value);
                }
                None => {
                    data.remove(&key);
                }
            }
        }
        Ok(())
    }
}

#[test]
fn deferred_entry_promotion_at_checkpoint_replays_once_and_skips_global_lookup() {
    const PREFIX: u8 = 0xf0;
    use std::sync::atomic::Ordering;

    let store = Arc::new(DeferredBatchProbeStore::new(false));
    let mut node = Node::new_leaf(b"promoted".to_vec());
    node.reference = 1;
    let hash = seed_node(store.as_ref(), PREFIX, &node);
    let mut cache = MptCache::new_deferred(Arc::clone(&store), PREFIX);

    stage_prepared_put(&mut cache, &node);
    cache.checkpoint();
    let _ = cache.take_mutation_stats();

    let promoted = cache
        .resolve(&hash)
        .unwrap()
        .expect("checkpointed deferred node promotes");
    assert_eq!(promoted.reference, 2);
    assert_eq!(store.scalar_reads.load(Ordering::Relaxed), 1);
    assert_eq!(cache.mutation_stats().finalization_backing_hits, 1);

    stage_prepared_put(&mut cache, &node);
    cache.delete_node(hash).unwrap();
    cache.commit().unwrap();

    assert_eq!(store.batch_reads.load(Ordering::Relaxed), 0);
    assert_eq!(store.scalar_reads.load(Ordering::Relaxed), 1);
    assert_eq!(store.stored_node(PREFIX, &hash).unwrap().reference, 2);
}

#[test]
fn deferred_commit_uses_one_sorted_unique_batch_and_is_retryable_after_metric_reset() {
    const PREFIX: u8 = 0xf0;
    use std::sync::atomic::Ordering;

    let store = Arc::new(DeferredBatchProbeStore::new(true));
    let mut cache = MptCache::new_deferred(Arc::clone(&store), PREFIX);
    let mut nodes = [
        Node::new_leaf(b"global-a".to_vec()),
        Node::new_leaf(b"global-b".to_vec()),
        Node::new_leaf(b"global-c".to_vec()),
    ];
    nodes.sort_unstable_by_key(|node| std::cmp::Reverse(node.hash().to_array()));
    let duplicated_hash = stage_prepared_put(&mut cache, &nodes[0]);
    for node in &nodes[1..] {
        stage_prepared_put(&mut cache, node);
    }
    stage_prepared_put(&mut cache, &nodes[0]);

    let staged_stats = cache.take_mutation_stats();
    assert_eq!(staged_stats.put_node_cached_calls, 4);
    assert_eq!(staged_stats.finalization_cache_hits, 1);

    let error = cache.commit().expect_err("first global lookup fails");
    assert!(error.to_string().contains("injected deferred batch"));
    assert_eq!(store.overlay_writes.load(Ordering::Relaxed), 0);
    assert!(store.data.lock().is_empty());
    let failed_stats = cache.take_mutation_stats();
    assert_eq!(failed_stats.finalization_lookup_errors, 3);
    assert_eq!(failed_stats.finalization_backing_misses, 0);

    cache
        .commit()
        .expect("deferred commit retries without restaging");
    let retry_stats = cache.take_mutation_stats();
    assert_eq!(retry_stats.finalization_lookup_errors, 0);
    assert_eq!(retry_stats.finalization_backing_misses, 3);
    assert_eq!(retry_stats.put_node_cached_calls, 0);
    assert_eq!(store.scalar_reads.load(Ordering::Relaxed), 0);
    assert_eq!(store.batch_reads.load(Ordering::Relaxed), 2);
    assert_eq!(store.overlay_writes.load(Ordering::Relaxed), 1);

    let batches = store.batches.lock();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0], batches[1]);
    assert_eq!(batches[0].len(), 3);
    assert!(batches[0].windows(2).all(|pair| pair[0] < pair[1]));
    drop(batches);

    assert_eq!(
        store
            .stored_node(PREFIX, &duplicated_hash)
            .unwrap()
            .reference,
        2,
        "duplicate payloads must replay as two ordered reference increments"
    );
}

#[test]
fn cache_commit_persists_nodes_with_exact_serialization_capacity() {
    let store = Arc::new(MockStore::new());
    let mut cache = MptCache::new(store.clone(), 0xf0);

    let mut branch = Node::new_branch();
    branch.set_child(0, Node::new_leaf(vec![1, 2, 3]));
    branch.set_child(1, Node::new_leaf(vec![4, 5, 6]));
    let expected_size = branch.size();

    cache.put_node(branch).unwrap();
    cache.commit().unwrap();

    let data = store.data.lock();
    let data = data
        .values()
        .next()
        .expect("cache commit writes serialized node");
    assert_eq!(data.len(), expected_size);
    assert_eq!(
        data.capacity(),
        expected_size,
        "cache commit should avoid reallocating serialized node buffers"
    );
}

#[test]
fn cache_commit_writes_staged_entries_through_one_bulk_overlay() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct BulkCountingStore {
        data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
        bulk_calls: AtomicUsize,
        single_writes: AtomicUsize,
        last_overlay_capacity: AtomicUsize,
    }

    impl BulkCountingStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
                bulk_calls: AtomicUsize::new(0),
                single_writes: AtomicUsize::new(0),
                last_overlay_capacity: AtomicUsize::new(0),
            }
        }
    }

    impl MptStoreSnapshot for BulkCountingStore {
        fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
            Ok(self.data.lock().get(key).cloned())
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
            self.single_writes.fetch_add(1, Ordering::Relaxed);
            self.data.lock().insert(key, value);
            Ok(())
        }

        fn delete(&self, key: Vec<u8>) -> MptResult<()> {
            self.single_writes.fetch_add(1, Ordering::Relaxed);
            self.data.lock().remove(&key);
            Ok(())
        }

        fn apply_overlay(&self, overlay: Vec<(Vec<u8>, Option<Vec<u8>>)>) -> MptResult<()> {
            self.bulk_calls.fetch_add(1, Ordering::Relaxed);
            self.last_overlay_capacity
                .store(overlay.capacity(), Ordering::Relaxed);
            let mut data = self.data.lock();
            for (key, value) in overlay {
                match value {
                    Some(value) => {
                        data.insert(key, value);
                    }
                    None => {
                        data.remove(&key);
                    }
                }
            }
            Ok(())
        }
    }

    let store = Arc::new(BulkCountingStore::new());
    let mut cache = MptCache::new(store.clone(), 0xf0);

    cache.put_node(Node::new_leaf(vec![1, 2, 3])).unwrap();
    cache.put_node(Node::new_leaf(vec![4, 5, 6])).unwrap();
    cache.commit().unwrap();

    assert_eq!(store.bulk_calls.load(Ordering::Relaxed), 1);
    assert_eq!(store.single_writes.load(Ordering::Relaxed), 0);
    assert_eq!(store.last_overlay_capacity.load(Ordering::Relaxed), 2);
    assert_eq!(store.data.lock().len(), 2);
}

#[test]
fn cache_commit_builds_overlay_in_one_dirty_entry_pass() {
    let source = include_str!("../../mpt/cache/operations.rs");
    let commit = slice_between(source, "pub fn commit(&mut self)", "fn resolve_internal");
    assert!(
        !commit.contains("pending_count"),
        "MptCache::commit should not scan entries once only to count dirty nodes"
    );
    assert!(
        commit.contains("Vec::with_capacity(")
            && commit.contains("self.entries")
            && commit.contains("self.deferred_entries.len()"),
        "the overlay should reserve both concrete and deferred cache upper bounds"
    );
}

#[test]
fn cache_commit_reuses_cached_serialized_payloads_for_dirty_nodes() {
    let source = include_str!("../../mpt/cache/operations.rs");
    let commit = slice_between(source, "pub fn commit(&mut self)", "fn resolve_internal");

    assert!(
        !commit.contains("node.to_array()"),
        "dirty nodes are hashed before staging; commit should append references to cached payload bytes instead of reserializing whole nodes"
    );
}

#[test]
fn deferred_resource_probe_parses_linux_proc_counters() {
    let io = "rchar: 10\nread_bytes: 4096\nwrite_bytes: 8192\n";
    assert_eq!(proc_io_counter(io, "read_bytes"), Some(4096));
    assert_eq!(proc_io_counter(io, "missing"), None);

    // The process name may contain spaces and closing parentheses; parsing
    // starts after the final closing parenthesis as `/proc/[pid]/stat` does.
    let stat = "123 (neo node (worker)) R 1 2 3 4 5 6 7 8 9 10";
    assert_eq!(proc_stat_faults(stat), Some((7, 9)));
}

#[test]
fn deferred_resource_delta_is_optional_and_saturating() {
    let mut stats = MptMutationStats::default();
    stats.record_deferred_resource_delta(
        Some(ProcessResourceSnapshot {
            read_bytes: 100,
            minor_faults: 8,
            major_faults: 4,
        }),
        Some(ProcessResourceSnapshot {
            read_bytes: 250,
            minor_faults: 11,
            major_faults: 5,
        }),
    );
    assert_eq!(stats.deferred_finalization_read_bytes, 150);
    assert_eq!(stats.deferred_finalization_minor_faults, 3);
    assert_eq!(stats.deferred_finalization_major_faults, 1);

    // A restricted/non-Linux process has no resource evidence; this must not
    // turn an otherwise valid MPT operation into an error or a false sample.
    stats.record_deferred_resource_delta(None, None);
    assert_eq!(stats.deferred_finalization_read_bytes, 150);
    assert_eq!(stats.deferred_finalization_minor_faults, 3);
    assert_eq!(stats.deferred_finalization_major_faults, 1);
}

fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_idx = source.find(start).expect("start marker exists");
    let tail = &source[start_idx..];
    let end_idx = tail.find(end).expect("end marker exists");
    &tail[..end_idx]
}
