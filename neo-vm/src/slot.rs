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
    #[must_use]
    pub fn new_with_items(items: Vec<StackItem>, reference_counter: ReferenceCounter) -> Self {
        Self::with_items(items, reference_counter)
    }

    /// Create a slot of the specified size.
    /// public Slot(int count, `IReferenceCounter` referenceCounter)
    #[must_use]
    pub fn new(count: usize, reference_counter: ReferenceCounter) -> Self {
        let items = vec![StackItem::Null; count];

        for _ in 0..count {
            reference_counter.add_stack_reference(&StackItem::Null, 1);
        }

        Self {
            reference_counter,
            items,
        }
    }

    /// Gets the item at the specified index in the slot.
    /// public `StackItem` this[int index] { get }
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&StackItem> {
        self.items.get(index)
    }

    /// Sets the item at the specified index in the slot.
    /// public `StackItem` this[int index] { internal set }
    pub fn set(&mut self, index: usize, value: StackItem) -> VmResult<()> {
        if index >= self.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        let slot_item = &mut self.items[index];
        self.reference_counter.remove_stack_reference(slot_item);
        *slot_item = value;
        self.reference_counter.add_stack_reference(slot_item, 1);
        Ok(())
    }

    /// Resets all items to `Null`, mirroring how the C# engine clears slots when contexts unload.
    pub fn clear(&mut self) {
        for item in &mut self.items {
            self.reference_counter.remove_stack_reference(item);
            *item = StackItem::Null;
            self.reference_counter.add_stack_reference(item, 1);
        }
    }

    /// Gets the number of items in the slot.
    /// public int Count => _items.Length;
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Returns the number of items in the slot (Rust-style).
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true when the slot holds zero items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// internal void `ClearReferences()`
    pub(crate) fn clear_references(&mut self) {
        for item in &mut self.items {
            self.reference_counter.remove_stack_reference(item);
            *item = StackItem::Null;
            self.reference_counter.add_stack_reference(item, 1);
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
