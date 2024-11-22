use alloc::rc::Rc;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use crate::compound_types::compound_trait::CompoundTrait;
use crate::stack_item::{SharedItem, StackItem};

/// Array is a reference type that holds a vector of thread-safe StackItems
#[derive(Default, Clone, Debug)]
pub struct ArrayItem {
    items: Vec<SharedItem>,
    ref_count:usize,
    read_only:bool,
}
impl CompoundTrait for ArrayItem {
    fn ref_count(&self) -> usize {
        self.ref_count
    }

    fn ref_inc(&mut self, count:usize) -> usize {
        self.ref_count += count;
        self.ref_count
    }

    fn ref_dec(&mut self, count:usize) -> usize {
        self.ref_count -= count;
        self.ref_count
    }

    fn sub_items(&self) -> Vec<SharedItem> {
        self.items.clone()
    }

    fn read_only(&mut self) {
        self.read_only = true;
    }

    fn clear(&mut self) {
        self.items.clear();
    }
}

impl ArrayItem {
    /// Creates a new Array with the specified initial size, filled with Null values
    #[inline]
    pub fn new(initial_size: usize) -> Self {
        Self {
            items: vec![Rc::new(RefCell::new(StackItem::Null)); initial_size],
            ref_count: 0,
            read_only: false,
        }
    }

    /// Creates a new Array from an existing vector of StackItems
    #[inline]
    pub fn from_vec(items: Vec<StackItem>) -> Self {
        Self {
            items: items.into_iter()
                .map(|item| Rc::new(RefCell::new(item)))
                .collect(),
            ref_count: 0,
            read_only: false,
        }
    }

    /// Adds a new item to the end of the array
    pub fn add(&mut self, item: SharedItem) {
        if !self.read_only {
            self.items.push(item);
        }
    }

    /// Gets the item at the specified index
    pub fn get(&self, index: usize) -> Option<SharedItem> {
        self.items.get(index).cloned()
    }

    /// Sets the item at the specified index
    pub fn set(&mut self, index: usize, item: SharedItem) -> bool {
        if self.read_only {
            return false;
        }
        if let Some(existing) = self.items.get_mut(index) {
            *existing = item;
            true
        } else {
            false
        }
    }
    /// Returns the number of items in the array
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the array is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Removes all items from the array
    pub fn clear(&mut self) {
        if !self.read_only {
            self.items.clear();
        }
    }

    /// Removes and returns the item at the specified index
    pub fn remove(&mut self, index: usize) -> Option<SharedItem> {
        if !self.read_only {
            if index < self.items.len() {
                Some(self.items.remove(index))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Inserts an item at the specified index
    pub fn insert(&mut self, index: usize, item: SharedItem) {
        if !self.read_only {
            self.items.insert(index, item);
        }
    }

    /// Returns an iterator over the items in the array
    pub fn iter(&self) -> impl Iterator<Item = &SharedItem> {
        self.items.iter()
    }

    /// Returns a mutable iterator over the items in the array
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut SharedItem> {
        self.items.iter_mut()
    }

    /// Resizes the array to the specified length, filling new slots with Null
    pub fn resize(&mut self, new_len: usize) {
        if !self.read_only {
            self.items.resize(new_len, Rc::new(RefCell::new(StackItem::Null)));
        }
    }

    /// Truncates the array to the specified length
    pub fn truncate(&mut self, len: usize) {
        if !self.read_only {
            self.items.truncate(len);
        }
    }

    /// Returns true if the array is read-only
    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Makes the array read-only
    #[inline]
    pub fn make_read_only(&mut self) {
        self.read_only = true;
    }

    /// Returns a reference to the vector of items
    #[inline]
    pub fn items(&self) -> &Vec<SharedItem> {
        &self.items
    }

    /// Returns a mutable reference to the vector of items
    #[inline]
    pub fn items_mut(&mut self) -> &mut Vec<SharedItem> {
        &mut self.items
    }

    /// Returns the raw pointer to the first element
    /// This should only be used for comparison operations
    #[inline]
    pub(crate) fn as_ptr(&self) -> *const SharedItem {
        self.items.as_ptr()
    }
}

impl Eq for ArrayItem {}

impl PartialEq for ArrayItem {
    /// Arrays are equal only if they point to the same underlying vector
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.as_ptr(), other.as_ptr())
    }
}

impl Hash for ArrayItem {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        StackItem::Array(self.clone()).get_type().hash(state);
        
        for item in self.sub_items() {
            match item.borrow().clone() {
                StackItem::Array(a) | StackItem::Struct(a) | StackItem::Map(a) => {
                    a.ref_count().hash(state);
                    StackItem::Array(a).get_type().hash(state);
                },
                other => {
                    other.hash(state);
                }
            }
        }
    }
}