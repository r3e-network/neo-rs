//! Slot storage for local variables, arguments and static fields.
//!
//! This is a faithful port of `Neo.VM/Slot.cs` from the C# reference node. A slot owns
//! a fixed-size collection of [`StackItem`]s and keeps the VM reference counter in sync
//! whenever the slot content changes.

use crate::reference_counter::ReferenceCounter;
use crate::stack_item::StackItem;
use crate::{VmError, VmResult};

/// Stores local variables, arguments or static fields for a single execution context.
#[derive(Clone, Debug)]
pub struct Slot {
    items: Vec<StackItem>,
    reference_counter: ReferenceCounter,
}

impl Slot {
    /// Creates a slot populated with the provided items.
    pub fn new(items: Vec<StackItem>, reference_counter: ReferenceCounter) -> Self {
        let mut slot = Self {
            items: Vec::with_capacity(items.len()),
            reference_counter,
        };

        for item in items {
            slot.push_internal(item);
        }

        slot
    }

    /// Creates a slot of the requested size initialised with `StackItem::null()`.
    pub fn with_count(count: usize, reference_counter: ReferenceCounter) -> Self {
        let mut slot = Self {
            items: Vec::with_capacity(count),
            reference_counter,
        };

        for _ in 0..count {
            slot.push_internal(StackItem::null());
        }

        slot
    }

    /// Returns the number of elements stored in the slot.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` when the slot does not contain any items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Provides immutable access to the backing slice. Intended for diagnostics/tests.
    pub fn as_slice(&self) -> &[StackItem] {
        &self.items
    }

    /// Returns the item at `index`.
    pub fn get(&self, index: usize) -> VmResult<&StackItem> {
        self.items
            .get(index)
            .ok_or_else(|| self.index_out_of_range(index))
    }

    /// Returns a mutable reference to the item at `index`.
    pub fn get_mut(&mut self, index: usize) -> VmResult<&mut StackItem> {
        if index >= self.items.len() {
            return Err(self.index_out_of_range(index));
        }
        Ok(&mut self.items[index])
    }

    /// Replaces the item stored at `index`, updating reference counters accordingly.
    pub fn set(&mut self, index: usize, item: StackItem) -> VmResult<()> {
        if index >= self.items.len() {
            return Err(self.index_out_of_range(index));
        }

        let old_value = &self.items[index];
        self.reference_counter.remove_stack_reference(old_value);
        self.reference_counter.add_stack_reference(&item);
        self.items[index] = item;
        Ok(())
    }

    /// Removes every stack reference tracked for this slot.
    pub fn clear_references(&mut self) {
        for item in &self.items {
            self.reference_counter.remove_stack_reference(item);
        }

        for slot_item in &mut self.items {
            *slot_item = StackItem::null();
        }
    }

    /// Returns an iterator over the contained items.
    pub fn iter(&self) -> impl Iterator<Item = &StackItem> {
        self.items.iter()
    }

    /// Returns a mutable iterator over the contained items.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut StackItem> {
        self.items.iter_mut()
    }

    /// Internal helper used during construction to append a new item while keeping
    /// the reference counter in sync.
    fn push_internal(&mut self, item: StackItem) {
        self.reference_counter.add_stack_reference(&item);
        self.items.push(item);
    }

    fn index_out_of_range(&self, index: usize) -> VmError {
        VmError::invalid_operation_msg(format!(
            "Index {index} out of range for slot of size {}",
            self.items.len()
        ))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.clear_references();
    }
}
