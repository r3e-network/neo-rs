//! Reference counting implementation for the Neo VM.
//!
//! This module mirrors `Neo.VM.ReferenceCounter` at neo-vm v3.10.1. The
//! counter only holds the total number of stack references; every compound
//! item carries its own `StackReferences` value, exactly like
//! `CompoundType.StackReferences` in the C# code base. There are no
//! parent/child edges, no zero-referred candidates and no collection pass:
//!
//! * `AddStackReference(item, count)` adds `count` to the total. A compound
//!   item also adds `count` to its own `StackReferences`, and when that makes
//!   it referenced for the first time, every sub item receives one reference.
//! * `RemoveStackReference(item)` subtracts one from the total, except for a
//!   compound item that is not stack referenced, which is left untouched. A
//!   compound item whose `StackReferences` reaches zero releases one reference
//!   of every sub item.
//! * `PostExecuteInstruction` compares the total with `MaxStackSize`.
//!
//! A value that can reach itself is therefore never released by the counter.
//!
//! The C# methods recurse into the sub items. Here the same walk runs on an
//! explicit work stack, because a single instruction can release and activate
//! a chain that is about two thousand compound items deep.

use crate::error::{VmError, VmResult};
use crate::stack_item::StackItem;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};

/// The sub items of a compound item whose stack-reference state just changed.
///
/// Only compound sub items can make the walk go deeper, so they are kept in
/// `SubItems` order while the other sub items are only counted. The snapshot is
/// taken under the lock of the owning item and used after that lock is
/// released, so a container that holds itself is never locked twice.
#[derive(Debug, Default)]
pub(crate) struct SubItems {
    /// Number of sub items that are not compound items.
    pub(crate) primitives: usize,
    /// The compound sub items, in `SubItems` order.
    pub(crate) compounds: Vec<StackItem>,
}

impl SubItems {
    /// Takes a snapshot of the given sub items.
    pub(crate) fn collect<'a>(items: impl IntoIterator<Item = &'a StackItem>) -> Self {
        let mut sub_items = Self::default();
        for item in items {
            match item {
                StackItem::Array(_) | StackItem::Struct(_) | StackItem::Map(_) => {
                    sub_items.compounds.push(item.clone());
                }
                _ => sub_items.primitives += 1,
            }
        }
        sub_items
    }
}

/// Outcome of removing one stack reference from a compound item.
#[derive(Debug)]
pub(crate) enum StackReferenceRelease {
    /// The item was not stack referenced; nothing changed.
    NotReferenced,
    /// One reference was removed and the item is still stack referenced.
    StillReferenced,
    /// The last reference was removed; the sub items must be released.
    Unreferenced(SubItems),
}

/// Adds `count` to the `StackReferences` of a compound item. Returns the sub
/// items when the item became stack referenced by this call.
fn add_compound_references(item: &StackItem, count: usize) -> Option<SubItems> {
    match item {
        StackItem::Array(array) => array.add_stack_references(count),
        StackItem::Struct(structure) => structure.add_stack_references(count),
        StackItem::Map(map) => map.add_stack_references(count),
        _ => None,
    }
}

/// Removes one from the `StackReferences` of a compound item. Returns `None`
/// for an item that is not a compound item.
fn remove_compound_reference(item: &StackItem) -> Option<StackReferenceRelease> {
    match item {
        StackItem::Array(array) => Some(array.remove_stack_reference()),
        StackItem::Struct(structure) => Some(structure.remove_stack_reference()),
        StackItem::Map(map) => Some(map.remove_stack_reference()),
        _ => None,
    }
}

/// Tracks references to VM stack items.
#[derive(Clone, Debug)]
pub struct ReferenceCounter {
    /// Total number of stack references. Signed like the C# `int`, so that an
    /// unbalanced release shows up as a negative value instead of wrapping.
    references_count: Arc<AtomicIsize>,
}

impl ReferenceCounter {
    /// Creates a new reference counter.
    pub fn new() -> Self {
        Self {
            references_count: Arc::new(AtomicIsize::new(0)),
        }
    }

    /// Returns the total number of references currently tracked.
    #[inline]
    pub fn count(&self) -> usize {
        usize::try_from(self.signed_count()).unwrap_or(0)
    }

    /// Returns the total as the signed value the limit check compares. A
    /// negative value means more references were released than added.
    #[inline]
    pub fn signed_count(&self) -> isize {
        self.references_count.load(Ordering::Relaxed)
    }

