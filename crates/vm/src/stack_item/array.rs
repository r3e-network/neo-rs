//! Array stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Array stack item implementation used in the Neo VM.

use crate::error::{VmError, VmResult};
use crate::reference_counter::{CompoundParent, ReferenceCounter};
use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::stack_item_vertex::next_stack_item_id;
use crate::stack_item::StackItem;
use std::cell::{Ref, RefCell, RefMut};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

#[derive(Debug)]
struct ArrayInner {
    items: Vec<StackItem>,
    reference_counter: Option<ReferenceCounter>,
    id: usize,
    is_read_only: bool,
}

impl ArrayInner {
    fn ensure_mutable(&self) -> VmResult<()> {
        if self.is_read_only {
            Err(VmError::invalid_operation_msg(
                "The array is readonly, can not modify.".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn validate_compound_reference(
        &self,
        rc: &ReferenceCounter,
        item: &StackItem,
    ) -> VmResult<()> {
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

    fn add_reference_for_items(&mut self) {
        if let Some(rc) = self.reference_counter.clone() {
            let parent = CompoundParent::Array(self.id);
            for item in &self.items {
                if let Err(err) = self.validate_compound_reference(&rc, item) {
                    panic!("{err}");
                }
                rc.add_compound_reference(item, parent);
            }
        }
    }
}

/// Represents an array of stack items in the VM. This type exhibits reference
/// semantics matching the C# implementation.
#[derive(Debug, Clone)]
pub struct Array {
    inner: Rc<RefCell<ArrayInner>>,
}

/// Read-only view over array items.
pub struct ArrayItemsRef<'a>(Ref<'a, [StackItem]>);

impl<'a> Deref for ArrayItemsRef<'a> {
    type Target = [StackItem];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Mutable view over array items.
pub struct ArrayItemsRefMut<'a>(RefMut<'a, [StackItem]>);

impl<'a> Deref for ArrayItemsRefMut<'a> {
    type Target = [StackItem];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> DerefMut for ArrayItemsRefMut<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Iterator over array items while holding the underlying borrow.
pub struct ArrayIter<'a> {
    inner: Ref<'a, [StackItem]>,
    index: usize,
}

impl<'a> Iterator for ArrayIter<'a> {
    type Item = &'a StackItem;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.get(self.index);
        self.index += 1;
        item
    }
}

impl<'a> IntoIterator for ArrayItemsRef<'a> {
    type Item = &'a StackItem;
    type IntoIter = ArrayIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ArrayIter {
            inner: self.0,
            index: 0,
        }
    }
}

impl Array {
    /// Creates a new array with the specified items.
    pub fn new(items: Vec<StackItem>, reference_counter: Option<ReferenceCounter>) -> Self {
        let inner = Rc::new(RefCell::new(ArrayInner {
            items,
            reference_counter,
            id: next_stack_item_id(),
            is_read_only: false,
        }));

        inner.borrow_mut().add_reference_for_items();

        Self { inner }
    }

    /// Returns the reference counter associated with this array, if any.
    pub fn reference_counter(&self) -> Option<ReferenceCounter> {
        self.inner.borrow().reference_counter.clone()
    }

    /// Returns the unique identifier for this array.
    pub fn id(&self) -> usize {
        self.inner.borrow().id
    }

    /// Returns whether the array is marked as read-only.
    pub fn is_read_only(&self) -> bool {
        self.inner.borrow().is_read_only
    }

    /// Sets the read-only flag.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.inner.borrow_mut().is_read_only = read_only;
    }

    /// Gets the items in the array.
    pub fn items(&self) -> ArrayItemsRef<'_> {
        ArrayItemsRef(Ref::map(self.inner.borrow(), |inner| inner.items.as_slice()))
    }

