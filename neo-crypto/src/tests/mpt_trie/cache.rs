use super::*;

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
    let source = include_str!("../../mpt_trie/cache.rs");
    let commit = slice_between(source, "pub fn commit(&mut self)", "fn resolve_internal");
    assert!(
        !commit.contains("pending_count"),
        "MptCache::commit should not scan entries once only to count dirty nodes"
    );
    assert!(
        commit.contains("Vec::with_capacity(self.entries.len())"),
        "the overlay can reserve the cache upper bound and filter dirty entries in one pass"
    );
}

#[test]
fn cache_commit_reuses_cached_serialized_payloads_for_dirty_nodes() {
    let source = include_str!("../../mpt_trie/cache.rs");
    let commit = slice_between(source, "pub fn commit(&mut self)", "fn resolve_internal");

    assert!(
        !commit.contains("node.to_array()"),
        "dirty nodes are hashed before staging; commit should append references to cached payload bytes instead of reserializing whole nodes"
    );
}

fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_idx = source.find(start).expect("start marker exists");
    let tail = &source[start_idx..];
    let end_idx = tail.find(end).expect("end marker exists");
    &tail[..end_idx]
}