    #[inline]
    fn increase(&self, count: usize) {
        self.references_count
            .fetch_add(count as isize, Ordering::Relaxed);
    }

    #[inline]
    fn decrease(&self, count: usize) {
        let previous = self
            .references_count
            .fetch_sub(count as isize, Ordering::Relaxed);
        debug_assert!(
            previous >= count as isize,
            "reference count underflow: {previous} - {count}"
        );
    }

    /// Adds `count` stack references for the supplied item.
    ///
    /// neo-vm v3.10.1 `ReferenceCounter.AddStackReference`.
    #[inline]
    pub fn add_stack_reference(&self, item: &StackItem, count: usize) {
        self.increase(count);
        if let Some(sub_items) = add_compound_references(item, count) {
            self.add_sub_items(sub_items);
        }
    }

    /// Gives one reference to every sub item of a compound item that just
    /// became stack referenced, going deeper wherever a sub item becomes
    /// stack referenced in turn.
    fn add_sub_items(&self, sub_items: SubItems) {
        self.increase(sub_items.primitives);
        let mut pending = vec![sub_items.compounds.into_iter()];
        while let Some(siblings) = pending.last_mut() {
            let Some(item) = siblings.next() else {
                pending.pop();
                continue;
            };
            self.increase(1);
            if let Some(sub_items) = add_compound_references(&item, 1) {
                self.increase(sub_items.primitives);
                pending.push(sub_items.compounds.into_iter());
            }
        }
    }

    /// Removes a single stack reference for the supplied item.
    ///
    /// neo-vm v3.10.1 `ReferenceCounter.RemoveStackReference`.
    #[inline]
    pub fn remove_stack_reference(&self, item: &StackItem) {
        match remove_compound_reference(item) {
            None | Some(StackReferenceRelease::StillReferenced) => self.decrease(1),
            Some(StackReferenceRelease::NotReferenced) => {}
            Some(StackReferenceRelease::Unreferenced(sub_items)) => {
                self.decrease(1);
                self.remove_sub_items(sub_items);
            }
        }
    }

    /// Releases one reference of every sub item of a compound item that just
    /// stopped being stack referenced, going deeper wherever a sub item stops
    /// being stack referenced in turn. CLEARITEMS releases the sub items it
    /// removed the same way.
    pub(crate) fn remove_sub_items(&self, sub_items: SubItems) {
        self.decrease(sub_items.primitives);
        let mut pending = vec![sub_items.compounds.into_iter()];
        while let Some(siblings) = pending.last_mut() {
            let Some(item) = siblings.next() else {
                pending.pop();
                continue;
            };
            match remove_compound_reference(&item) {
                None | Some(StackReferenceRelease::StillReferenced) => self.decrease(1),
                Some(StackReferenceRelease::NotReferenced) => {}
                Some(StackReferenceRelease::Unreferenced(sub_items)) => {
                    self.decrease(1);
                    self.decrease(sub_items.primitives);
                    pending.push(sub_items.compounds.into_iter());
                }
            }
        }
    }

    /// Fails when the total exceeds `max_stack_size`.
    ///
    /// neo-vm v3.10.1 `ReferenceCounter.PostExecuteInstruction`.
    #[inline]
    pub fn post_execute_instruction(&self, max_stack_size: u32) -> VmResult<()> {
        let count = self.signed_count();
        if count > max_stack_size as isize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxStackSize exceed: {count}/{max_stack_size}"
            )));
        }
        Ok(())
    }

    /// Returns true when both handles refer to the same counter.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.references_count, &other.references_count)
    }
}

