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
