use super::*;
use crate::stack_item::StackItem;

fn arr(items: Vec<StackItem>) -> StackItem {
    StackItem::from_array(items)
}

#[test]
fn primitive_stack_reference_has_no_recursion() {
    let rc = ReferenceCounter::new();
    let item = StackItem::from_int(7);

    rc.add_stack_reference(&item, 1);
    assert_eq!(rc.count(), 1);
    assert!(!rc.is_stack_referenced(&item));

    rc.remove_stack_reference(&item);
    assert_eq!(rc.count(), 0);
}

#[test]
fn empty_compound_counts_one() {
    let rc = ReferenceCounter::new();
    let array = arr(vec![]);

    rc.add_stack_reference(&array, 1);
    assert_eq!(rc.count(), 1);
    assert!(rc.is_stack_referenced(&array));

    rc.remove_stack_reference(&array);
    assert_eq!(rc.count(), 0);
    assert!(!rc.is_stack_referenced(&array));
}

#[test]
fn compound_push_counts_subitems_recursively() {
    // C# v3.10.0: a compound's children are counted when it first becomes
    // stack-referenced: [Int, Null] => 1 (array) + 2 (children) = 3.
    let rc = ReferenceCounter::new();
    let array = arr(vec![StackItem::from_int(1), StackItem::Null]);

    rc.add_stack_reference(&array, 1);
    assert_eq!(rc.count(), 3);

    rc.remove_stack_reference(&array);
    assert_eq!(rc.count(), 0);
}

#[test]
fn nested_compound_recurses_through_levels() {
    // outer -> inner -> [Int]; pushing outer counts outer, inner, and the int.
    let rc = ReferenceCounter::new();
    let inner = arr(vec![StackItem::from_int(9)]);
    let outer = arr(vec![inner]);

    rc.add_stack_reference(&outer, 1);
    assert_eq!(rc.count(), 3);

    rc.remove_stack_reference(&outer);
    assert_eq!(rc.count(), 0);
}

#[test]
fn extra_stack_reference_does_not_re_recurse() {
    // A second stack reference to the same compound raises the total by one
    // only (no re-recursion into subitems), matching C#'s `== count` guard;
    // the final removal de-recurses exactly once.
    let rc = ReferenceCounter::new();
    let array = arr(vec![StackItem::from_int(1), StackItem::Null]);

    rc.add_stack_reference(&array, 1); // 1 + 2 children = 3
    assert_eq!(rc.count(), 3);
    rc.add_stack_reference(&array, 1); // +1 (array only) = 4
    assert_eq!(rc.count(), 4);

    rc.remove_stack_reference(&array); // -1, StackReferences 2->1, no de-recurse
    assert_eq!(rc.count(), 3);
    rc.remove_stack_reference(&array); // -1 array + de-recurse 2 children = 0
    assert_eq!(rc.count(), 0);
}

#[test]
fn shared_subitem_counted_per_parent() {
    // `shared` (with one child) belongs to both `a` and `c` (same Arc/id).
    // Pushing both counts `shared` once per parent, but its child only once
    // (recursion only on `shared`'s first stack reference).
    let rc = ReferenceCounter::new();
    let shared = arr(vec![StackItem::from_int(1)]);
    let a = arr(vec![shared.clone()]);
    let c = arr(vec![shared]);

    rc.add_stack_reference(&a, 1); // a + shared + shared.child = 3
    assert_eq!(rc.count(), 3);
    rc.add_stack_reference(&c, 1); // c + shared(2nd ref, no recurse) = +2 => 5
    assert_eq!(rc.count(), 5);
}

#[test]
fn clear_resets_state() {
    let rc = ReferenceCounter::new();
    let array = arr(vec![StackItem::from_int(1), StackItem::Null]);
    rc.add_stack_reference(&array, 1);
    assert_eq!(rc.count(), 3);

    rc.clear();
    assert_eq!(rc.count(), 0);
    assert!(!rc.is_stack_referenced(&array));
}
