//! Array stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Array stack item implementation used in the Neo VM.

use crate::error::{VmError, VmResult};
use crate::reference_counter::{CompoundParent, ReferenceCounter};
use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::stack_item_vertex::next_stack_item_id;
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
        items: Vec<StackItem>,
        reference_counter: Option<ReferenceCounter>,
    ) -> VmResult<Self> {
        let array = Self {
            inner: Arc::new(Mutex::new(ArrayInner {
                items,
                reference_counter,
                id: next_stack_item_id(),
                is_read_only: false,
            })),
        };

        if let Some(rc) = array.reference_counter() {
            array.add_reference_for_items(&rc)?;
        }

        Ok(array)
    }

    /// Creates a new array without a reference counter.
    #[must_use] 
    pub fn new_untracked(items: Vec<StackItem>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ArrayInner {
                items,
                reference_counter: None,
                id: next_stack_item_id(),
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
    pub fn set(&self, index: usize, item: StackItem) -> VmResult<()> {
        let mut inner = self.inner.lock();
        if index >= inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        Self::ensure_mutable(&inner)?;

        if let Some(rc) = &inner.reference_counter {
            Self::validate_compound_reference(rc, &item)?;
            let parent = CompoundParent::Array(inner.id);
            rc.remove_compound_reference(&inner.items[index], parent);
            rc.add_compound_reference(&item, parent);
        }

        inner.items[index] = item;
        Ok(())
    }

    /// Adds an item to the end of the array.
    pub fn push(&self, item: StackItem) -> VmResult<()> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;

        if let Some(rc) = &inner.reference_counter {
            Self::validate_compound_reference(rc, &item)?;
            rc.add_compound_reference(&item, CompoundParent::Array(inner.id));
        }

        inner.items.push(item);
        Ok(())
    }

    /// Removes and returns the last item in the array.
    pub fn pop(&self) -> VmResult<StackItem> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        let item = inner
            .items
            .pop()
            .ok_or_else(|| VmError::invalid_operation_msg("Array is empty"))?;

        if let Some(rc) = &inner.reference_counter {
            rc.remove_compound_reference(&item, CompoundParent::Array(inner.id));
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
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        if let Some(rc) = &inner.reference_counter {
            let parent = CompoundParent::Array(inner.id);
            for item in &inner.items {
                rc.remove_compound_reference(item, parent);
            }
        }
        inner.items.clear();
        Ok(())
    }

    /// Creates a deep copy of the array.
    pub fn deep_copy(&self, reference_counter: Option<ReferenceCounter>) -> VmResult<Self> {
        let items = self
            .items()
            .into_iter()
            .map(|item| item.deep_clone())
            .collect();
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
    pub fn insert(&self, index: usize, item: StackItem) -> VmResult<()> {
        let mut inner = self.inner.lock();
        if index > inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        Self::ensure_mutable(&inner)?;

        if let Some(rc) = &inner.reference_counter {
            Self::validate_compound_reference(rc, &item)?;
            rc.add_compound_reference(&item, CompoundParent::Array(inner.id));
        }

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
        let removed = inner.items.remove(index);

        if let Some(rc) = &inner.reference_counter {
            rc.remove_compound_reference(&removed, CompoundParent::Array(inner.id));
        }

        Ok(removed)
    }

    /// Returns an iterator over the items.
    #[must_use] 
    pub fn iter(&self) -> std::vec::IntoIter<StackItem> {
        self.items().into_iter()
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

    fn add_reference_for_items(&self, rc: &ReferenceCounter) -> VmResult<()> {
        let items = self.items();
        let parent = CompoundParent::Array(self.id());
        for item in &items {
            Self::validate_compound_reference(rc, item)?;
            rc.add_compound_reference(item, parent);
        }
        Ok(())
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
        {
            let mut inner = self.inner.lock();
            if let Some(existing) = &inner.reference_counter {
                if existing.ptr_eq(rc) {
                    return Ok(());
                }
                return Err(VmError::invalid_operation_msg(
                    "Array has mismatched reference counter.",
                ));
            }

            for item in &mut inner.items {
                item.attach_reference_counter(rc)?;
            }

            inner.reference_counter = Some(rc.clone());
        }

        self.add_reference_for_items(rc)?;
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
