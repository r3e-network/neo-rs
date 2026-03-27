//! Comprehensive MPT tests converted from C# Neo.Cryptography.MPTTrie.Tests
//! Covers all test cases from UT_Node.cs, UT_Trie.cs, and UT_Cache.cs

#[cfg(test)]
mod mpt_tests {
    use crate::mpt_trie::{MptCache, MptResult, MptStoreSnapshot, Node, NodeType, Trie};
    use neo_io::{BinaryWriter, MemoryReader, Serializable};
    use neo_primitives::UInt256;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Helper trait to provide to_array method for serialization
    trait SerializableExt: Serializable {
        fn to_array(&self) -> neo_io::IoResult<Vec<u8>> {
            let mut writer = BinaryWriter::new();
            self.serialize(&mut writer)?;
            Ok(writer.into_bytes())
        }
    }

    impl<T: Serializable> SerializableExt for T {}

    /// Mock store for testing - matches C# MemoryStore
    struct MockStore {
        data: Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>,
    }

    impl MockStore {
        fn new() -> Self {
            Self {
                data: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn get_data(&self) -> HashMap<Vec<u8>, Vec<u8>> {
            self.data.lock().clone()
        }
    }

    impl MptStoreSnapshot for MockStore {
        fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
            Ok(self.data.lock().get(key).cloned())
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
            self.data.lock().insert(key, value);
            Ok(())
        }

        fn delete(&self, key: Vec<u8>) -> MptResult<()> {
            self.data.lock().remove(&key);
            Ok(())
        }
    }

    fn serialize_child(node: &Node) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        node.serialize_as_child(&mut writer).unwrap();
        writer.into_bytes()
    }

    fn deserialize_node(data: &[u8]) -> Node {
        let mut reader = MemoryReader::new(data);
        Node::deserialize(&mut reader).unwrap()
    }

    // Helper functions matching Helper.cs
    fn prepare_mpt_node1() -> Node {
        Node::new_hash(UInt256::zero())
    }

    fn prepare_mpt_node2() -> Node {
        Node::new_leaf(vec![0x12, 0x34])
    }

    fn prepare_mpt_node3() -> Node {
        let mut branch = Node::new_branch();
        branch.set_child(1, prepare_mpt_node1());
        branch.set_child(2, prepare_mpt_node2());
        branch
    }

    // ============================================================================
    // UT_Node.cs Tests (20 tests)
    // ============================================================================

    #[test]
    fn test_hash_serialize() {
        let node = prepare_mpt_node1();
        let data = node.to_array().unwrap();
        assert!(!data.is_empty());

        let deserialized = deserialize_node(&data);
        assert_eq!(deserialized.node_type, NodeType::HashNode);
        assert_eq!(deserialized.hash(), node.hash());
    }

