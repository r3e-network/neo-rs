use alloc::vec::Vec;

use neo_vm::VmValue;

#[derive(Debug, Default)]
pub struct StorageIterator {
    items: Vec<VmValue>,
    index: usize,
}

impl StorageIterator {
    pub fn new(items: Vec<VmValue>) -> Self {
        Self { items, index: 0 }
    }

    pub fn next(&mut self) -> Option<VmValue> {
        if self.index >= self.items.len() {
            None
        } else {
            let item = self.items[self.index].clone();
            self.index += 1;
            Some(item)
        }
    }
}