    /// Gets a mutable reference to the items in the array.
    pub fn items_mut(&mut self) -> ArrayItemsRefMut<'_> {
        ArrayItemsRefMut(RefMut::map(self.inner.borrow_mut(), |inner| {
            inner.items.as_mut_slice()
        }))
    }

    /// Returns an iterator over the items.
    pub fn iter(&self) -> ArrayIter<'_> {
        ArrayIter {
            inner: Ref::map(self.inner.borrow(), |inner| inner.items.as_slice()),
            index: 0,
        }
    }

    /// Returns a stable pointer used for identity tracking.
    pub fn as_ptr(&self) -> *const StackItem {
        self.inner.borrow().items.as_ptr()
    }

    /// Gets the item at the specified index, cloning it from the array.
    pub fn get(&self, index: usize) -> Option<StackItem> {
        self.inner.borrow().items.get(index).cloned()
    }

    /// Sets the item at the specified index.
    pub fn set(&mut self, index: usize, item: StackItem) -> VmResult<()> {
        let mut inner = self.inner.borrow_mut();
        if index >= inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        inner.ensure_mutable()?;

        if let Some(rc) = inner.reference_counter.clone() {
            inner.validate_compound_reference(&rc, &item)?;
            let parent = CompoundParent::Array(inner.id);
            rc.remove_compound_reference(&inner.items[index], parent);
            rc.add_compound_reference(&item, parent);
        }

        inner.items[index] = item;
        Ok(())
    }

    /// Adds an item to the end of the array.
    pub fn push(&mut self, item: StackItem) -> VmResult<()> {
        let mut inner = self.inner.borrow_mut();
        inner.ensure_mutable()?;

        if let Some(rc) = inner.reference_counter.clone() {
            inner.validate_compound_reference(&rc, &item)?;
            rc.add_compound_reference(&item, CompoundParent::Array(inner.id));
        }

        inner.items.push(item);
        Ok(())
    }

    /// Removes and returns the last item in the array.
    pub fn pop(&mut self) -> VmResult<StackItem> {
        let mut inner = self.inner.borrow_mut();
        inner.ensure_mutable()?;
        let item = inner
            .items
            .pop()
            .ok_or_else(|| VmError::invalid_operation_msg("Array is empty"))?;

        if let Some(rc) = inner.reference_counter.clone() {
            rc.remove_compound_reference(&item, CompoundParent::Array(inner.id));
        }

        Ok(item)
    }

    /// Gets the number of items in the array.
    pub fn len(&self) -> usize {
        self.inner.borrow().items.len()
    }

    /// Returns true if the array is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.borrow().items.is_empty()
    }

    /// Removes all items from the array.
    pub fn clear(&mut self) -> VmResult<()> {
        let mut inner = self.inner.borrow_mut();
        inner.ensure_mutable()?;
        if let Some(rc) = inner.reference_counter.clone() {
            let parent = CompoundParent::Array(inner.id);
            for item in &inner.items {
                rc.remove_compound_reference(item, parent);
            }
        }
        inner.items.clear();
        Ok(())
    }

    /// Creates a deep copy of the array.
    pub fn deep_copy(&self, reference_counter: Option<ReferenceCounter>) -> Self {
        let items = self
            .inner
            .borrow()
            .items
            .iter()
            .map(|item| item.deep_clone())
            .collect();
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
        let mut inner = self.inner.borrow_mut();
        if index > inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        inner.ensure_mutable()?;

        if let Some(rc) = inner.reference_counter.clone() {
            inner.validate_compound_reference(&rc, &item)?;
            rc.add_compound_reference(&item, CompoundParent::Array(inner.id));
        }

        inner.items.insert(index, item);
        Ok(())
    }

    /// Removes the item at the specified index.
    pub fn remove(&mut self, index: usize) -> VmResult<StackItem> {
        let mut inner = self.inner.borrow_mut();
        if index >= inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        inner.ensure_mutable()?;
        let removed = inner.items.remove(index);

        if let Some(rc) = inner.reference_counter.clone() {
            rc.remove_compound_reference(&removed, CompoundParent::Array(inner.id));
        }

        Ok(removed)
    }

    /// Consumes the array and returns the underlying items.
    pub fn into_vec(self) -> Vec<StackItem> {
        Rc::try_unwrap(self.inner)
            .map(|cell| cell.into_inner().items)
            .unwrap_or_else(|rc| rc.borrow().items.clone())
    }
}

impl From<Array> for Vec<StackItem> {
    fn from(array: Array) -> Self {
        array.into_vec()
    }
}