impl Default for ReferenceCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stack_item::{Array, Map, Struct};

    fn array(items: Vec<StackItem>) -> (Array, StackItem) {
        let array = Array::new(items);
        (array.clone(), StackItem::Array(array))
    }

    #[test]
    fn primitives_count_once_per_reference() {
        let rc = ReferenceCounter::new();
        let item = StackItem::from_int(1);
        rc.add_stack_reference(&item, 1);
        rc.add_stack_reference(&StackItem::Null, 3);
        assert_eq!(rc.count(), 4);
        rc.remove_stack_reference(&item);
        assert_eq!(rc.count(), 3);
    }

    #[test]
    fn first_reference_adds_sub_items_and_last_reference_releases_them() {
        let rc = ReferenceCounter::new();
        let (inner, inner_item) = array(vec![StackItem::from_int(1), StackItem::from_int(2)]);
        let (outer, outer_item) = array(vec![inner_item, StackItem::Null]);

        rc.add_stack_reference(&outer_item, 1);
        // outer + inner + two integers + Null
        assert_eq!(rc.count(), 5);
        assert_eq!(outer.stack_references(), 1);
        assert_eq!(inner.stack_references(), 1);

        rc.add_stack_reference(&outer_item, 1);
        assert_eq!(rc.count(), 6);
        assert_eq!(outer.stack_references(), 2);
        assert_eq!(inner.stack_references(), 1);

        rc.remove_stack_reference(&outer_item);
        assert_eq!(rc.count(), 5);
        rc.remove_stack_reference(&outer_item);
        assert_eq!(rc.count(), 0);
        assert!(!outer.is_stack_referenced());
        assert!(!inner.is_stack_referenced());
    }

    #[test]
    fn removing_from_an_unreferenced_compound_changes_nothing() {
        let rc = ReferenceCounter::new();
        let (_, item) = array(vec![StackItem::from_int(1)]);
        rc.add_stack_reference(&StackItem::Null, 1);
        rc.remove_stack_reference(&item);
        assert_eq!(rc.count(), 1);
    }

    #[test]
    fn map_sub_items_are_keys_and_values() {
        let rc = ReferenceCounter::new();
        let (_, value) = array(vec![StackItem::Null]);
        let map = Map::new(Vec::new());
        map.set(StackItem::from_int(1), value).expect("set");
        map.set(StackItem::from_int(2), StackItem::from_int(3))
            .expect("set");
        let item = StackItem::Map(map);

        rc.add_stack_reference(&item, 1);
        // map + 2 keys + array + Null + integer
        assert_eq!(rc.count(), 6);
        rc.remove_stack_reference(&item);
        assert_eq!(rc.count(), 0);
    }

    #[test]
    fn a_value_that_holds_itself_is_never_released() {
        let rc = ReferenceCounter::new();
        let (array, item) = array(Vec::new());
        rc.add_stack_reference(&item, 1);
        array.push(item.clone()).expect("push");
        rc.add_stack_reference(&item, 1);
        assert_eq!(rc.count(), 2);
        assert_eq!(array.stack_references(), 2);

        rc.remove_stack_reference(&item);
        assert_eq!(rc.count(), 1);
        assert_eq!(array.stack_references(), 1);
    }

    #[test]
    fn a_shared_sub_item_is_counted_once_per_referenced_parent() {
        let rc = ReferenceCounter::new();
        let (shared, shared_item) = array(vec![StackItem::from_int(7)]);
        let structure = Struct::new(vec![shared_item.clone(), shared_item.clone()]);
        let item = StackItem::Struct(structure);

        rc.add_stack_reference(&item, 1);
        // struct + shared (twice) + the integer (once)
        assert_eq!(rc.count(), 4);
        assert_eq!(shared.stack_references(), 2);
        rc.remove_stack_reference(&item);
        assert_eq!(rc.count(), 0);
        assert!(!shared.is_stack_referenced());
    }

    #[test]
    fn a_deep_chain_is_walked_without_recursion() {
        const DEPTH: usize = 100_000;
        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let rc = ReferenceCounter::new();
                let mut head = StackItem::Array(Array::new(Vec::new()));
                for _ in 1..DEPTH {
                    head = StackItem::Array(Array::new(vec![head]));
                }
                rc.add_stack_reference(&head, 1);
                let added = rc.count();
                rc.remove_stack_reference(&head);
                let removed = rc.count();

                // Take the chain apart link by link so that dropping it does
                // not recurse either.
                let mut next = Some(head);
                while let Some(StackItem::Array(array)) = next {
                    next = array.with_items_mut(Vec::pop);
                }
                (added, removed)
            })
            .expect("spawn");
        assert_eq!(handle.join().expect("join"), (DEPTH, 0));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "reference count underflow")]
    fn an_unbalanced_release_is_reported_in_debug_builds() {
        let rc = ReferenceCounter::new();
        rc.remove_stack_reference(&StackItem::Null);
    }

    #[test]
    fn the_limit_check_only_fails_above_the_limit() {
        let rc = ReferenceCounter::new();
        rc.add_stack_reference(&StackItem::Null, 2048);
        assert!(rc.post_execute_instruction(2048).is_ok());
        rc.add_stack_reference(&StackItem::Null, 1);
        let err = rc.post_execute_instruction(2048).expect_err("over limit");
        assert!(err.to_string().contains("MaxStackSize exceed: 2049/2048"));
    }
}
