//! Parity tests for the Neo VM reference counter port.

use neo_vm::reference_counter::{CompoundParent, ReferenceCounter};
use neo_vm::stack_item::{Array, Map, StackItem, Struct};
use std::collections::BTreeMap;

fn new_reference_counter() -> ReferenceCounter {
    ReferenceCounter::new()
}

fn new_array() -> StackItem {
    StackItem::from_array(vec![StackItem::from_int(1), StackItem::Null])
}

fn new_struct() -> StackItem {
    StackItem::from_struct(vec![StackItem::from_int(5)])
}

#[test]
fn stack_references_are_counted() {
    let counter = new_reference_counter();
    let array = new_array();

    counter.add_stack_reference(&array, 1);
    counter.add_stack_reference(&array, 2);
    assert_eq!(counter.count(), 3);

    counter.remove_stack_reference(&array);
    counter.remove_stack_reference(&array);
    counter.remove_stack_reference(&array);
    assert_eq!(counter.count(), 0);

    // Drain zero-referred queue without panicking.
    assert_eq!(counter.check_zero_referred(), 0);
}

#[test]
fn object_references_keep_children_alive() {
    let counter = new_reference_counter();
    let parent = new_array();
    let child = new_struct();

    counter.add_reference(&child, &parent);
    assert_eq!(counter.count(), 1);

    // While the parent still references the child the zero set should be empty.
    assert_eq!(counter.check_zero_referred(), 1);

    counter.remove_reference(&child, &parent);
    assert_eq!(counter.count(), 0);
    assert_eq!(counter.check_zero_referred(), 0);
}

#[test]
fn zero_referred_records_are_cleared() {
    let counter = new_reference_counter();
    let array = new_array();

    counter.add_stack_reference(&array, 1);
    assert_eq!(counter.count(), 1);

    counter.remove_stack_reference(&array);
    assert_eq!(counter.count(), 0);

    // First call removes the outstanding tracked record.
    assert_eq!(counter.check_zero_referred(), 0);
    // Second call is a no-op.
    assert_eq!(counter.check_zero_referred(), 0);
}

#[test]
fn array_compound_references_track_children() {
    let counter = ReferenceCounter::new();
    let child_struct = StackItem::Struct(Struct::new(Vec::new(), Some(counter.clone())));
    let mut array = Array::new(Vec::new(), Some(counter.clone()));

    assert_eq!(counter.count(), 0);

    array.push(child_struct.clone()).unwrap();
    assert_eq!(counter.count(), 1);

    let popped = array.pop().unwrap();
    assert!(popped.equals(&child_struct).unwrap());
    assert_eq!(counter.count(), 0);
    assert_eq!(counter.check_zero_referred(), 0);
}

#[test]
fn map_references_keys_and_values() {
    let counter = ReferenceCounter::new();
    let mut map = Map::new(BTreeMap::new(), Some(counter.clone()));

    let key = StackItem::from_int(7);
    let value = StackItem::Struct(Struct::new(Vec::new(), Some(counter.clone())));

    map.set(key.clone(), value.clone()).unwrap();
    assert_eq!(counter.count(), 2);

    let removed = map.remove(&key).unwrap();
    assert!(removed.equals(&value).unwrap());
    assert_eq!(counter.count(), 0);
    assert_eq!(counter.check_zero_referred(), 0);
}

#[test]
fn cyclic_compound_items_are_collected() {
    let counter = ReferenceCounter::new();

    let array_a = Array::new(Vec::new(), Some(counter.clone()));
    let array_b = Array::new(Vec::new(), Some(counter.clone()));

    let id_a = array_a.id();
    let id_b = array_b.id();

    let item_a = StackItem::Array(array_a);
    let item_b = StackItem::Array(array_b);

    counter.add_stack_reference(&item_a, 1);
    counter.add_stack_reference(&item_b, 1);

    counter.add_compound_reference(&item_a, CompoundParent::Array(id_b));
    counter.add_compound_reference(&item_b, CompoundParent::Array(id_a));

    counter.remove_stack_reference(&item_a);
    counter.remove_stack_reference(&item_b);

    assert_eq!(counter.check_zero_referred(), 0);
    assert_eq!(counter.check_zero_referred(), 0);
}
