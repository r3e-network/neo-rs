//! Array stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Array stack item implementation used in the Neo VM.

use crate::StackItemType;
use crate::error::VmResult;
use crate::reference_counter::{StackReferenceRelease, SubItems};
use crate::stack_item::StackItem;
use crate::stack_item::compound::CompoundInner;
use parking_lot::Mutex;
use std::sync::Arc;

/// Diagnostic reported when an array is mutated after being frozen.
const READ_ONLY: &str = "The array is readonly, can not modify.";
/// Diagnostic reported when an item is popped from an empty array.
const EMPTY: &str = "Array is empty";

/// Represents an array of stack items in the VM.
#[derive(Debug, Clone)]
pub struct Array {
    inner: Arc<Mutex<CompoundInner>>,
}

impl Array {
    /// Creates a new array with the specified items.
    ///
    /// As in neo-vm v3.10.1, an array holds no reference counter and none of
    /// its methods touch one: the opcode handlers do the counting.
    #[must_use]
    pub fn new(items: Vec<StackItem>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CompoundInner::new(items))),
        }
    }

    /// Returns the number of stack references held on this array.
    #[must_use]
    pub fn stack_references(&self) -> usize {
        self.inner.lock().stack_references
    }

    /// Returns whether this array is stack referenced (C# `IsStackReferenced`).
    #[must_use]
    pub fn is_stack_referenced(&self) -> bool {
        self.inner.lock().stack_references != 0
    }

    /// Adds `count` stack references. Returns the sub items when this call
    /// made the array stack referenced.
    pub(crate) fn add_stack_references(&self, count: usize) -> Option<SubItems> {
        self.inner.lock().add_stack_references(count)
    }

    /// Removes one stack reference, if the array is stack referenced.
    pub(crate) fn remove_stack_reference(&self) -> StackReferenceRelease {
        self.inner.lock().remove_stack_reference()
    }

    /// Takes a snapshot of the sub items (C# `SubItems`).
    pub(crate) fn sub_items(&self) -> SubItems {
        self.inner.lock().sub_items()
    }

    /// Returns the unique identifier for this array.
    #[must_use]
    pub fn id(&self) -> usize {
        self.inner.lock().id
    }

    /// Returns whether the array is marked as read-only.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.inner.lock().is_read_only
    }

    /// Sets the read-only flag.
    pub fn set_read_only(&self, read_only: bool) {
        self.inner.lock().is_read_only = read_only;
    }

    /// Gets the items in the array.
    #[must_use]
    pub fn items(&self) -> Vec<StackItem> {
        self.inner.lock().items.clone()
    }

    /// Returns a stable pointer used for identity tracking.
    #[must_use]
    pub fn as_ptr(&self) -> *const StackItem {
        self.inner.lock().items.as_ptr()
    }

    /// Gets the item at the specified index.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<StackItem> {
        self.inner.lock().get(index)
    }

    /// Sets the item at the specified index.
    pub fn set(&self, index: usize, item: StackItem) -> VmResult<()> {
        // The replaced item is dropped after the lock is released.
        let replaced = self.inner.lock().set(index, item, READ_ONLY)?;
        drop(replaced);
        Ok(())
    }

    /// Adds an item to the end of the array.
    pub fn push(&self, item: StackItem) -> VmResult<()> {
        self.inner.lock().push(item, READ_ONLY)
    }

    /// Removes and returns the last item in the array.
    pub fn pop(&self) -> VmResult<StackItem> {
        self.inner.lock().pop(EMPTY, READ_ONLY)
    }

    /// Gets the number of items in the array.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().items.len()
    }

    /// Returns true if the array is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().items.is_empty()
    }

    /// Removes all items from the array.
    pub fn clear(&self) -> VmResult<()> {
        // The removed items are dropped after the lock is released.
        let removed = self.inner.lock().clear(READ_ONLY)?;
        drop(removed);
        Ok(())
    }

    /// Creates a deep copy of the array.
    pub fn deep_copy(&self) -> VmResult<Self> {
        // Snapshot first: cloning a sub item may need this array's lock.
        let items = self
            .items()
            .iter()
            .map(super::stack_item::StackItem::deep_clone)
            .collect();
        let copy = Self::new(items);
        copy.set_read_only(true);
        Ok(copy)
    }

    /// Gets the type of the stack item.
    #[must_use]
    pub const fn stack_item_type(&self) -> StackItemType {
        StackItemType::Array
    }

    /// Inserts an item at the specified index.
    pub fn insert(&self, index: usize, item: StackItem) -> VmResult<()> {
        self.inner.lock().insert(index, item, READ_ONLY)
    }

    /// Removes the item at the specified index.
    pub fn remove(&self, index: usize) -> VmResult<StackItem> {
        self.inner.lock().remove(index, READ_ONLY)
    }

    /// Returns an iterator over the items.
    #[must_use]
    pub fn iter(&self) -> std::vec::IntoIter<StackItem> {
        self.items().into_iter()
    }

    /// Provides zero-copy read access to the items under the lock.
    ///
    /// This avoids the `Vec` clone that `items()` performs, which is significant
    /// when the caller only needs to inspect or iterate without taking ownership.
    #[inline]
    pub fn with_items<R>(&self, f: impl FnOnce(&[StackItem]) -> R) -> R {
        let inner = self.inner.lock();
        f(&inner.items)
    }

    /// Provides zero-copy mutable access to the items under the lock.
    #[inline]
    pub fn with_items_mut<R>(&self, f: impl FnOnce(&mut Vec<StackItem>) -> R) -> R {
        let mut inner = self.inner.lock();
        f(&mut inner.items)
    }

    /// Reverses the order of items in the array.
    pub fn reverse_items(&self) -> VmResult<()> {
        self.inner.lock().reverse(READ_ONLY)
    }

    /// One-lock snapshot of the fields the `Debug` renderer prints, as
    /// `(items, id, stack_references, is_read_only)`.
    pub(crate) fn debug_snapshot(&self) -> (Vec<StackItem>, usize, usize, bool) {
        self.inner.lock().debug_snapshot()
    }
}

impl From<Array> for Vec<StackItem> {
    fn from(array: Array) -> Self {
        array.items()
    }
}

impl IntoIterator for Array {
    type Item = StackItem;
    type IntoIter = std::vec::IntoIter<StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items().into_iter()
    }
}

impl IntoIterator for &Array {
    type Item = StackItem;
    type IntoIter = std::vec::IntoIter<StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items().into_iter()
    }
}
