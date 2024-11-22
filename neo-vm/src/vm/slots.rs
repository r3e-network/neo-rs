// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{vec, vec::Vec};
use crate::stack_item::{SharedItem, StackItem};

pub(crate) struct Slots {
    items: Vec<SharedItem>,
}

impl Slots {
    pub fn new(slots: usize) -> Self {
        Self { items: vec![StackItem::with_null().into_arc_mutex(); slots] }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<SharedItem> {
        self.items.get(index).cloned()
    }
}

impl core::ops::Index<usize> for Slots {
    type Output = SharedItem;

    fn index(&self, index: usize) -> Self::Output {
        self.items[index].clone()
    }
}

impl core::ops::IndexMut<usize> for Slots {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.items[index]
    }
}
