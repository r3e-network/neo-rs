// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{vec, vec::Vec};

use crate::StackItem;

pub(crate) struct Slots {
    items: Vec<StackItem>,
}

impl Slots {
    pub fn new(slots: usize) -> Self { Self { items: vec![StackItem::with_null(); slots] } }

    #[inline]
    pub fn len(&self) -> usize { self.items.len() }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&StackItem> { self.items.get(index) }
}

impl core::ops::Index<usize> for Slots {
    type Output = StackItem;

    fn index(&self, index: usize) -> &Self::Output { &self.items[index] }
}

impl core::ops::IndexMut<usize> for Slots {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output { &mut self.items[index] }
}
