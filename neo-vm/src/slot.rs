//! Slot implementation.
//!
//! This module provides the Slot functionality exactly matching C# Neo.VM.Slot.

// Matches C# using directives exactly:
// using Neo.VM.Types;
// using System.Collections;
// using System.Collections.Generic;
// using Array = System.Array;

use crate::error::{VmError, VmResult};
use crate::reference_counter::ReferenceCounter;
use crate::stack_item::StackItem;
use std::ops::Index;

/// namespace Neo.VM -> public class Slot : `IReadOnlyList<StackItem>`
/// Used to store local variables, arguments and static fields in the VM.
#[derive(Clone, Debug)]
pub struct Slot {
    // private readonly IReferenceCounter _referenceCounter;
    reference_counter: ReferenceCounter,

    // private readonly StackItem[] _items;
    items: Vec<StackItem>,
}

impl Slot {
    /// Creates a slot containing the specified items.
    /// public Slot(StackItem[] items, `IReferenceCounter` referenceCounter)
    #[inline]
    #[must_use]
    pub fn with_items(items: Vec<StackItem>, reference_counter: ReferenceCounter) -> Self {
        for item in &items {
            reference_counter.add_stack_reference(item, 1);
        }

        Self {
            reference_counter,
            items,
        }
    }

    /// Convenience constructor matching the C# signature `new Slot(items, referenceCounter)`.
    #[inline]
    #[must_use]
    pub fn new_with_items(items: Vec<StackItem>, reference_counter: ReferenceCounter) -> Self {
        Self::with_items(items, reference_counter)
    }

    /// Create a slot of the specified size.
    /// public Slot(int count, `IReferenceCounter` referenceCounter)
    #[inline]
    #[must_use]
    pub fn new(count: usize, reference_counter: ReferenceCounter) -> Self {
        let items = vec![StackItem::Null; count];

        // Batch: single atomic add for all Null items instead of N individual adds.
        reference_counter.add_stack_reference(&StackItem::Null, count);

        Self {
            reference_counter,
            items,
        }
    }

    /// Creates a slot with the specified capacity, pre-allocating storage.
    /// This is more efficient than `new` when the items will be set later.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize, reference_counter: ReferenceCounter) -> Self {
        let mut items = Vec::with_capacity(capacity);
        items.resize(capacity, StackItem::Null);

        // Batch: single atomic add instead of N individual adds.
        reference_counter.add_stack_reference(&StackItem::Null, capacity);

        Self {
            reference_counter,
            items,
        }
    }

    /// Gets the item at the specified index in the slot.
    /// public `StackItem` this[int index] { get }
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&StackItem> {
        self.items.get(index)
    }

    /// Sets the item at the specified index in the slot.
    /// public `StackItem` this[int index] { internal set }
    #[inline]
    pub fn set(&mut self, index: usize, value: StackItem) -> VmResult<()> {
        if index >= self.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        // SAFETY: Bounds checked above
        let slot_item = unsafe { self.items.get_unchecked_mut(index) };
        self.reference_counter.remove_stack_reference(slot_item);
        *slot_item = value;
        self.reference_counter.add_stack_reference(slot_item, 1);
        Ok(())
    }

    /// Creates a slot from items popped with `PopNoRef`.
    ///
    /// neo-vm v3.11.2 `Slot(EvaluationStack stack, int n, IReferenceCounter)`:
    /// the items keep the stack references they already hold and the reference
    /// counter is not adjusted here. The first item is the topmost popped
    /// element.
    #[inline]
    #[must_use]
    pub fn from_popped_items(items: Vec<StackItem>, reference_counter: ReferenceCounter) -> Self {
        Self {
            reference_counter,
            items,
        }
    }

    /// Checks that `index` is within a slot of the given length, matching the
    /// C# v3.11.2 `Slot.Store` bounds check.
    #[inline]
    pub fn check_bounds(index: usize, len: usize) -> VmResult<()> {
        if index >= len {
            return Err(VmError::invalid_operation_msg(format!(
                "Out of stack bounds: {index}/{len}"
            )));
        }
        Ok(())
    }

    /// Stores an item that was popped via `PopNoRef` at the specified index.
    ///
    /// neo-vm v3.11.2 `Slot.Store`: the item moves from the evaluation stack to
    /// the slot, so its own reference is kept and only the previous slot
    /// value's reference is released. Callers must run `check_bounds` before
    /// popping so a failed store leaves the evaluation stack untouched.
    #[inline]
    pub fn store_popped(&mut self, index: usize, item: StackItem) {
        let old = std::mem::replace(&mut self.items[index], item);
        self.reference_counter.remove_stack_reference(&old);
    }

    /// Resets all items to `Null`, mirroring how the C# engine clears slots when contexts unload.
    pub fn clear(&mut self) {
        for item in &mut self.items {
            self.reference_counter.remove_stack_reference(item);
            *item = StackItem::Null;
        }
    }

    /// Gets the number of items in the slot.
    /// public int Count => _items.Length;
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Returns the number of items in the slot (Rust-style).
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true when the slot holds zero items.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// internal void `ClearReferences()`
    /// Matches C# Slot.ClearReferences: only removes stack references, doesn't replace items.
    pub(crate) fn clear_references(&mut self) {
        for item in &self.items {
            self.reference_counter.remove_stack_reference(item);
        }
    }

    /// Get an iterator over the items
    pub fn iter(&self) -> impl Iterator<Item = &StackItem> {
        self.items.iter()
    }

    /// Get a mutable iterator over the items
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut StackItem> {
        self.items.iter_mut()
    }

    /// Returns a copy of the underlying items as a Vec (matching C# Slot.ToArray semantics).
    #[must_use]
    pub fn to_vec(&self) -> Vec<StackItem> {
        self.items.clone()
    }

    /// Removes the item at the specified index.
    pub fn remove(&mut self, index: usize) -> VmResult<StackItem> {
        if index >= self.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        let removed = self.items.remove(index);
        self.reference_counter.remove_stack_reference(&removed);
        Ok(removed)
    }

    /// Consumes the slot and returns the underlying items.
    #[must_use]
    pub fn into_vec(self) -> Vec<StackItem> {
        self.items
    }
}

impl Index<usize> for Slot {
    type Output = StackItem;

    fn index(&self, index: usize) -> &Self::Output {
        &self.items[index]
    }
}

// IEnumerable<StackItem> implementation
impl IntoIterator for Slot {
    type Item = StackItem;
    type IntoIter = std::vec::IntoIter<StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
