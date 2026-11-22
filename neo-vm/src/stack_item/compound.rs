//! Shared state for the compound stack item types.
//!
//! [`Array`](super::Array) and [`Struct`](super::Struct) are the same value: an
//! ordered list of stack items with a read-only flag, a unique id that mirrors
//! reference equality and a stack reference counter (C# `CompoundType`). They
//! differ only in their type tag and in the diagnostics they report. The state
//! and every operation on it live here so that the behaviour is implemented
//! once instead of per wrapper.

use crate::error::{VmError, VmResult};
use crate::next_stack_item_id;
use crate::reference_counter::{StackReferenceRelease, SubItems};
use crate::stack_item::StackItem;

/// The state behind one compound stack item.
#[derive(Debug)]
pub(crate) struct CompoundInner {
    /// The items in the compound value.
    pub(crate) items: Vec<StackItem>,
    /// Number of stack references held on this value (C# `CompoundType.StackReferences`).
    ///
    /// Signed like the C# `int`: the dynamically priced handlers decrement it
    /// directly (UNPACK, VALUES), and a negative value still reads as
    /// stack-referenced.
    pub(crate) stack_references: isize,
    /// Unique identifier to mimic reference equality semantics.
    pub(crate) id: usize,
    /// Indicates whether the value is read-only.
    pub(crate) is_read_only: bool,
}

impl CompoundInner {
    /// Creates the state for a new compound value with the specified items.
    pub(crate) fn new(items: Vec<StackItem>) -> Self {
        Self {
            items,
            stack_references: 0,
            id: next_stack_item_id(),
            is_read_only: false,
        }
    }

    /// Rejects a mutation of a read-only value, reporting `message`.
    #[inline]
    fn ensure_mutable(&self, message: &'static str) -> VmResult<()> {
        if self.is_read_only {
            Err(VmError::invalid_operation_msg(message.to_string()))
        } else {
            Ok(())
        }
    }

    /// Returns the item at `index`, if there is one.
    pub(crate) fn get(&self, index: usize) -> Option<StackItem> {
        self.items.get(index).cloned()
    }

    /// Sets the item at `index` and returns the item it displaced.
    ///
    /// The caller drops the returned item only after releasing the lock, since
    /// dropping a compound item may take other locks.
    pub(crate) fn set(
        &mut self,
        index: usize,
        item: StackItem,
        message: &'static str,
    ) -> VmResult<StackItem> {
        if index >= self.items.len() {
            return Err(index_out_of_range(index));
        }

        self.ensure_mutable(message)?;
        Ok(std::mem::replace(&mut self.items[index], item))
    }

    /// Adds an item to the end of the value.
    pub(crate) fn push(&mut self, item: StackItem, message: &'static str) -> VmResult<()> {
        self.ensure_mutable(message)?;
        self.items.push(item);
        Ok(())
    }

    /// Removes and returns the last item, reporting `empty` when there is none.
    pub(crate) fn pop(
        &mut self,
        empty: &'static str,
        message: &'static str,
    ) -> VmResult<StackItem> {
        self.ensure_mutable(message)?;
        self.items
            .pop()
            .ok_or_else(|| VmError::invalid_operation_msg(empty))
    }

    /// Inserts an item at `index`.
    pub(crate) fn insert(
        &mut self,
        index: usize,
        item: StackItem,
        message: &'static str,
    ) -> VmResult<()> {
        if index > self.items.len() {
            return Err(index_out_of_range(index));
        }

        self.ensure_mutable(message)?;
        self.items.insert(index, item);
        Ok(())
    }

    /// Removes and returns the item at `index`.
    pub(crate) fn remove(&mut self, index: usize, message: &'static str) -> VmResult<StackItem> {
        if index >= self.items.len() {
            return Err(index_out_of_range(index));
        }

        self.ensure_mutable(message)?;
        Ok(self.items.remove(index))
    }

    /// Empties the value and returns the items it held.
    ///
    /// The caller drops the returned items only after releasing the lock, since
    /// dropping a compound item may take other locks.
    pub(crate) fn clear(&mut self, message: &'static str) -> VmResult<Vec<StackItem>> {
        self.ensure_mutable(message)?;
        Ok(std::mem::take(&mut self.items))
    }

    /// Reverses the order of the items.
    pub(crate) fn reverse(&mut self, message: &'static str) -> VmResult<()> {
        self.ensure_mutable(message)?;
        self.items.reverse();
        Ok(())
    }

    /// Adds `count` stack references. Returns the sub items when this call made
    /// the value stack referenced.
    pub(crate) fn add_stack_references(&mut self, count: usize) -> Option<SubItems> {
        self.stack_references += count as isize;
        (self.stack_references == count as isize).then(|| SubItems::collect(&self.items))
    }

    /// Adds `count` stack references without touching any sub item.
    ///
    /// C# `CompoundType.StackReferences++` as used by the dynamically priced
    /// handlers: the handler accounts for the item and its sub items itself via
    /// `PushItemCounted`, so this must not propagate.
    pub(crate) fn inc_stack_references(&mut self, count: usize) {
        self.stack_references += count as isize;
    }

    /// Removes `count` stack references without touching any sub item.
    ///
    /// C# `CompoundType.StackReferences--` as used by UNPACK and VALUES.
    pub(crate) fn dec_stack_references(&mut self, count: usize) {
        self.stack_references -= count as isize;
    }

    /// Removes one stack reference, if the value is stack referenced.
    pub(crate) fn remove_stack_reference(&mut self) -> StackReferenceRelease {
        if self.stack_references == 0 {
            return StackReferenceRelease::NotReferenced;
        }
        self.stack_references -= 1;
        if self.stack_references == 0 {
            StackReferenceRelease::Unreferenced(SubItems::collect(&self.items))
        } else {
            StackReferenceRelease::StillReferenced
        }
    }

    /// Takes a snapshot of the sub items (C# `SubItems`).
    pub(crate) fn sub_items(&self) -> SubItems {
        SubItems::collect(&self.items)
    }

    /// One-lock snapshot of the fields the `Debug` renderer prints, as
    /// `(items, id, stack_references, is_read_only)`.
    pub(crate) fn debug_snapshot(&self) -> (Vec<StackItem>, usize, isize, bool) {
        (
            self.items.clone(),
            self.id,
            self.stack_references,
            self.is_read_only,
        )
    }
}

/// The error reported when an index falls outside the value.
fn index_out_of_range(index: usize) -> VmError {
    VmError::invalid_operation_msg(format!("Index out of range: {index}"))
}
