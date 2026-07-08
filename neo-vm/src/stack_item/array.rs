//! Array stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Array stack item implementation used in the Neo VM.

use crate::error::{VmError, VmResult};
use crate::reference_counter::{CompoundId, ReferenceCounter};
use crate::stack_item::StackItem;
use neo_vm_rs::StackItemType;
use neo_vm_rs::next_stack_item_id;
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
    /// Reference counter shared with the VM (mirrors C# behaviour).
    reference_counter: Option<ReferenceCounter>,
    /// Unique identifier to mimic reference equality semantics.
    id: usize,
    /// Indicates whether the array is read-only.
    is_read_only: bool,
}

impl Array {
    /// Creates a new array with the specified items.
    pub fn new(
        mut items: Vec<StackItem>,
        reference_counter: Option<ReferenceCounter>,
    ) -> VmResult<Self> {
        if let Some(rc) = &reference_counter {
            for item in &mut items {
                item.attach_reference_counter(rc)?;
            }
        }

        // C# v3.10.1: constructing a compound does NOT reference-count its
        // children — they are counted via the AddStackReference recursion only
        // when the compound first becomes stack-referenced (e.g. on Push).
        let array = Self {
            inner: Arc::new(Mutex::new(ArrayInner {
                items,
                reference_counter,
                id: next_stack_item_id() as usize,
                is_read_only: false,
            })),
        };

        Ok(array)
    }

    /// Creates a new array without a reference counter.
    #[must_use]
    pub fn new_untracked(items: Vec<StackItem>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ArrayInner {
                items,
                reference_counter: None,
                id: next_stack_item_id() as usize,
                is_read_only: false,
            })),
        }
    }

    /// Returns the reference counter associated with this array, if any.
    #[must_use]
    pub fn reference_counter(&self) -> Option<ReferenceCounter> {
        self.inner.lock().reference_counter.clone()
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
    pub fn set(&self, index: usize, mut item: StackItem) -> VmResult<()> {
        let (rc_opt, referenced, old_item, new_item) = {
            let inner = self.inner.lock();
            if index >= inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            Self::ensure_mutable(&inner)?;
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Array(inner.id)));
            (rc_opt, referenced, inner.items[index].clone(), item.clone())
        };
        if let Some(rc) = &rc_opt {
            item.attach_reference_counter(rc)?;
            Self::validate_compound_reference(rc, &item)?;
        }
        {
            let mut inner = self.inner.lock();
            if index >= inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            Self::ensure_mutable(&inner)?;
            inner.items[index] = item;
        }
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.remove_stack_reference(&old_item);
                rc.add_stack_reference(&new_item, 1);
            }
        }
        Ok(())
    }

    /// Adds an item to the end of the array.
    pub fn push(&self, mut item: StackItem) -> VmResult<()> {
        let (rc_opt, referenced) = {
            let inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Array(inner.id)));
            (rc_opt, referenced)
        };
        if let Some(rc) = &rc_opt {
            item.attach_reference_counter(rc)?;
            Self::validate_compound_reference(rc, &item)?;
        }
        {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            inner.items.push(item.clone());
        }
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.add_stack_reference(&item, 1);
            }
        }
        Ok(())
    }

    /// Removes and returns the last item in the array.
    pub fn pop(&self) -> VmResult<StackItem> {
        let (item, rc_opt, referenced) = {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            let item = inner
                .items
                .pop()
                .ok_or_else(|| VmError::invalid_operation_msg("Array is empty"))?;
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Array(inner.id)));
            (item, rc_opt, referenced)
        };
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.remove_stack_reference(&item);
            }
        }

        Ok(item)
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
        let (rc, sub_items) = {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            // C# v3.10.1 CLEARITEMS snapshots sub-items, clears first, then
            // releases the snapshot. Clearing first breaks self-cycles before
            // recursive reference removal asks the same compound for SubItems.
            let id = inner.id;
            let rc = inner.reference_counter.clone();
            let sub_items = if rc
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Array(id)))
            {
                inner.items.clone()
            } else {
                Vec::new()
            };
            inner.items.clear();
            (rc, sub_items)
        };
        if let Some(rc) = rc {
            for item in sub_items {
                rc.remove_stack_reference(&item);
            }
        }
        Ok(())
    }

    /// Creates a deep copy of the array.
    pub fn deep_copy(&self, reference_counter: Option<ReferenceCounter>) -> VmResult<Self> {
        let items = self.with_items(|items| items.iter().map(|item| item.deep_clone()).collect());
        let copy = Self::new(items, reference_counter)?;
        copy.set_read_only(true);
        Ok(copy)
    }

    /// Gets the type of the stack item.
    #[must_use]
    pub const fn stack_item_type(&self) -> StackItemType {
        StackItemType::Array
    }

    /// Inserts an item at the specified index.
    pub fn insert(&self, index: usize, mut item: StackItem) -> VmResult<()> {
        let (rc_opt, referenced) = {
            let inner = self.inner.lock();
            if index > inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            Self::ensure_mutable(&inner)?;
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Array(inner.id)));
            (rc_opt, referenced)
        };
        if let Some(rc) = &rc_opt {
            item.attach_reference_counter(rc)?;
            Self::validate_compound_reference(rc, &item)?;
        }
        {
            let mut inner = self.inner.lock();
            if index > inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            Self::ensure_mutable(&inner)?;
            inner.items.insert(index, item.clone());
        }
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.add_stack_reference(&item, 1);
            }
        }
        Ok(())
    }

    /// Removes the item at the specified index.
    pub fn remove(&self, index: usize) -> VmResult<StackItem> {
        let (removed, rc_opt, referenced) = {
            let mut inner = self.inner.lock();
            if index >= inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }

            Self::ensure_mutable(&inner)?;
            let removed = inner.items.remove(index);
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Array(inner.id)));
            (removed, rc_opt, referenced)
        };
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.remove_stack_reference(&removed);
            }
        }

        Ok(removed)
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

    fn ensure_mutable(inner: &ArrayInner) -> VmResult<()> {
        if inner.is_read_only {
            Err(VmError::invalid_operation_msg(
                "The array is readonly, can not modify.".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn validate_compound_reference(rc: &ReferenceCounter, item: &StackItem) -> VmResult<()> {
        match item {
            StackItem::Array(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set an Array without a ReferenceCounter.".to_string(),
                )),
            },
            StackItem::Struct(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set an Array without a ReferenceCounter.".to_string(),
                )),
            },
            StackItem::Map(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set an Array without a ReferenceCounter.".to_string(),
                )),
            },
            _ => Ok(()),
        }
    }

    /// Ensures the array and its children share the provided reference counter.
    pub(crate) fn attach_reference_counter(&self, rc: &ReferenceCounter) -> VmResult<()> {
        let children = {
            let mut inner = self.inner.lock();
            if let Some(existing) = &inner.reference_counter {
                if existing.ptr_eq(rc) {
                    return Ok(());
                }
                return Err(VmError::invalid_operation_msg(
                    "Array has mismatched reference counter.",
                ));
            }

            let children = inner.items.clone();
            inner.reference_counter = Some(rc.clone());
            children
        };

        for mut item in children {
            item.attach_reference_counter(rc)?;
        }

        // No reference counting on attach: children are counted via the
        // AddStackReference recursion when this compound becomes stack-referenced.
        Ok(())
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
