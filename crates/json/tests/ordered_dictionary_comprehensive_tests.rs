//! Comprehensive OrderedDictionary Tests
//! Tests for OrderedDictionary functionality matching C# Neo.Json.UnitTests.UT_OrderedDictionary

use neo_json::OrderedDictionary;

// Helper to create test dictionary like C# SetUp method
fn create_test_dict() -> OrderedDictionary<String, u32> {
    let mut od = OrderedDictionary::new();
    od.insert("a".to_string(), 1);
    od.insert("b".to_string(), 2);
    od.insert("c".to_string(), 3);
    od
}

/// Test TestClear functionality (matches C# UT_OrderedDictionary.TestClear)
#[test]
fn test_clear() {
    let mut od = create_test_dict();

    // Verify initial state
    assert_eq!(od.len(), 3);
    assert!(od.contains_key(&"a".to_string()));

    // Clear the dictionary
    od.clear();

    // Verify cleared state
    assert!(od.is_empty());
    assert_eq!(od.len(), 0);
    assert!(!od.contains_key(&"a".to_string()));
    assert_eq!(od.get(&"a".to_string()), None);
}

/// Test TestCount functionality (matches C# UT_OrderedDictionary.TestCount)
#[test]
fn test_count() {
    let mut od = create_test_dict();

    // Verify initial count
    assert_eq!(od.len(), 3);

    // Add an item and verify count increases
    od.insert("d".to_string(), 4);
    assert_eq!(od.len(), 4);

    // Add another item
    od.insert("e".to_string(), 5);
    assert_eq!(od.len(), 5);

    // Remove an item and verify count decreases
    od.remove(&"a".to_string());
    assert_eq!(od.len(), 4);
}

/// Test TestIsReadOnly functionality (matches C# UT_OrderedDictionary.TestIsReadOnly)
#[test]
fn test_is_read_only() {
    let od = create_test_dict();

    // OrderedDictionary should not be read-only (this is more of a concept in C#)
    // In Rust, we can test mutability by trying to modify it
    let mut mutable_od = od.clone();

    // Should be able to modify
    mutable_od.insert("new_key".to_string(), 999);
    assert_eq!(mutable_od.len(), 4);
    assert_eq!(mutable_od.get(&"new_key".to_string()), Some(&999));
}

/// Test TestSetAndGetItem functionality (matches C# UT_OrderedDictionary.TestSetAndGetItem)
#[test]
fn test_set_and_get_item() {
    let mut od = create_test_dict();

    // Test getting existing item
    let val = od.get(&"a".to_string());
    assert_eq!(val, Some(&1));

    // Test setting new item
    od.insert("d".to_string(), 10);
    assert_eq!(od.get(&"d".to_string()), Some(&10));

    // Test updating existing item
    od.insert("d".to_string(), 15);
    assert_eq!(od.get(&"d".to_string()), Some(&15));

    // Test that the length doesn't change when updating existing key
    assert_eq!(od.len(), 4); // a, b, c, d

    // Test getting non-existent item
    assert_eq!(od.get(&"z".to_string()), None);
}

/// Test TestGetKeys functionality (matches C# UT_OrderedDictionary.TestGetKeys)
#[test]
fn test_get_keys() {
    let od = create_test_dict();

    // Get keys and verify they contain expected values
    let keys: Vec<&String> = od.keys().collect();
    assert_eq!(keys.len(), 3);

    // Verify keys are in insertion order
    assert_eq!(keys[0], &"a".to_string());
    assert_eq!(keys[1], &"b".to_string());
    assert_eq!(keys[2], &"c".to_string());

    // Test that keys contains expected values
    let keys_vec: Vec<String> = od.keys().cloned().collect();
    assert!(keys_vec.contains(&"a".to_string()));
    assert!(keys_vec.contains(&"b".to_string()));
    assert!(keys_vec.contains(&"c".to_string()));
    assert!(!keys_vec.contains(&"z".to_string()));
}

/// Test TestGetValues functionality (matches C# UT_OrderedDictionary.TestGetValues)
#[test]
fn test_get_values() {
    let od = create_test_dict();

    // Get values and verify they contain expected values
    let values: Vec<&u32> = od.values().collect();
    assert_eq!(values.len(), 3);

    // Verify values are in insertion order
    assert_eq!(values[0], &1);
    assert_eq!(values[1], &2);
    assert_eq!(values[2], &3);

    // Test that values contains expected values
    let values_vec: Vec<u32> = od.values().cloned().collect();
    assert!(values_vec.contains(&1));
    assert!(values_vec.contains(&2));
    assert!(values_vec.contains(&3));
    assert!(!values_vec.contains(&99));
}

