//! Struct tests converted from C# Neo.VM.Tests/UT_Struct.cs
//!
//! Tests for Struct stack item functionality including cloning, equality, and hashing.

use neo_vm::{execution_engine_limits::ExecutionEngineLimits, stack_item::StackItem};
use num_bigint::BigInt;
use std::collections::HashSet;

/// Helper to create a deeply nested struct for testing limits
fn create_deep_struct(depth: usize) -> StackItem {
    let mut result = StackItem::from_int(BigInt::from(1));
    for _ in 0..depth {
        result = StackItem::from_struct(vec![result]);
    }
    result
}

#[test]
fn test_struct_clone() {
    // Create a struct with nested content
    let s1 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(1)),
        StackItem::from_struct(vec![StackItem::from_int(BigInt::from(2))]),
    ]);

    // Clone the struct
    let limits = ExecutionEngineLimits::default();
    let s2 = s1.deep_copy(&limits).unwrap();

    // Modify original struct
    if let StackItem::Struct(ref items) = s1.clone() {
        items.set(0, StackItem::from_int(BigInt::from(3))).unwrap();
    }

    // Verify clone is independent
    if let StackItem::Struct(ref items) = s2 {
        let struct_items = items.items();
        assert_eq!(struct_items[0], StackItem::from_int(BigInt::from(1)));
        if let StackItem::Struct(ref nested) = struct_items[1] {
            let nested_items = nested.items();
            assert_eq!(nested_items[0], StackItem::from_int(BigInt::from(2)));
        }
    }
}

#[test]
fn test_struct_clone_depth_limit() {
    // Create a very deep struct (exceeding typical limits)
    let deep_struct = create_deep_struct(2048);

    // Attempt to clone with default limits should fail
    let limits = ExecutionEngineLimits::default();
    let result = deep_struct.deep_copy(&limits);

    // This should fail due to depth limits
    assert!(result.is_err());
}

#[test]
fn test_struct_equals() {
    // Create identical structs
    let s1 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(1)),
        StackItem::ByteString(vec![0x01, 0x02]),
    ]);

    let s2 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(1)),
        StackItem::ByteString(vec![0x01, 0x02]),
    ]);

    // They should be equal
    assert_eq!(s1, s2);

    // Create a different struct
    let s3 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(2)),
        StackItem::ByteString(vec![0x01, 0x02]),
    ]);

    // Should not be equal
    assert_ne!(s1, s3);
}

#[test]
fn test_struct_equals_with_nesting() {
    // Test equality with nested structs
    let s1 = StackItem::from_struct(vec![StackItem::from_struct(vec![StackItem::from_int(
        BigInt::from(1),
    )])]);

    let s2 = StackItem::from_struct(vec![StackItem::from_struct(vec![StackItem::from_int(
        BigInt::from(1),
    )])]);

    assert_eq!(s1, s2);

    // Different nested value
    let s3 = StackItem::from_struct(vec![StackItem::from_struct(vec![StackItem::from_int(
        BigInt::from(2),
    )])]);

    assert_ne!(s1, s3);
}

#[test]
fn test_struct_hash_code() {
    // Create structs
    let s1 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(1)),
        StackItem::Boolean(true),
    ]);

    let s2 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(1)),
        StackItem::Boolean(true),
    ]);

    let s3 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(2)),
        StackItem::Boolean(false),
    ]);

    // Equal structs should have same hash
    assert_eq!(s1.get_hash_code(), s2.get_hash_code());

    // Different structs should (likely) have different hashes
    assert_ne!(s1.get_hash_code(), s3.get_hash_code());
}

#[test]
fn test_struct_in_hashset() {
    let mut set = HashSet::new();

    // Add a struct to the set
    let s1 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(1)),
        StackItem::ByteString(vec![0x01]),
    ]);

    set.insert(s1.get_hash_code());

    // Same struct should be found
    let s2 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(1)),
        StackItem::ByteString(vec![0x01]),
    ]);

    assert!(set.contains(&s2.get_hash_code()));

    // Different struct should not be found
    let s3 = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(2)),
        StackItem::ByteString(vec![0x02]),
    ]);

    assert!(!set.contains(&s3.get_hash_code()));
}

#[test]
fn test_struct_reference_equals() {
    // Test that structs are reference types
    let s1 = StackItem::from_struct(vec![StackItem::from_int(BigInt::from(1))]);
    let s2 = s1.clone();

    // Cloned structs should be equal by value
    assert_eq!(s1, s2);

    // But modifying one shouldn't affect the other in a deep copy
    let limits = ExecutionEngineLimits::default();
    let s3 = s1.deep_copy(&limits).unwrap();

    if let StackItem::Struct(ref items) = s1.clone() {
        items.set(0, StackItem::from_int(BigInt::from(2))).unwrap();
    }

    // s3 should still have original value
    if let StackItem::Struct(ref items) = s3 {
        let struct_items = items.items();
        assert_eq!(struct_items[0], StackItem::from_int(BigInt::from(1)));
    }
}

#[test]
fn test_struct_empty() {
    // Test empty struct
    let empty = StackItem::from_struct(vec![]);

    if let StackItem::Struct(ref items) = empty {
        assert_eq!(items.len(), 0);
    }

    // Empty structs should be equal
    let empty2 = StackItem::from_struct(vec![]);
    assert_eq!(empty, empty2);
}

#[test]
fn test_struct_with_various_types() {
    // Test struct containing various types
    let s = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(42)),
        StackItem::Boolean(true),
        StackItem::ByteString(vec![0x01, 0x02, 0x03]),
        StackItem::Null,
        StackItem::from_array(vec![StackItem::from_int(BigInt::from(1))]),
    ]);

    if let StackItem::Struct(ref items) = s {
        let struct_items = items.items();
        assert_eq!(struct_items.len(), 5);
        assert_eq!(struct_items[0], StackItem::from_int(BigInt::from(42)));
        assert_eq!(struct_items[1], StackItem::Boolean(true));
        assert_eq!(struct_items[2], StackItem::ByteString(vec![0x01, 0x02, 0x03]));
        assert_eq!(struct_items[3], StackItem::Null);

        if let StackItem::Array(ref arr) = struct_items[4] {
            let arr_items = arr.items();
            assert_eq!(arr_items[0], StackItem::from_int(BigInt::from(1)));
        }
    }
}

#[test]
fn test_struct_circular_reference() {
    // Note: In the C# version, circular references are handled by reference counting
    // In Rust, we need to be careful about creating circular references

    // Create a struct that could potentially have circular reference
    let s1 = StackItem::from_struct(vec![StackItem::from_int(BigInt::from(1))]);

    // In a real scenario, we'd need to handle circular references carefully
    // This is typically done through reference counting or weak references

    // For now, we just test that we can create nested structures
    let s2 = StackItem::from_struct(vec![s1.clone()]);
    let s3 = StackItem::from_struct(vec![s2]);

    if let StackItem::Struct(ref items) = s3 {
        let struct_items = items.items();
        if let StackItem::Struct(ref nested) = struct_items[0] {
            let nested_items = nested.items();
            if let StackItem::Struct(ref deeply_nested) = nested_items[0] {
                let deep_items = deeply_nested.items();
                assert_eq!(deep_items[0], StackItem::from_int(BigInt::from(1)));
            }
        }
    }
}
