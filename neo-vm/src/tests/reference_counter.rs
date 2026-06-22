use super::*;
use crate::stack_item::StackItem;

fn make_array() -> StackItem {
    StackItem::from_array(vec![StackItem::from_int(1), StackItem::Null])
}

fn make_struct() -> StackItem {
    StackItem::from_struct(vec![StackItem::from_int(42)])
}

#[test]
fn stack_references_increment_and_decrement() {
    let counter = ReferenceCounter::new();
    let item = make_array();

    counter.add_stack_reference(&item, 1);
    assert_eq!(counter.count(), 1);

    counter.remove_stack_reference(&item);
    assert_eq!(counter.count(), 0);
    assert_eq!(counter.check_zero_referred(), 0);
}

#[test]
fn object_references_affect_zero_tracking() {
    let counter = ReferenceCounter::new();
    let child = make_struct();
    let parent = make_array();

    counter.add_reference(&child, &parent);
    assert_eq!(counter.count(), 1);

    counter.remove_reference(&child, &parent);
    assert_eq!(counter.count(), 0);
    assert_eq!(counter.check_zero_referred(), 0);
}

#[test]
fn zero_referred_removes_item_records() {
    let counter = ReferenceCounter::new();
    let item = make_array();

    counter.add_stack_reference(&item, 2);
    assert_eq!(counter.count(), 2);

    counter.remove_stack_reference(&item);
    counter.remove_stack_reference(&item);
    assert_eq!(counter.count(), 0);

    assert_eq!(counter.check_zero_referred(), 0);
    assert_eq!(counter.check_zero_referred(), 0);
}
