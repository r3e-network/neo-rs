use alloc::rc::Rc;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use crate::compound_types::compound_trait::CompoundTrait;
use crate::stack_item::{SharedItem, StackItem};

/// Struct is a value type that holds a vector of thread-safe StackItems
#[derive(Default, Clone, Debug)]
pub struct StructItem {
    pub(crate) items: Vec<SharedItem>,
    pub(crate) ref_count:usize,
    pub(crate) read_only:bool,
}

impl CompoundTrait for StructItem {
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

impl StructItem {
    /// Creates a new Struct from an existing vector of StackItems
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
    /// Creates a new Struct with the specified initial size, filled with Null values
    #[inline]
    pub fn new(initial_size: usize) -> Self {
        Self {
            items: vec![Rc::new(RefCell::new(StackItem::Null)); initial_size],
            ref_count: 0,
            read_only: false,
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

    /// Returns the number of items in the struct
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the struct is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns true if the struct is read-only
    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Makes the struct read-only
    #[inline]
    pub fn make_read_only(&mut self) {
        self.read_only = true;
    }

    /// Returns an iterator over the items in the struct
    pub fn iter(&self) -> impl Iterator<Item = &SharedItem> {
        self.items.iter()
    }

    /// Returns a mutable iterator over the items in the struct
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut SharedItem> {
        self.items.iter_mut()
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

impl Eq for StructItem {}

impl PartialEq for StructItem {
    // `eq` only with same reference, and cannot be compared in `neo C#`
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.as_ptr(), other.as_ptr())
    }
}

impl Hash for StructItem {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ptr().hash(state);
    }
}



// fn equals_struct(a: &StructItem, b: &StructItem, limit: &mut usize) -> bool {
//     if a.items().len() != b.items().len() {
//         return false;
//     }

//     let mut comparable_size = MAX_BYTE_ARRAY_COMPARABLE_SIZE;
//     for (item_a, item_b) in a.iter().zip(b.iter()) {
//         *limit -= 1;
//         if *limit == 0 {
//             panic!("Too many elements");
//         }

//         match (item_a, item_b) {
//             (StackItem::ByteArray(ba_a), StackItem::ByteArray(ba_b)) => {
//                 if !Self::equals_byte_array(ba_a, ba_b, &mut comparable_size) {
//                     return false;
//                 }
//             }
//             _ => {
//                 if comparable_size == 0 {
//                     panic!("Too big to compare");
//                 }
//                 comparable_size -= 1;
//                 if !item_a.equals(item_b) {
//                     return false;
//                 }
//             }
//         }
//     }
//     true
// }