/// Test TestRemove functionality (matches C# UT_OrderedDictionary.TestRemove)
#[test]
fn test_remove() {
    let mut od = create_test_dict();

    // Verify initial state
    assert_eq!(od.len(), 3);
    assert!(od.contains_key(&"a".to_string()));

    // Remove an item
    let removed_value = od.remove(&"a".to_string());
    assert_eq!(removed_value, Some(1));

    // Verify item was removed
    assert_eq!(od.len(), 2);
    assert!(!od.contains_key(&"a".to_string()));
    assert_eq!(od.get(&"a".to_string()), None);

    // Verify remaining items are still there and in order
    let keys: Vec<&String> = od.keys().collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], &"b".to_string());
    assert_eq!(keys[1], &"c".to_string());

    // Test removing non-existent item
    let removed_none = od.remove(&"z".to_string());
    assert_eq!(removed_none, None);
    assert_eq!(od.len(), 2); // Length should remain unchanged
}

/// Test TestTryGetValue functionality (matches C# UT_OrderedDictionary.TestTryGetValue)
#[test]
fn test_try_get_value() {
    let od = create_test_dict();

    // Test successful get
    let value_a = od.get(&"a".to_string());
    assert_eq!(value_a, Some(&1));

    // Test unsuccessful get
    let value_d = od.get(&"d".to_string());
    assert_eq!(value_d, None);

    // Test with different approach mimicking TryGetValue
    match od.get(&"a".to_string()) {
        Some(value) => {
            assert_eq!(*value, 1);
        }
        None => panic!("Should have found value for key 'a'"),
    }

    match od.get(&"nonexistent".to_string()) {
        Some(_) => panic!("Should not have found value for non-existent key"),
        None => {
            // This is expected
        }
    }
}

/// Test TestCollectionAddAndContains functionality (matches C# UT_OrderedDictionary.TestCollectionAddAndContains)
#[test]
fn test_collection_add_and_contains() {
    let mut od = create_test_dict();

    // Add a new key-value pair
    od.insert("d".to_string(), 4);

    // Test that it was added
    assert!(od.contains_key(&"d".to_string()));
    assert_eq!(od.get(&"d".to_string()), Some(&4));
    assert_eq!(od.len(), 4);

    // Test contains for existing keys
    assert!(od.contains_key(&"a".to_string()));
    assert!(od.contains_key(&"b".to_string()));
    assert!(od.contains_key(&"c".to_string()));

    // Test contains for non-existent key
    assert!(!od.contains_key(&"z".to_string()));
}

/// Test TestCollectionCopyTo functionality (matches C# UT_OrderedDictionary.TestCollectionCopyTo)
#[test]
fn test_collection_copy_to() {
    let od = create_test_dict();

    // Create a vector to copy into (equivalent to C# array)
    let pairs: Vec<(&String, &u32)> = od.iter().collect();

    // Verify the copied data maintains insertion order
    assert_eq!(pairs.len(), 3);
    assert_eq!(pairs[0], (&"a".to_string(), &1));
    assert_eq!(pairs[1], (&"b".to_string(), &2));
    assert_eq!(pairs[2], (&"c".to_string(), &3));

    // Test copying keys and values separately
    let keys: Vec<String> = od.keys().cloned().collect();
    let values: Vec<u32> = od.values().cloned().collect();

    assert_eq!(
        keys,
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
    assert_eq!(values, vec![1, 2, 3]);
}

/// Test TestCollectionRemove functionality (matches C# UT_OrderedDictionary.TestCollectionRemove)
#[test]
fn test_collection_remove() {
    let mut od = create_test_dict();

    // Remove a key-value pair
    let removed = od.remove(&"a".to_string());
    assert_eq!(removed, Some(1));

    // Verify it was removed
    assert!(!od.contains_key(&"a".to_string()));
    assert_eq!(od.len(), 2);

    // Verify other items are still there
    assert!(od.contains_key(&"b".to_string()));
    assert!(od.contains_key(&"c".to_string()));

    // Test that order is maintained after removal
    let keys: Vec<&String> = od.keys().collect();
    assert_eq!(keys, vec![&"b".to_string(), &"c".to_string()]);
}

/// Test TestGetEnumerator functionality (matches C# UT_OrderedDictionary.TestGetEnumerator)
#[test]
fn test_get_enumerator() {
    let od = create_test_dict();

    // Test that iterator works
    let mut iter = od.iter();

    // Should be able to get first item
    let first = iter.next();
    assert!(first.is_some());
    let (key, value) = first.unwrap();
    assert_eq!(key, &"a".to_string());
    assert_eq!(value, &1);

    // Should be able to get second item
    let second = iter.next();
    assert!(second.is_some());
    let (key, value) = second.unwrap();
    assert_eq!(key, &"b".to_string());
    assert_eq!(value, &2);

    // Should be able to get third item
    let third = iter.next();
    assert!(third.is_some());
    let (key, value) = third.unwrap();
    assert_eq!(key, &"c".to_string());
    assert_eq!(value, &3);

    // Should be None after all items
    let fourth = iter.next();
    assert!(fourth.is_none());

    // Test that we can iterate multiple times
    let count = od.iter().count();
    assert_eq!(count, 3);

    // Test collecting all items
    let all_items: Vec<(&String, &u32)> = od.iter().collect();
    assert_eq!(all_items.len(), 3);
    assert_eq!(all_items[0], (&"a".to_string(), &1));
    assert_eq!(all_items[1], (&"b".to_string(), &2));
    assert_eq!(all_items[2], (&"c".to_string(), &3));
}
