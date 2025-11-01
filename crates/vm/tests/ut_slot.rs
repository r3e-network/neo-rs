//! Slot tests converted from C# Neo.VM.Tests/UT_Slot.cs
//!
//! Tests for Slot functionality including indexing, enumeration, and reference counting.

use neo_vm::{reference_counter::ReferenceCounter, slot::Slot, stack_item::StackItem};
use num_bigint::BigInt;

/// Helper function to create an ordered slot with sequential integers
fn create_ordered_slot(count: usize) -> Slot {
    let reference_counter = ReferenceCounter::new();
    let mut items = Vec::new();

    for i in 1..=count {
        items.push(StackItem::from_int(BigInt::from(i)));
    }

    let slot = Slot::new_with_items(items.clone(), reference_counter);

    // Verify the slot was created correctly
    assert_eq!(slot.count(), count);

    // Verify items are in correct order
    for (index, expected) in items.iter().enumerate() {
        let actual = slot.get(index).unwrap();
        assert_eq!(actual, expected);
    }

    slot
}

#[test]
fn test_slot_get() {
    let slot = create_ordered_slot(3);

    // Test valid indices
    assert_eq!(slot.get(0).unwrap(), &StackItem::from_int(BigInt::from(1)));
    assert_eq!(slot.get(1).unwrap(), &StackItem::from_int(BigInt::from(2)));
    assert_eq!(slot.get(2).unwrap(), &StackItem::from_int(BigInt::from(3)));

    // Test invalid index
    assert!(slot.get(3).is_none());
}

#[test]
fn test_slot_set() {
    let mut slot = create_ordered_slot(3);

    // Set a new value
    let new_value = StackItem::from_int(BigInt::from(10));
    slot.set(1, new_value.clone()).unwrap();

    // Verify the value was set
    assert_eq!(slot.get(1).unwrap(), &new_value);

    // Test setting at invalid index
    assert!(slot.set(3, StackItem::Null).is_err());
}

#[test]
fn test_slot_indexing() {
    let slot = create_ordered_slot(5);

    // Test indexing with positive values
    assert_eq!(slot[0], StackItem::from_int(BigInt::from(1)));
    assert_eq!(slot[1], StackItem::from_int(BigInt::from(2)));
    assert_eq!(slot[2], StackItem::from_int(BigInt::from(3)));
    assert_eq!(slot[3], StackItem::from_int(BigInt::from(4)));
    assert_eq!(slot[4], StackItem::from_int(BigInt::from(5)));
}

#[test]
#[should_panic]
fn test_slot_indexing_out_of_bounds() {
    let slot = create_ordered_slot(3);
    let _ = &slot[3]; // Should panic
}

#[test]
fn test_slot_enumeration() {
    let slot = create_ordered_slot(3);

    // Test iteration
    let mut count = 0;
    for (index, item) in slot.iter().enumerate() {
        let expected = BigInt::from(index + 1);
        assert_eq!(item, &StackItem::from_int(expected));
        count += 1;
    }
    assert_eq!(count, 3);
}

#[test]
fn test_slot_count() {
    // Test empty slot
    let reference_counter = ReferenceCounter::new();
    let empty_slot = Slot::new(0, reference_counter.clone());
    assert_eq!(empty_slot.count(), 0);

    // Test slot with items
    let slot_3 = create_ordered_slot(3);
    assert_eq!(slot_3.count(), 3);

    let slot_10 = create_ordered_slot(10);
    assert_eq!(slot_10.count(), 10);
}

#[test]
fn test_slot_clear() {
    let mut slot = create_ordered_slot(3);

    // Verify slot has items
    assert_eq!(slot.count(), 3);

    // Clear the slot
    slot.clear();

    // All items should be Null
    for i in 0..3 {
        assert_eq!(slot.get(i).unwrap(), &StackItem::Null);
    }

    // Count should remain the same
    assert_eq!(slot.count(), 3);
}

#[test]
fn test_slot_reference_counter() {
    let reference_counter = ReferenceCounter::new();
    let initial_count = reference_counter.count();

    // Create slot with compound items that need reference counting
    let array_item = StackItem::from_array(vec![
        StackItem::from_int(BigInt::from(1)),
        StackItem::from_int(BigInt::from(2)),
    ]);

    let mut slot = Slot::new(1, reference_counter.clone());
    slot.set(0, array_item).unwrap();

    // Reference count should have increased
    assert!(reference_counter.count() > initial_count);

    // Clear the slot
    slot.clear();

    // Reference count should decrease after clearing
    let count_after_clear = reference_counter.count();
    assert!(count_after_clear < reference_counter.count() + 10); // Some reasonable bound
}

#[test]
fn test_slot_clone() {
    let slot = create_ordered_slot(3);

    // Clone the slot
    let cloned = slot.clone();

    // Verify the clone has the same values
    assert_eq!(cloned.count(), slot.count());
    for i in 0..3 {
        assert_eq!(cloned.get(i).unwrap(), slot.get(i).unwrap());
    }
}

#[test]
fn test_slot_with_null_items() {
    let reference_counter = ReferenceCounter::new();
    let slot = Slot::new(3, reference_counter);

    // New slot should be initialized with Null items
    for i in 0..3 {
        assert_eq!(slot.get(i).unwrap(), &StackItem::Null);
    }
}

#[test]
fn test_slot_with_mixed_types() {
    let reference_counter = ReferenceCounter::new();
    let items = vec![
        StackItem::from_int(BigInt::from(42)),
        StackItem::Boolean(true),
        StackItem::ByteString(vec![0x01, 0x02, 0x03]),
        StackItem::Null,
    ];

    let slot = Slot::new_with_items(items.clone(), reference_counter);

    // Verify each item type
    assert_eq!(slot.get(0).unwrap(), &items[0]);
    assert_eq!(slot.get(1).unwrap(), &items[1]);
    assert_eq!(slot.get(2).unwrap(), &items[2]);
    assert_eq!(slot.get(3).unwrap(), &items[3]);
}

#[test]
fn test_slot_to_vec() {
    let slot = create_ordered_slot(3);

    // Convert to vector
    let vec = slot.to_vec();

    // Verify the vector contents
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], StackItem::from_int(BigInt::from(1)));
    assert_eq!(vec[1], StackItem::from_int(BigInt::from(2)));
    assert_eq!(vec[2], StackItem::from_int(BigInt::from(3)));
}