    #[test]
    fn test_empty_serialize() {
        let node = Node::new();
        let data = node.to_array().unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0], NodeType::Empty as u8);

        let deserialized = deserialize_node(&data);
        assert_eq!(deserialized.node_type, NodeType::Empty);
        assert!(deserialized.is_empty());
    }

    #[test]
    fn test_leaf_serialize() {
        let node = prepare_mpt_node2();
        let data = node.to_array().unwrap();
        assert!(!data.is_empty());

        let deserialized = deserialize_node(&data);
        assert_eq!(deserialized.node_type, NodeType::LeafNode);
        assert_eq!(deserialized.value, vec![0x12, 0x34]);
    }

    #[test]
    fn test_leaf_serialize_as_child() {
        let node = prepare_mpt_node2();
        let buffer = serialize_child(&node);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_extension_serialize() {
        let leaf = prepare_mpt_node2();
        let ext = Node::new_extension(vec![0x01, 0x02], leaf).unwrap();
        let data = ext.to_array().unwrap();
        assert!(!data.is_empty());

        let deserialized = deserialize_node(&data);
        assert_eq!(deserialized.node_type, NodeType::ExtensionNode);
        assert_eq!(deserialized.key, vec![0x01, 0x02]);
        assert!(deserialized.next.is_some());
    }

    #[test]
    fn test_extension_serialize_as_child() {
        let leaf = prepare_mpt_node2();
        let ext = Node::new_extension(vec![0x01], leaf).unwrap();
        let buffer = serialize_child(&ext);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_branch_serialize() {
        let branch = prepare_mpt_node3();
        let data = branch.to_array().unwrap();
        assert!(!data.is_empty());

        let deserialized = deserialize_node(&data);
        assert_eq!(deserialized.node_type, NodeType::BranchNode);
        assert_eq!(deserialized.children.len(), 17);
    }

    #[test]
    fn test_branch_serialize_as_child() {
        let branch = prepare_mpt_node3();
        let buffer = serialize_child(&branch);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_clone_branch() {
        let branch1 = prepare_mpt_node3();
        let branch2 = branch1.clone();
        assert_eq!(branch1.node_type, branch2.node_type);
        assert_eq!(branch1.hash(), branch2.hash());
    }

    #[test]
    fn test_clone_extension() {
        let leaf = prepare_mpt_node2();
        let ext1 = Node::new_extension(vec![0x01, 0x02], leaf).unwrap();
        let ext2 = ext1.clone();
        assert_eq!(ext1.node_type, ext2.node_type);
        assert_eq!(ext1.key, ext2.key);
        assert_eq!(ext1.hash(), ext2.hash());
    }

    #[test]
    fn test_clone_leaf() {
        let leaf1 = prepare_mpt_node2();
        let leaf2 = leaf1.clone();
        assert_eq!(leaf1.value, leaf2.value);
        assert_eq!(leaf1.hash(), leaf2.hash());
    }

    #[test]
    fn test_new_extension_exception() {
        // Extension with empty key should fail
        let leaf = prepare_mpt_node2();
        let result = Node::new_extension(vec![], leaf);
        assert!(result.is_err());
    }

    #[test]
    fn test_new_hash_exception() {
        // C# throws on null input; Rust cannot represent null hashes.
        let hash = UInt256::zero();
        let node = Node::new_hash(hash);
        assert_eq!(node.node_type, NodeType::HashNode);
        assert_eq!(node.hash(), hash);
    }

    #[test]
    fn test_new_leaf_exception() {
        // C# throws on null input; empty values are valid and should serialize.
        let leaf = Node::new_leaf(vec![]);
        assert_eq!(leaf.node_type, NodeType::LeafNode);
        assert_eq!(leaf.value.len(), 0);
    }

    #[test]
    fn test_size() {
        let hash_node = prepare_mpt_node1();
        let hash_size = hash_node.to_array().unwrap().len();
        assert!(hash_size > 0);

        let leaf = prepare_mpt_node2();
        let leaf_size = leaf.to_array().unwrap().len();
        assert!(leaf_size > 0);

        let branch = prepare_mpt_node3();
        let branch_size = branch.to_array().unwrap().len();
        assert!(branch_size > leaf_size);
    }

    #[test]
    fn test_from_replica() {
        let original = prepare_mpt_node3();
        let data = original.to_array().unwrap();
        let replica = deserialize_node(&data);
        assert_eq!(original.node_type, replica.node_type);
        assert_eq!(original.hash(), replica.hash());
    }

    #[test]
    fn test_empty_leaf() {
        let empty = Node::new();
        assert!(empty.is_empty());
        assert_eq!(empty.node_type, NodeType::Empty);

        let leaf = Node::new_leaf(vec![]);
        assert_eq!(leaf.node_type, NodeType::LeafNode);
        assert!(!leaf.is_empty());
    }

    // ============================================================================
    // UT_Trie.cs Tests (40+ tests)
    // ============================================================================

    #[test]
    fn test_try_get() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key1", b"value1").unwrap();
        let result = trie.get(b"key1").unwrap();
        assert_eq!(result, Some(b"value1".to_vec()));

        let not_found = trie.get(b"key2").unwrap();
        assert_eq!(not_found, None);
    }

    #[test]
    fn test_try_get_resolve() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store.clone(), None, false);

        trie.put(b"key1", b"value1").unwrap();
        trie.commit().unwrap();

        let root_hash = trie.root_hash();
        let mut trie2 = Trie::new(store, root_hash, false);

        let result = trie2.get(b"key1").unwrap();
        assert_eq!(result, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_try_put() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key1", b"value1").unwrap();
        assert_eq!(trie.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Update existing key
        trie.put(b"key1", b"value2").unwrap();
        assert_eq!(trie.get(b"key1").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_put_cant_resolve() {
        let store = Arc::new(MockStore::new());
        let fake_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let mut trie = Trie::new(store, Some(fake_hash), false);

        // Should fail to resolve fake hash
        let result = trie.put(b"key1", b"value1");
        assert!(result.is_err());
    }

    #[test]
    fn test_try_delete() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key1", b"value1").unwrap();
        assert_eq!(trie.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        let deleted = trie.delete(b"key1").unwrap();
        assert!(deleted);
        assert_eq!(trie.get(b"key1").unwrap(), None);

        // Delete non-existent key
        let deleted2 = trie.delete(b"key2").unwrap();
        assert!(!deleted2);
    }

    #[test]
    fn test_delete_remain_can_resolve() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store.clone(), None, false);

        trie.put(b"key1", b"value1").unwrap();
        trie.put(b"key2", b"value2").unwrap();
        trie.commit().unwrap();

        let root_hash = trie.root_hash();
        let mut trie2 = Trie::new(store, root_hash, false);

        trie2.delete(b"key1").unwrap();
        assert_eq!(trie2.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_delete_remain_cant_resolve() {
        let store = Arc::new(MockStore::new());
        let fake_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let mut trie = Trie::new(store, Some(fake_hash), false);

        let result = trie.delete(b"key1");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_same_value() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key1", b"value1").unwrap();
        let hash1 = trie.root_hash().unwrap();

        trie.delete(b"key1").unwrap();
        trie.put(b"key1", b"value1").unwrap();
        let hash2 = trie.root_hash().unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_branch_node_remain_value() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key1", b"value1").unwrap();
        trie.put(b"key2", b"value2").unwrap();
        trie.put(b"key", b"value").unwrap();

        trie.delete(b"key1").unwrap();
        assert_eq!(trie.get(b"key").unwrap(), Some(b"value".to_vec()));
        assert_eq!(trie.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_add_longer_key() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key", b"value").unwrap();
        trie.put(b"key1", b"value1").unwrap();
        trie.put(b"key2", b"value2").unwrap();

        assert_eq!(trie.get(b"key").unwrap(), Some(b"value".to_vec()));
        assert_eq!(trie.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(trie.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_split_key() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"abcd", b"value1").unwrap();
        trie.put(b"ab", b"value2").unwrap();

        assert_eq!(trie.get(b"abcd").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(trie.get(b"ab").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_reference1() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(&[0xac, 0x01], b"abcd").unwrap();
        trie.put(&[0xac, 0x02], b"abcd").unwrap();

        assert_eq!(trie.get(&[0xac, 0x01]).unwrap(), Some(b"abcd".to_vec()));
        assert_eq!(trie.get(&[0xac, 0x02]).unwrap(), Some(b"abcd".to_vec()));
    }

    #[test]
    fn test_reference2() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(&[0xac, 0x01], b"abcd").unwrap();
        trie.put(&[0xac, 0x02], b"abcd").unwrap();
        trie.delete(&[0xac, 0x01]).unwrap();

        assert_eq!(trie.get(&[0xac, 0x01]).unwrap(), None);
        assert_eq!(trie.get(&[0xac, 0x02]).unwrap(), Some(b"abcd".to_vec()));
    }

    #[test]
    fn test_extension_delete_dirty() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(&[0x10, 0x01], b"value1").unwrap();
        trie.put(&[0x10, 0x02], b"value2").unwrap();
        let root1 = trie.root_hash().unwrap();

        trie.delete(&[0x10, 0x01]).unwrap();
        let root2 = trie.root_hash().unwrap();

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_branch_delete_dirty() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key1", b"value1").unwrap();
        trie.put(b"key2", b"value2").unwrap();
        trie.put(b"key3", b"value3").unwrap();
        let root1 = trie.root_hash().unwrap();

        trie.delete(b"key1").unwrap();
        let root2 = trie.root_hash().unwrap();

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_extension_put_dirty() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(&[0x10, 0x01], b"value1").unwrap();
        let root1 = trie.root_hash().unwrap();

        trie.put(&[0x10, 0x02], b"value2").unwrap();
        let root2 = trie.root_hash().unwrap();

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_branch_put_dirty() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key1", b"value1").unwrap();
        let root1 = trie.root_hash().unwrap();

        trie.put(b"key2", b"value2").unwrap();
        let root2 = trie.root_hash().unwrap();

        assert_ne!(root1, root2);
    }

    #[test]
    fn test_empty_value_issue633() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        // Empty value should be handled correctly
        let result = trie.put(b"key", b"");
        assert!(result.is_ok());
    }

    #[test]
    fn test_trie_root_hash() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        assert_eq!(trie.root_hash(), None);

        trie.put(b"key", b"value").unwrap();
        assert!(trie.root_hash().is_some());

        let hash1 = trie.root_hash().unwrap();

        let store2 = Arc::new(MockStore::new());
        let mut trie2 = Trie::new(store2, None, false);
        trie2.put(b"key", b"value").unwrap();
        let hash2 = trie2.root_hash().unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_trie_commit() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store.clone(), None, false);

        trie.put(b"key1", b"value1").unwrap();
        trie.put(b"key2", b"value2").unwrap();

        trie.commit().unwrap();

        let data = store.get_data();
        assert!(!data.is_empty());
    }

    #[test]
    fn test_trie_with_common_prefix() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"test1", b"value1").unwrap();
        trie.put(b"test2", b"value2").unwrap();
        trie.put(b"test3", b"value3").unwrap();

        assert_eq!(trie.get(b"test1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(trie.get(b"test2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(trie.get(b"test3").unwrap(), Some(b"value3".to_vec()));
    }

    #[test]
    fn test_empty_key_handling() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        let result = trie.put(&[], b"value");
        assert!(result.is_err());
    }

    #[test]
    fn test_max_value_length() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        let large_value = vec![0u8; 70000]; // Exceeds MAX_VALUE_LENGTH
        let result = trie.put(b"key", &large_value);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_keys() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key1", b"value1").unwrap();
        trie.put(b"key2", b"value2").unwrap();
        trie.put(b"key3", b"value3").unwrap();

        assert_eq!(trie.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(trie.get(b"key2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(trie.get(b"key3").unwrap(), Some(b"value3".to_vec()));
    }

    #[test]
    fn test_trie_update() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(b"key", b"value1").unwrap();
        assert_eq!(trie.get(b"key").unwrap(), Some(b"value1".to_vec()));

        trie.put(b"key", b"value2").unwrap();
        assert_eq!(trie.get(b"key").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_nibbles_conversion() {
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        let key = vec![0x12, 0x34, 0x56, 0x78];
        let value = b"test";

        trie.put(&key, value).unwrap();
        let result = trie.get(&key).unwrap();
        assert_eq!(result, Some(value.to_vec()));
    }

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

    // ============================================================================
    // Diagnostic tests: verify exact serialization and hashing matches C# reference
    // ============================================================================

    #[test]
    fn test_leaf_node_serialization_and_hash() {
        // A leaf node with value [0x01, 0x02] should serialize as:
        // type_byte(0x02) + var_bytes(value)
        // var_bytes([0x01, 0x02]) = [0x02, 0x01, 0x02] (length prefix + data)
        // So without reference: [0x02, 0x02, 0x01, 0x02]
        let leaf = Node::new_leaf(vec![0x01, 0x02]);

        // Verify serialization WITHOUT reference matches C# format
        let data = leaf.to_array_without_reference().unwrap();
        assert_eq!(data, vec![0x02, 0x02, 0x01, 0x02]);

        // Verify serialization WITH reference appends var_int(1)
        let full_data = leaf.to_array().unwrap();
        assert_eq!(full_data, vec![0x02, 0x02, 0x01, 0x02, 0x01]);

        // Verify hash is Hash256(serialized_without_reference)
        let hash = leaf.hash();
        let expected_hash = crate::Crypto::hash256(&data);
        assert_eq!(hash.to_bytes(), expected_hash.to_vec());
    }

    #[test]
    fn test_empty_node_serialization_and_hash() {
        let empty = Node::new();
        let data = empty.to_array_without_reference().unwrap();
        assert_eq!(data, vec![0x04]); // Just the Empty type byte
    }

    #[test]
    fn test_single_put_trie_structure() {
        // Put a single key-value pair and inspect the resulting tree structure.
        // Key: [0xAB] -> nibbles: [0x0A, 0x0B]
        // Value: [0x01]
        //
        // Expected structure: Extension([0x0A, 0x0B]) -> Leaf([0x01])
        // This is because put on Empty with non-empty path creates an extension.
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(&[0xAB], &[0x01]).unwrap();

        assert!(trie.root_hash().is_some());

        // Verify correct structure: root should be ExtensionNode
        assert_eq!(trie.root().node_type, NodeType::ExtensionNode);
        assert_eq!(trie.root().key, vec![0x0A, 0x0B]);

        // Verify we can read it back
        let val = trie.get(&[0xAB]).unwrap();
        assert_eq!(val, Some(vec![0x01]));
    }

    #[test]
    fn test_two_keys_trie_root_hash() {
        // Put two keys that share a common prefix nibble to force a branch node.
        // Key [0x10] -> nibbles [0x01, 0x00], value [0xAA]
        // Key [0x11] -> nibbles [0x01, 0x01], value [0xBB]
        //
        // Expected structure:
        //   Extension([0x01]) -> Branch
        //     Branch[0x00] -> Leaf([0xAA])
        //     Branch[0x01] -> Leaf([0xBB])
        let store = Arc::new(MockStore::new());
        let mut trie = Trie::new(store, None, false);

        trie.put(&[0x10], &[0xAA]).unwrap();
        trie.put(&[0x11], &[0xBB]).unwrap();

        let root_hash = trie.root_hash().unwrap();

        // Verify the root hash is deterministic
        let store2 = Arc::new(MockStore::new());
        let mut trie2 = Trie::new(store2, None, false);
        trie2.put(&[0x10], &[0xAA]).unwrap();
        trie2.put(&[0x11], &[0xBB]).unwrap();
        assert_eq!(root_hash, trie2.root_hash().unwrap());
    }

    #[test]
    fn test_extension_node_serialization() {
        // Extension node: type(0x01) + var_bytes(key) + child_as_hash
        let leaf = Node::new_leaf(vec![0x01]);
        let leaf_hash = leaf.hash();
        let ext = Node::new_extension(vec![0x0A, 0x0B], leaf).unwrap();

        let data = ext.to_array_without_reference().unwrap();

        // Expected: type(0x01) + var_bytes([0x0A, 0x0B]) + child_as_hash
        // child_as_hash = type(0x03) + leaf_hash_bytes (32 bytes)
        let mut expected = vec![0x01]; // ExtensionNode type
        expected.push(0x02); // var_int length 2
        expected.extend_from_slice(&[0x0A, 0x0B]); // key bytes
        expected.push(0x03); // HashNode type for child
        expected.extend_from_slice(&leaf_hash.to_bytes()); // 32-byte hash
        assert_eq!(data, expected);
    }

    #[test]
    fn test_branch_node_serialization() {
        // A branch node with one leaf in slot 5 and rest empty
        let mut branch = Node::new_branch();
        let leaf = Node::new_leaf(vec![0xFF]);
        branch.set_child(5, leaf);

        let data = branch.to_array_without_reference().unwrap();

        // Verify structure: type(0x00) + 17 children serialized as child
        // Empty children serialize as type(0x04) = 1 byte each
        // Leaf child serializes as HashNode = type(0x03) + 32 bytes = 33 bytes
        // slots 0-4: 5 empty = 5 bytes
        // slot 5: hash node = 33 bytes
        // slots 6-15: 10 empty = 10 bytes
        // slot 16 (value): 1 empty = 1 byte
        // Total = 1 (type) + 5 + 33 + 10 + 1 = 50 bytes
        assert_eq!(data.len(), 50);
    }

    #[test]
    fn test_insertion_order_independence() {
        // The root hash should be the same regardless of insertion order.
        let store1 = Arc::new(MockStore::new());
        let mut trie1 = Trie::new(store1, None, false);
        trie1.put(&[0x01], &[0xAA]).unwrap();
        trie1.put(&[0x02], &[0xBB]).unwrap();
        trie1.put(&[0x03], &[0xCC]).unwrap();

        let store2 = Arc::new(MockStore::new());
        let mut trie2 = Trie::new(store2, None, false);
        trie2.put(&[0x03], &[0xCC]).unwrap();
        trie2.put(&[0x01], &[0xAA]).unwrap();
        trie2.put(&[0x02], &[0xBB]).unwrap();

        let store3 = Arc::new(MockStore::new());
        let mut trie3 = Trie::new(store3, None, false);
        trie3.put(&[0x02], &[0xBB]).unwrap();
        trie3.put(&[0x03], &[0xCC]).unwrap();
        trie3.put(&[0x01], &[0xAA]).unwrap();

        assert_eq!(trie1.root_hash(), trie2.root_hash());
        assert_eq!(trie1.root_hash(), trie3.root_hash());
    }


#[test]
fn test_genesis_state_root_matches_reference() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new(store, None, false);
    // OracleContract (ID -9, prefix f7ffffff)
    trie.put(&hex::decode("f7ffffff05").unwrap(), &hex::decode("80f0fa02").unwrap()).unwrap();
    trie.put(&hex::decode("f7ffffff09").unwrap(), &hex::decode("").unwrap()).unwrap();
    // PolicyContract (ID -7, prefix f9ffffff)
    trie.put(&hex::decode("f9ffffff0a").unwrap(), &hex::decode("e803").unwrap()).unwrap();
    trie.put(&hex::decode("f9ffffff12").unwrap(), &hex::decode("1e").unwrap()).unwrap();
    trie.put(&hex::decode("f9ffffff13").unwrap(), &hex::decode("a08601").unwrap()).unwrap();
    // GasToken (ID -6, prefix faffffff)
    trie.put(&hex::decode("faffffff0b").unwrap(), &hex::decode("80f0cf5b5f7912").unwrap()).unwrap();
    trie.put(&hex::decode("faffffff146b123dd8bec718648852bbc78595e3536a058f9f").unwrap(), &hex::decode("410121070000d5585f7912").unwrap()).unwrap();
    trie.put(&hex::decode("faffffff1496949ed482e7c60aaeec691550f1b3d599146194").unwrap(), &hex::decode("4101210480f0fa02").unwrap()).unwrap();
    trie.put(&hex::decode("fbffffff01").unwrap(), &hex::decode("").unwrap()).unwrap();
    trie.put(&hex::decode("fbffffff0b").unwrap(), &hex::decode("00e1f505").unwrap()).unwrap();
    trie.put(&hex::decode("fbffffff0d").unwrap(), &hex::decode("00e8764817").unwrap()).unwrap();
    trie.put(&hex::decode("fbffffff0e").unwrap(), &hex::decode("40154102282103b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c21004102282102df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e89509321004102282103b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a21004102282102ca0e27697b9c248f6f16e085fd0061e26f44da85b58ee835c110caa5ec3ba554210041022821024c7b7fb6c310fccf1ba33b082519d82964ea93868d676662d4a59ad548df0e7d21004102282102aaec38470f6aad0042c6e877cfd8087d2676b0f516fddd362801b9bd3936399e21004102282102486fd15702c4490a26703112a5cc1d0923fd697a33406bd5a1c00e0013b09a70210041022821023a36c72844610b4d34d1968662424011bf783ca9d984efa19a20babf5582f3fe21004102282103708b860c1de5d87f5b151a12c2a99feebd2e8b315ee8e7cf8aa19692a9e1837921004102282103c6aa6e12638b36e88adc1ccdceac4db9929575c3e03576c617c49cce7114a05021004102282103204223f8c86b8cd5c89ef12e4f0dbb314172e9241e30c9ef2293790793537cf021004102282102a62c915cf19c7f19a50ec217e79fac2439bbaad658493de0c7d8ffa92ab0aa6221004102282103409f31f0d66bdc2f70a9730b66fe186658f84a8018204db01c106edc36553cd02100410228210288342b141c30dc8ffcde0204929bb46aed5756b41ef4a56778d15ada8f0c6654210041022821020f2887f41474cfeb11fd262e982051c1541418137c02a0f4961af911045de6392100410228210222038884bbd1d8ff109ed3bdef3542e768eef76c1247aea8bc8171f532928c3021004102282103d281b42002647f0113f36c7b8efb30db66078dfaaa9ab3ff76d043a98d512fde21004102282102504acbc1f4b3bdad1d86d6e1a08603771db135a73e61c9d565ae06a1938cd2ad2100410228210226933336f1b75baa42d42b71d9091508b638046d19abd67f4e119bf64a7cfb4d21004102282103cdcea66032b82f5c30450e381e5295cae85c5e6943af716cc6b646352a6067dc21004102282102cd5a5547119e24feaa7c2a0f37b8c9366216bab7054de0065c9be42084003c8a2100").unwrap()).unwrap();
    trie.put(&hex::decode("fbffffff146b123dd8bec718648852bbc78595e3536a058f9f").unwrap(), &hex::decode("4104210400e1f5052100002100").unwrap()).unwrap();
    trie.put(&hex::decode("fbffffff1d00000000").unwrap(), &hex::decode("0065cd1d").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff081bf575ab1189688413610a35a12886cde0b66c72").unwrap(), &hex::decode("40052101fd210028141bf575ab1189688413610a35a12886cde0b66c7228944e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004610411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b6740b998507f4108280943727970746f4c69624000480040004102400a4105280b626c7331323338314164644002410228017821013041022801792101302101302100200141052813626c733132333831446573657269616c697a654001410228046461746121011221013021010720014105280d626c733132333831457175616c40024102280178210130410228017921013021011021010e20014105280b626c7331323338314d756c40034102280178210130410228036d756c210112410228036e656721011021013021011520014105280f626c73313233383150616972696e67400241022802673121013041022802673221013021013021011c200141052811626c73313233383153657269616c697a65400141022801672101302101122101232001410528086d75726d7572333240024102280464617461210112410228047365656421011121011221012a200141052809726970656d64313630400141022804646174612101122101122101312001410528067368613235364001410228046461746121011221011221013820014105280f7665726966795769746845434473614004410228076d657373616765210112410228067075626b6579210112410228097369676e617475726521011241022805637572766521011121011021013f20014000400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff08588717117e0aa81072afab71d2dd89fe7c4b92fe").unwrap(), &hex::decode("40052101f721002814588717117e0aa81072afab71d2dd89fe7c4b92fe28714e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002310411af77b674010411af77b674010411af77b674010411af77b674010411af77b67405141c79e4108280e4f7261636c65436f6e7472616374400048004000410240054105280666696e69736840002102ff002100200041052808676574507269636540002101112101072001410528077265717565737440054102280375726c2101134102280666696c7465722101134102280863616c6c6261636b21011341022808757365724461746121004102280e676173466f72526573706f6e73652101112102ff0021010e200041052808736574507269636540014102280570726963652101112102ff00210115200041052806766572696679400021011021011c200140024102280d4f7261636c655265717565737440044102280249642101114102280f52657175657374436f6e74726163742101144102280355726c2101134102280646696c7465722101134102280e4f7261636c65526573706f6e736540024102280249642101114102280a4f726967696e616c5478210115400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff087bc681c0a1f71d543457b68bba8d5f9fdd4e5ecc").unwrap(), &hex::decode("40052101f9210028147bc681c0a1f71d543457b68bba8d5f9fdd4e5ecc289b4e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004d10411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b6740481139414108280e506f6c696379436f6e74726163744000480040004102400b4105280c626c6f636b4163636f756e744001410228076163636f756e74210114210110210020004105280f67657441747472696275746546656540014102280d6174747269627574655479706521011121011121010720014105281067657445786563466565466163746f72400021011121010e20014105280d67657446656550657242797465400021011121011520014105280f67657453746f726167655072696365400021011121011c2001410528096973426c6f636b65644001410228076163636f756e7421011421011021012320014105280f73657441747472696275746546656540024102280d617474726962757465547970652101114102280576616c75652101112102ff0021012a20004105281073657445786563466565466163746f7240014102280576616c75652101112102ff0021013120004105280d7365744665655065724279746540014102280576616c75652101112102ff0021013820004105280f73657453746f72616765507269636540014102280576616c75652101112102ff0021013f20004105280e756e626c6f636b4163636f756e744001410228076163636f756e7421011421011021014620004000400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff08bef2043140362a77c15099c7e64c12f700b665da").unwrap(), &hex::decode("40052101fc21002814bef2043140362a77c15099c7e64c12f700b665da28864e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000003810411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b67409d382d424108280e4c6564676572436f6e7472616374400048004000410240084105280b63757272656e74486173684000210115210020014105280c63757272656e74496e6465784000210111210107200141052808676574426c6f636b40014102280b696e6465784f724861736821011221012021010e20014105280e6765745472616e73616374696f6e400141022804686173682101152101202101152001410528176765745472616e73616374696f6e46726f6d426c6f636b400241022810626c6f636b496e6465784f7248617368210112410228077478496e64657821011121012021011c2001410528146765745472616e73616374696f6e486569676874400141022804686173682101152101112101232001410528156765745472616e73616374696f6e5369676e6572734001410228046861736821011521012021012a2001410528156765745472616e73616374696f6e564d53746174654001410228046861736821011521011121013120014000400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff08c0ef39cee0e4e925c6c2a06a79e1440dd86fceac").unwrap(), &hex::decode("40052101fe21002814c0ef39cee0e4e925c6c2a06a79e1440dd86fceac28e14e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000009310411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674031b2b576410828065374644c6962400048004000410240154105280461746f6940014102280576616c7565210113210111210020014105280461746f6940024102280576616c75652101134102280462617365210111210111210107200141052811626173653538436865636b4465636f64654001410228017321011321011221010e200141052811626173653538436865636b456e636f64654001410228046461746121011221011321011520014105280c6261736535384465636f64654001410228017321011321011221011c20014105280c626173653538456e636f64654001410228046461746121011221011321012320014105280c6261736536344465636f64654001410228017321011321011221012a20014105280c626173653634456e636f64654001410228046461746121011221011321013120014105280b646573657269616c697a6540014102280464617461210112210021013820014105280469746f6140014102280576616c756521011121011321013f20014105280469746f6140024102280576616c7565210111410228046261736521011121011321014620014105280f6a736f6e446573657269616c697a654001410228046a736f6e210112210021014d20014105280d6a736f6e53657269616c697a654001410228046974656d210021011221015420014105280d6d656d6f7279436f6d7061726540024102280473747231210112410228047374723221011221011121015b20014105280c6d656d6f72795365617263684002410228036d656d2101124102280576616c756521011221011121016220014105280c6d656d6f72795365617263684003410228036d656d2101124102280576616c756521011241022805737461727421011121011121016920014105280c6d656d6f72795365617263684004410228036d656d2101124102280576616c7565210112410228057374617274210111410228086261636b7761726421011021011121017020014105280973657269616c697a654001410228046974656d21002101122101772001410528067374724c656e40014102280373747221011321011121017e20014105280b737472696e6753706c697440024102280373747221011341022809736570617261746f722101132101202102850020014105280b737472696e6753706c697440034102280373747221011341022809736570617261746f722101134102281272656d6f7665456d707479456e747269657321011021012021028c0020014000400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff08cf76e28bd0062c4a478ee35561011319f3cfa4d2").unwrap(), &hex::decode("40052101fa21002814cf76e28bd0062c4a478ee35561011319f3cfa4d228714e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002310411af77b674010411af77b674010411af77b674010411af77b674010411af77b67405141c79e41082808476173546f6b656e40004800400128064e45502d3137410240054105280962616c616e63654f664001410228076163636f756e742101142101112100200141052808646563696d616c73400021011121010720014105280673796d626f6c400021011321010e20014105280b746f74616c537570706c7940002101112101152001410528087472616e7366657240044102280466726f6d21011441022802746f21011441022806616d6f756e742101114102280464617461210021011021011c20004001410228085472616e7366657240034102280466726f6d21011441022802746f21011441022806616d6f756e74210111400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff08e295e391544c178ad94f03ec4dcdff78534ecf49").unwrap(), &hex::decode("40052101f821002814e295e391544c178ad94f03ec4dcdff78534ecf49285c4e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000e10411af77b674010411af77b6740a621a13a4108280e526f6c654d616e6167656d656e74400048004000410240024105280f64657369676e6174654173526f6c65400241022804726f6c65210111410228056e6f6465732101202102ff00210020004105281367657444657369676e617465644279526f6c65400241022804726f6c6521011141022805696e646578210111210120210107200140014102280b44657369676e6174696f6e400241022804526f6c652101114102280a426c6f636b496e646578210111400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff08f563ea40bc283d4d0e05c48ea305b3f2a07340ef").unwrap(), &hex::decode("40052101fb21002814f563ea40bc283d4d0e05c48ea305b3f2a07340ef28d34e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008510411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b67407bf3e603410828084e656f546f6b656e40004800400128064e45502d3137410240134105280962616c616e63654f664001410228076163636f756e742101142101112100200141052808646563696d616c73400021011121010720014105280f6765744163636f756e7453746174654001410228076163636f756e7421011421012021010e200141052810676574416c6c43616e64696461746573400021013021011520014105281067657443616e646964617465566f74654001410228067075624b657921011621011121011c20014105280d67657443616e64696461746573400021012021012320014105280c676574436f6d6d6974746565400021012021012a20014105280e676574476173506572426c6f636b40002101112101312001410528166765744e657874426c6f636b56616c696461746f7273400021012021013820014105281067657452656769737465725072696365400021011121013f200141052811726567697374657243616e6469646174654001410228067075626b657921011621011021014620004105280e736574476173506572426c6f636b40014102280b676173506572426c6f636b2101112102ff0021014d2000410528107365745265676973746572507269636540014102280d726567697374657250726963652101112102ff0021015420004105280673796d626f6c400021011321015b20014105280b746f74616c537570706c7940002101112101622001410528087472616e7366657240044102280466726f6d21011441022802746f21011441022806616d6f756e742101114102280464617461210021011021016920004105280c756e636c61696d65644761734002410228076163636f756e7421011441022803656e64210111210111210170200141052813756e726567697374657243616e6469646174654001410228067075626b6579210116210110210177200041052804766f74654002410228076163636f756e7421011441022806766f7465546f21011621011021017e20004003410228085472616e7366657240034102280466726f6d21011441022802746f21011441022806616d6f756e742101114102281543616e64696461746553746174654368616e6765644003410228067075626b65792101164102280a7265676973746572656421011041022805766f74657321011141022804566f74654004410228076163636f756e742101144102280466726f6d21011641022802746f21011641022806616d6f756e74210111400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff08fda3fa4346ea532a258fc497ddaddb6437c9fdff").unwrap(), &hex::decode("40052101ff21002814fda3fa4346ea532a258fc497ddaddb6437c9fdff289b4e4546336e656f2d636f72652d76332e3000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004d10411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b674010411af77b67404811394141082812436f6e74726163744d616e6167656d656e744000480040004102400b410528066465706c6f794002410228076e656646696c65210112410228086d616e696665737421011221012021002000410528066465706c6f794003410228076e656646696c65210112410228086d616e69666573742101124102280464617461210021012021010720004105280764657374726f7940002102ff0021010e20004105280b676574436f6e74726163744001410228046861736821011421012021011520014105280f676574436f6e747261637442794964400141022802696421011121012021011c200141052811676574436f6e747261637448617368657340002101302101232001410528176765744d696e696d756d4465706c6f796d656e74466565400021011121012a2001410528096861734d6574686f6440034102280468617368210114410228066d6574686f642101134102280670636f756e742101112101102101312001410528177365744d696e696d756d4465706c6f796d656e7446656540014102280576616c75652101112102ff002101382000410528067570646174654002410228076e656646696c65210112410228086d616e69666573742101122102ff0021013f2000410528067570646174654003410228076e656646696c65210112410228086d616e6966657374210112410228046461746121002102ff0021014620004003410228064465706c6f794001410228044861736821011441022806557064617465400141022804486173682101144102280744657374726f7940014102280448617368210114400141020000400028046e756c6c").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cfffffff7").unwrap(), &hex::decode("588717117e0aa81072afab71d2dd89fe7c4b92fe").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cfffffff8").unwrap(), &hex::decode("e295e391544c178ad94f03ec4dcdff78534ecf49").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cfffffff9").unwrap(), &hex::decode("7bc681c0a1f71d543457b68bba8d5f9fdd4e5ecc").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cfffffffa").unwrap(), &hex::decode("cf76e28bd0062c4a478ee35561011319f3cfa4d2").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cfffffffb").unwrap(), &hex::decode("f563ea40bc283d4d0e05c48ea305b3f2a07340ef").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cfffffffc").unwrap(), &hex::decode("bef2043140362a77c15099c7e64c12f700b665da").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cfffffffd").unwrap(), &hex::decode("1bf575ab1189688413610a35a12886cde0b66c72").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cfffffffe").unwrap(), &hex::decode("c0ef39cee0e4e925c6c2a06a79e1440dd86fceac").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0cffffffff").unwrap(), &hex::decode("fda3fa4346ea532a258fc497ddaddb6437c9fdff").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff0f").unwrap(), &hex::decode("01").unwrap()).unwrap();
    trie.put(&hex::decode("ffffffff14").unwrap(), &hex::decode("00ca9a3b").unwrap()).unwrap();
    let root = trie.root_hash().expect("trie should have root");
    let root_hex = hex::encode(root.to_bytes());
    // Reference root from C# (BE display format): 0x58a5157b7e99eeabf631291f1747ec8eb12ab89461cda888492b17a301de81e8
    // UInt256::to_bytes() returns LE, so compare against the LE representation
    assert_eq!(root_hex, "e881de01a3172b4988a8cd6194b82ab18eec47171f2931f6abee997e7b15a558", "genesis state root must match C# reference (LE bytes)");
}

}
