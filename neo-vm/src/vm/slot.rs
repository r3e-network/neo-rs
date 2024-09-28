use alloc::rc::Rc;
use std::cell::RefCell;

use crate::{References, StackItem};

#[derive(Debug, Clone, Default)]
pub struct Slot {
    items: Vec<Rc<RefCell<StackItem>>>,
    reference_counter: Rc<RefCell<References>>,
}

impl Slot {
    pub fn new(
        items: Vec<Rc<RefCell<StackItem>>>,
        reference_counter: Rc<RefCell<References>>,
    ) -> Self {
        let mut slot = Self { items, reference_counter };
        for item in &slot.items {
            slot.reference_counter.borrow_mut().add_stack_reference(item.clone(), 1);
        }
        slot
    }

    pub fn new_with_count(count: i32, reference_counter: Rc<RefCell<References>>) -> Self {
        let mut items = Vec::new();
        for _ in 0..count {
            items.push(StackItemTrait::from(Null::default()).into());
        }

        Self { items, reference_counter }
    }

    pub fn with_capacity(capacity: usize, reference_counter: Rc<RefCell<References>>) -> Self {
        Self { items: Vec::with_capacity(capacity), reference_counter }
    }

    pub fn get(&self, index: usize) -> Rc<RefCell<StackItem>> {
        self.items[index].clone()
    }

    pub fn set(&mut self, index: usize, value: Rc<RefCell<StackItem>>) {
        let old_value = std::mem::replace(&mut self.items[index], value.clone());
        self.reference_counter.borrow_mut().remove_stack_reference(old_value);
        self.reference_counter.borrow_mut().add_stack_reference(value, 1);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn clear_references(&mut self) {
        for item in &self.items {
            self.reference_counter.get_mut().remove_stack_reference(item.clone());
        }
    }
}

impl IntoIterator for Slot {
    type Item = Rc<RefCell<StackItem>>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
