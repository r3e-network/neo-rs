use alloc::vec::Vec;

use super::StorageFindItem;

#[derive(Debug, Default)]
pub struct StorageIterator {
    items: Vec<StorageFindItem>,
    index: usize,
}

impl StorageIterator {
    pub fn new(items: Vec<StorageFindItem>) -> Self {
        Self { items, index: 0 }
    }

    pub fn next(&mut self) -> Option<StorageFindItem> {
        if self.index >= self.items.len() {
            None
        } else {
            let item = self.items[self.index].clone();
            self.index += 1;
            Some(item)
        }
    }
}
