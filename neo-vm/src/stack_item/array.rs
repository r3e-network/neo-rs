//! Array stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Array stack item implementation used in the Neo VM.

use crate::error::{VmError, VmResult};
use crate::reference_counter::{CompoundParent, ReferenceCounter};
use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::stack_item_vertex::next_stack_item_id;
use crate::stack_item::StackItem;
use std::ops::{Index, IndexMut};

/// Represents an array of stack items in the VM.
#[derive(Debug)]
pub struct Array {
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
    pub fn new(items: Vec<StackItem>, reference_counter: Option<ReferenceCounter>) -> Self {
        let mut array = Self {
            items,
            reference_counter,
            id: next_stack_item_id(),
            is_read_only: false,
        };

        if let Some(rc) = array.reference_counter.clone() {
            array.add_reference_for_items(&rc);
            array.reference_counter = Some(rc);
        }

        array
    }

    /// Returns the reference counter associated with this array, if any.
    pub fn reference_counter(&self) -> Option<&ReferenceCounter> {
        self.reference_counter.as_ref()
    }

    /// Returns the unique identifier for this array.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Returns whether the array is marked as read-only.
    pub fn is_read_only(&self) -> bool {
        self.is_read_only
    }

    /// Sets the read-only flag.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.is_read_only = read_only;
    }

    /// Gets the items in the array.
    pub fn items(&self) -> &[StackItem] {
        &self.items
    }

    /// Gets a mutable reference to the items in the array.
    pub(crate) fn items_mut(&mut self) -> &mut Vec<StackItem> {
        &mut self.items
    }

    /// Returns a stable pointer used for identity tracking.
    pub fn as_ptr(&self) -> *const StackItem {
        self.items.as_ptr()
    }

    /// Gets the item at the specified index.
    pub fn get(&self, index: usize) -> Option<&StackItem> {
        self.items.get(index)
    }

    /// Sets the item at the specified index.
    pub fn set(&mut self, index: usize, item: StackItem) -> VmResult<()> {
        if index >= self.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        self.ensure_mutable()?;

        if let Some(rc) = &self.reference_counter {
            self.validate_compound_reference(rc, &item)?;
            let parent = CompoundParent::Array(self.id);
            rc.remove_compound_reference(&self.items[index], parent);
            rc.add_compound_reference(&item, parent);
        }

        self.items[index] = item;
        Ok(())
    }

    /// Adds an item to the end of the array.
    pub fn push(&mut self, item: StackItem) -> VmResult<()> {
        self.ensure_mutable()?;

        if let Some(rc) = &self.reference_counter {
            self.validate_compound_reference(rc, &item)?;
            rc.add_compound_reference(&item, CompoundParent::Array(self.id));
        }

        self.items.push(item);
        Ok(())
    }

    /// Removes and returns the last item in the array.
    pub fn pop(&mut self) -> VmResult<StackItem> {
        self.ensure_mutable()?;
        let item = self
            .items
            .pop()
            .ok_or_else(|| VmError::invalid_operation_msg("Array is empty"))?;

        if let Some(rc) = &self.reference_counter {
            rc.remove_compound_reference(&item, CompoundParent::Array(self.id));
        }

        Ok(item)
    }

    /// Gets the number of items in the array.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the array is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Removes all items from the array.
    pub fn clear(&mut self) -> VmResult<()> {
        self.ensure_mutable()?;
        if let Some(rc) = &self.reference_counter {
            let parent = CompoundParent::Array(self.id);
            for item in &self.items {
                rc.remove_compound_reference(item, parent);
            }
        }
        self.items.clear();
        Ok(())
    }

    /// Creates a deep copy of the array.
    pub fn deep_copy(&self, reference_counter: Option<ReferenceCounter>) -> Self {
        let items = self.items.iter().map(|item| item.deep_clone()).collect();
        let mut copy = Self::new(items, reference_counter);
        copy.set_read_only(true);
        copy
    }

    /// Gets the type of the stack item.
    pub fn stack_item_type(&self) -> StackItemType {
        StackItemType::Array
    }

    /// Inserts an item at the specified index.
    pub fn insert(&mut self, index: usize, item: StackItem) -> VmResult<()> {
        if index > self.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        self.ensure_mutable()?;

        if let Some(rc) = &self.reference_counter {
            self.validate_compound_reference(rc, &item)?;
            rc.add_compound_reference(&item, CompoundParent::Array(self.id));
        }

        self.items.insert(index, item);
        Ok(())
    }

    /// Removes the item at the specified index.
    pub fn remove(&mut self, index: usize) -> VmResult<StackItem> {
        if index >= self.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        self.ensure_mutable()?;
        let removed = self.items.remove(index);

        if let Some(rc) = &self.reference_counter {
            rc.remove_compound_reference(&removed, CompoundParent::Array(self.id));
        }

        Ok(removed)
    }

    /// Returns an iterator over the items.
    pub fn iter(&self) -> std::slice::Iter<'_, StackItem> {
        self.items.iter()
    }

    /// Returns a mutable iterator over the items.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, StackItem> {
        self.items.iter_mut()
    }

    fn ensure_mutable(&self) -> VmResult<()> {
        if self.is_read_only {
            Err(VmError::invalid_operation_msg(
                "The array is readonly, can not modify.".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn add_reference_for_items(&mut self, rc: &ReferenceCounter) {
        let parent = CompoundParent::Array(self.id);
        for item in &self.items {
            if let Err(err) = self.validate_compound_reference(rc, item) {
                panic!("{err}");
            }
            rc.add_compound_reference(item, parent);
        }
    }

    fn validate_compound_reference(&self, rc: &ReferenceCounter, item: &StackItem) -> VmResult<()> {
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
}

impl Clone for Array {
    fn clone(&self) -> Self {
        let mut clone = Self {
            items: self.items.clone(),
            reference_counter: self.reference_counter.clone(),
            id: next_stack_item_id(),
            is_read_only: self.is_read_only,
        };

        if let Some(rc) = clone.reference_counter.clone() {
            clone.add_reference_for_items(&rc);
            clone.reference_counter = Some(rc);
        }

        clone
    }
}

impl Index<usize> for Array {
    type Output = StackItem;

    fn index(&self, index: usize) -> &Self::Output {
        &self.items[index]
    }
}

impl IndexMut<usize> for Array {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.items[index]
    }
}

impl IntoIterator for Array {
    type Item = StackItem;
    type IntoIter = std::vec::IntoIter<StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a Array {
    type Item = &'a StackItem;
    type IntoIter = std::slice::Iter<'a, StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a mut Array {
    type Item = &'a mut StackItem;
    type IntoIter = std::slice::IterMut<'a, StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter_mut()
    }
}

impl From<Array> for Vec<StackItem> {
    fn from(array: Array) -> Self {
        array.items
    }
}
