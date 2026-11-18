//! Array stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Array stack item implementation used in the Neo VM.

use crate::StackItemType;
use crate::error::{VmError, VmResult};
use crate::next_stack_item_id;
use crate::reference_counter::{StackReferenceRelease, SubItems};
use crate::stack_item::StackItem;
use parking_lot::Mutex;
use std::sync::Arc;

/// Represents an array of stack items in the VM.
#[derive(Debug, Clone)]
pub struct Array {
    inner: Arc<Mutex<ArrayInner>>,
}

#[derive(Debug)]
struct ArrayInner {
    /// The items in the array.
    items: Vec<StackItem>,
    /// Number of stack references held on this array (C# `CompoundType.StackReferences`).
    stack_references: usize,
    /// Unique identifier to mimic reference equality semantics.
    id: usize,
    /// Indicates whether the array is read-only.
    is_read_only: bool,
}

impl Array {
    /// Creates a new array with the specified items.
    ///
    /// As in neo-vm v3.10.1, an array holds no reference counter and none of
    /// its methods touch one: the opcode handlers do the counting.
    #[must_use]
    pub fn new(items: Vec<StackItem>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ArrayInner {
                items,
                stack_references: 0,
                id: next_stack_item_id(),
                is_read_only: false,
            })),
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
        let mut inner = self.inner.lock();
        inner.stack_references += count;
        (inner.stack_references == count).then(|| SubItems::collect(&inner.items))
    }

    /// Removes one stack reference, if the array is stack referenced.
    pub(crate) fn remove_stack_reference(&self) -> StackReferenceRelease {
        let mut inner = self.inner.lock();
        if inner.stack_references == 0 {
            return StackReferenceRelease::NotReferenced;
        }
        inner.stack_references -= 1;
        if inner.stack_references == 0 {
            StackReferenceRelease::Unreferenced(SubItems::collect(&inner.items))
        } else {
            StackReferenceRelease::StillReferenced
        }
    }

    /// Takes a snapshot of the sub items (C# `SubItems`).
    pub(crate) fn sub_items(&self) -> SubItems {
        SubItems::collect(&self.inner.lock().items)
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
        self.inner.lock().items.get(index).cloned()
    }

    /// Sets the item at the specified index.
    pub fn set(&self, index: usize, item: StackItem) -> VmResult<()> {
        // The replaced item is dropped after the lock is released.
        let _replaced = {
            let mut inner = self.inner.lock();
            if index >= inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }

            Self::ensure_mutable(&inner)?;
            std::mem::replace(&mut inner.items[index], item)
        };
        Ok(())
    }

    /// Adds an item to the end of the array.
    pub fn push(&self, item: StackItem) -> VmResult<()> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        inner.items.push(item);
        Ok(())
    }

    /// Removes and returns the last item in the array.
    pub fn pop(&self) -> VmResult<StackItem> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        inner
            .items
            .pop()
            .ok_or_else(|| VmError::invalid_operation_msg("Array is empty"))
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
        let _removed = {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            std::mem::take(&mut inner.items)
        };
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
        let mut inner = self.inner.lock();
        if index > inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        Self::ensure_mutable(&inner)?;
        inner.items.insert(index, item);
        Ok(())
    }

    /// Removes the item at the specified index.
    pub fn remove(&self, index: usize) -> VmResult<StackItem> {
        let mut inner = self.inner.lock();
        if index >= inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        Self::ensure_mutable(&inner)?;
        Ok(inner.items.remove(index))
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
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        inner.items.reverse();
        Ok(())
    }

    /// One-lock snapshot of the fields the `Debug` renderer prints, as
    /// `(items, id, stack_references, is_read_only)`.
    pub(crate) fn debug_snapshot(&self) -> (Vec<StackItem>, usize, usize, bool) {
        let inner = self.inner.lock();
        (
            inner.items.clone(),
            inner.id,
            inner.stack_references,
            inner.is_read_only,
        )
    }

    fn ensure_mutable(inner: &ArrayInner) -> VmResult<()> {
        if inner.is_read_only {
            Err(VmError::invalid_operation_msg(
                "The array is readonly, can not modify.".to_string(),
            ))
        } else {
            Ok(())
        }
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
