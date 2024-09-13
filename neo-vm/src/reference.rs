// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::rc::Rc;
use core::hash::{Hash, Hasher};
use core::ops::Deref;
use core::cell::Cell;

use crate::StackItem;


pub struct WrapItem {
    pub(crate) item: Rc<StackItem>,
}

impl WrapItem {
    #[inline]
    pub fn new(item: Rc<StackItem>) -> Self { Self { item } }
}

impl Deref for WrapItem {
    type Target = StackItem;

    fn deref(&self) -> &Self::Target { self.item.deref() }
}

impl PartialEq<Self> for WrapItem {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self, other)
    }
}

impl Eq for WrapItem {}

impl Hash for WrapItem {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.item.as_ref() as *const StackItem as usize).hash(state)
    }
}

// #[derive(Debug, errors::Error)]
// pub enum ReferenceError {
//     TooDepthNestedReference,
// }


pub struct References {
    // tracked: hashbrown::HashSet<WrapItem>,
    // zero_referred: hashbrown::HashSet<WrapItem>,
    references: Cell<usize>,
}


impl References {
    #[inline]
    pub fn new() -> Self {
        Self {
            // tracked: Default::default(),
            // zero_referred: Default::default(),
            references: Cell::new(0),
        }
    }

    // StackItem must add to References before any use
    #[inline]
    pub fn add(&self, item: &Rc<StackItem>) {
        self.recursive_add(item, 1)
    }

    fn recursive_add(&self, item: &Rc<StackItem>, depth: u32) {
        self.references.set(self.references() + 1);
        let stage = item.clone();
        if Rc::strong_count(&stage) <= 2 {
            return;
        }

        match item.as_ref() {
            StackItem::Array(items) => {
                items.iter().for_each(|x| self.recursive_add(x, depth + 1));
            }
            StackItem::Struct(items) => {
                items.iter().for_each(|x| self.recursive_add(x, depth + 1));
            }
            StackItem::Map(items) => {
                // TODO: reference for key
                items.iter().for_each(|(_k, v)| self.recursive_add(v, depth + 1));
            }
            _ => {}
        }
    }

    // StackItem must remove from References before destroy
    #[inline]
    pub fn remove(&self, item: &Rc<StackItem>) {
        self.recursive_remove(item, 1);
    }

    fn recursive_remove(&self, item: &Rc<StackItem>, depth: u32) {
        self.references.set(self.references() - 1);
        if Rc::strong_count(item) > 1 {
            return;
        }

        match item.as_ref() {
            StackItem::Array(items) => {
                items.iter().for_each(|x| self.recursive_remove(x, depth + 1));
            }
            StackItem::Struct(items) => {
                items.iter().for_each(|x| self.recursive_remove(x, depth + 1));
            }
            StackItem::Map(items) => {
                // TODO: reference for key
                items.iter().for_each(|(_k, v)| self.recursive_remove(v, depth + 1));
            }
            _ => {}
        }
    }

    #[inline]
    pub fn references(&self) -> usize {
        self.references.get()
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_references() {
        //
    }
}