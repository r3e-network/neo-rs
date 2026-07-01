use super::*;

// ============================================================================
// UT_Trie.cs Tests (40+ tests)
// ============================================================================

fn trie_root_after(entries: &[(&[u8], &[u8])]) -> UInt256 {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new(store, None, false);
    for (key, value) in entries {
        trie.put(key, value).unwrap();
    }
    trie.root_hash().expect("non-empty trie has a root hash")
}

fn assert_trie_contains(trie: &mut Trie<MockStore>, entries: &[(&[u8], &[u8])]) {
    for (key, value) in entries {
        assert_eq!(
            trie.get(key).unwrap(),
            Some(value.to_vec()),
            "key {} should resolve to expected value",
            hex::encode(key)
        );
    }
}

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
fn test_mutations_keep_root_hash_cached() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new(store, None, false);

    trie.put(b"key1", b"value1").unwrap();
    trie.put(b"key2", b"value2").unwrap();
    assert!(
        trie.root().hash_is_cached(),
        "put should cache the resulting root hash"
    );

    trie.delete(b"key1").unwrap();
    assert!(
        trie.root().hash_is_cached(),
        "delete should cache the resulting root hash"
    );
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
fn insertion_order_does_not_change_extension_branch_root() {
    let entries: &[(&[u8], &[u8])] = &[
        (&[0x12], b"prefix"),
        (&[0x12, 0x30], b"left"),
        (&[0x12, 0x34], b"middle"),
        (&[0x12, 0x35], b"right"),
        (&[0x12, 0x35, 0x01], b"deeper"),
    ];
    let expected_root = trie_root_after(entries);

    let permutations: &[&[(&[u8], &[u8])]] = &[
        &[
            (&[0x12, 0x35, 0x01], b"deeper"),
            (&[0x12, 0x35], b"right"),
            (&[0x12, 0x34], b"middle"),
            (&[0x12, 0x30], b"left"),
            (&[0x12], b"prefix"),
        ],
        &[
            (&[0x12, 0x34], b"middle"),
            (&[0x12], b"prefix"),
            (&[0x12, 0x35, 0x01], b"deeper"),
            (&[0x12, 0x30], b"left"),
            (&[0x12, 0x35], b"right"),
        ],
        &[
            (&[0x12, 0x30], b"left"),
            (&[0x12, 0x35], b"right"),
            (&[0x12], b"prefix"),
            (&[0x12, 0x34], b"middle"),
            (&[0x12, 0x35, 0x01], b"deeper"),
        ],
    ];

    for permutation in permutations {
        assert_eq!(
            trie_root_after(permutation),
            expected_root,
            "same final key/value set must have one canonical root"
        );
    }
}

#[test]
fn delete_compression_matches_fresh_remaining_trie() {
    let store = Arc::new(MockStore::new());
    let mut trie = Trie::new(store, None, false);
    let starting_entries: &[(&[u8], &[u8])] = &[
        (&[0x12], b"prefix"),
        (&[0x12, 0x30], b"left"),
        (&[0x12, 0x34], b"middle"),
        (&[0x12, 0x35], b"right"),
    ];
    for (key, value) in starting_entries {
        trie.put(key, value).unwrap();
    }

    assert!(trie.delete(&[0x12]).unwrap());
    assert!(trie.delete(&[0x12, 0x30]).unwrap());
    assert!(trie.delete(&[0x12, 0x34]).unwrap());

    let remaining_entries: &[(&[u8], &[u8])] = &[(&[0x12, 0x35], b"right")];
    assert_trie_contains(&mut trie, remaining_entries);
    assert_eq!(
        trie.root_hash(),
        Some(trie_root_after(remaining_entries)),
        "branch-to-single-child compression must match a fresh trie"
    );
}

#[test]
fn reopened_history_then_followup_delete_matches_fresh_trie() {
    let store = Arc::new(MockStore::new());
    let mut historical = Trie::new(store.clone(), None, false);
    let initial_entries: &[(&[u8], &[u8])] = &[
        (&[0x12], b"prefix"),
        (&[0x12, 0x30], b"left"),
        (&[0x12, 0x34], b"middle"),
        (&[0x12, 0x35], b"right"),
        (&[0x12, 0x35, 0x01], b"deeper"),
    ];
    for (key, value) in initial_entries {
        historical.put(key, value).unwrap();
    }
    historical.commit().unwrap();

    let root = historical.root_hash();
    let mut reopened = Trie::new(store, root, false);
    assert!(reopened.delete(&[0x12, 0x34]).unwrap());
    reopened.put(&[0x12, 0x35], b"right-updated").unwrap();

    let remaining_entries: &[(&[u8], &[u8])] = &[
        (&[0x12], b"prefix"),
        (&[0x12, 0x30], b"left"),
        (&[0x12, 0x35], b"right-updated"),
        (&[0x12, 0x35, 0x01], b"deeper"),
    ];
    assert_trie_contains(&mut reopened, remaining_entries);
    assert_eq!(
        reopened.root_hash(),
        Some(trie_root_after(remaining_entries)),
        "commit/reopen refcounts must not make later mutations history-dependent"
    );
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