impl IntoIterator for Array {
    type Item = StackItem;
    type IntoIter = std::vec::IntoIter<StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a Array {
    type Item = &'a StackItem;
    type IntoIter = ArrayIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_traits::ToPrimitive;

    #[test]
    fn test_array_creation() {
        let items = vec![
            StackItem::from_int(1),
            StackItem::from_int(2),
            StackItem::from_int(3),
        ];

        let array = Array::new(items.clone(), None);

        assert_eq!(array.len(), 3);
        assert_eq!(array.items().to_vec(), items);
        assert_eq!(array.stack_item_type(), StackItemType::Array);
    }

    #[test]
    fn test_array_get() -> Result<(), Box<dyn std::error::Error>> {
        let items = vec![
            StackItem::from_int(1),
            StackItem::from_int(2),
            StackItem::from_int(3),
        ];

        let array = Array::new(items, None);

        assert_eq!(array.get(0).unwrap().as_int().unwrap().to_i32().unwrap(), 1);
        assert_eq!(array.get(1).unwrap().as_int().unwrap().to_i32().unwrap(), 2);
        assert_eq!(array.get(2).unwrap().as_int().unwrap().to_i32().unwrap(), 3);
        assert!(array.get(3).is_none());
        Ok(())
    }

    #[test]
    fn test_array_set() -> Result<(), Box<dyn std::error::Error>> {
        let items = vec![
            StackItem::from_int(1),
            StackItem::from_int(2),
            StackItem::from_int(3),
        ];

        let mut array = Array::new(items, None);

        array.set(1, StackItem::from_int(42)).unwrap();

        assert_eq!(array.get(0).unwrap().as_int().unwrap().to_i32().unwrap(), 1);
        assert_eq!(
            array.get(1).unwrap().as_int().unwrap().to_i32().unwrap(),
            42
        );
        assert_eq!(array.get(2).unwrap().as_int().unwrap().to_i32().unwrap(), 3);

        // Test setting out of bounds - should produce an error
        assert!(array.set(3, StackItem::from_int(4)).is_err());
        Ok(())
    }

    #[test]
    fn test_array_push_pop() -> Result<(), Box<dyn std::error::Error>> {
        let items = vec![StackItem::from_int(1), StackItem::from_int(2)];

        let mut array = Array::new(items, None);

        array.push(StackItem::from_int(3)).unwrap();

        assert_eq!(array.len(), 3);
        assert_eq!(array.get(2).unwrap().as_int().unwrap().to_i32().unwrap(), 3);

        let popped = array.pop().unwrap();

        assert_eq!(array.len(), 2);
        assert_eq!(
            popped
                .as_int()
                .expect("intermediate value should exist")
                .to_i32()
                .unwrap(),
            3
        );
        Ok(())
    }

    #[test]
    fn test_array_clear() {
        let items = vec![
            StackItem::from_int(1),
            StackItem::from_int(2),
            StackItem::from_int(3),
        ];

        let mut array = Array::new(items, None);

        array.clear().unwrap();

        assert_eq!(array.len(), 0);
        assert!(array.is_empty());
    }

    #[test]
    fn test_array_deep_copy() -> Result<(), Box<dyn std::error::Error>> {
        let items = vec![
            StackItem::from_int(1),
            StackItem::from_int(2),
            StackItem::from_array(vec![StackItem::from_int(3), StackItem::from_int(4)]),
        ];

        let array = Array::new(items, None);
        let copied = array.deep_copy(None);

        assert_eq!(copied.len(), array.len());
        assert_eq!(
            copied.get(0).unwrap().as_int().unwrap(),
            array.get(0).unwrap().as_int().unwrap()
        );
        let nested_original = array.get(2).unwrap().as_array().unwrap();
        let nested_copied = copied.get(2).unwrap().as_array().unwrap();
        assert_eq!(nested_original.len(), nested_copied.len());
        Ok(())
    }

    #[test]
    fn test_array_insert_remove() -> Result<(), Box<dyn std::error::Error>> {
        let items = vec![StackItem::from_int(1), StackItem::from_int(3)];
        let mut array = Array::new(items, None);

        array.insert(1, StackItem::from_int(2)).unwrap();
        assert_eq!(array.len(), 3);
        assert_eq!(array.get(1).unwrap().as_int().unwrap().to_i32().unwrap(), 2);

        let removed = array.remove(1).unwrap();
        assert_eq!(removed.as_int().unwrap().to_i32().unwrap(), 2);
        assert_eq!(array.len(), 2);
        Ok(())
    }
}
