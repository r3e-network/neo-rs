use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use crate::References;
use crate::stack_item::{SharedItem, StackItem};

pub struct EvaluationStack {
    inner_list:        VecDeque<SharedItem>,
    reference_counter: Rc<RefCell<References>>,
}

impl EvaluationStack {
    pub fn new(reference_counter: Rc<RefCell<References>>) -> Self {
        Self { inner_list: VecDeque::new(), reference_counter }
    }

    pub fn clear(&mut self) {
        for item in &self.inner_list {
            self.reference_counter.borrow_mut().remove(&item);
        }
        self.inner_list.clear();
    }

    pub fn copy_to(&self, stack: &mut EvaluationStack, count: i32) {
        if count < -1 || count as usize > self.inner_list.len() {
            panic!("Argument out of range");
        }
        if count == 0 {
            return;
        }
        if count == -1 || count as usize == self.inner_list.len() {
            stack.inner_list.extend(&self.inner_list);
        } else {
            let start = self.inner_list.len() - count as usize;
            stack.inner_list.extend(&self.inner_list[start..]);
        }
    }

    pub fn insert(&mut self, index: usize, item: SharedItem) {
        if index > self.inner_list.len() {
            panic!("Insert out of bounds");
        }
        self.inner_list.insert(self.inner_list.len() - index, item.clone());
        self.reference_counter.borrow_mut().add(&item);
    }

    pub fn move_to(&mut self, stack: &mut EvaluationStack, count: i32) {
        if count == 0 {
            return;
        }
        self.copy_to(stack, count);
        if count == -1 || count as usize == self.inner_list.len() {
            self.inner_list.clear();
        } else {
            let end = self.inner_list.len() - count as usize;
            self.inner_list.drain(end..);
        }
    }

    pub fn peek(&self, index: i32) -> SharedItem {
        let index = index as isize;
        if index >= self.inner_list.len() as isize {
            panic!("Peek out of bounds");
        }
        if index < 0 {
            let index = self.inner_list.len() as isize + index;
            if index < 0 {
                panic!("Peek out of bounds");
            }
        }
        self.inner_list.get((self.inner_list.len() as isize - index - 1) as usize).unwrap().clone()
    }

    pub fn top(&self) -> SharedItem {
        self.inner_list.back().unwrap().clone()
    }

    pub fn push(&mut self, item: SharedItem) {
        self.inner_list.push_back(item.clone());
        self.reference_counter.borrow_mut().add(&item);  
    }

    pub fn reverse(&mut self, n: i32) {
        let n = n as usize;
        if n < 0 || n > self.inner_list.len() {
            panic!("Argument out of range");
        }
        if n <= 1 {
            return;
        }
        let end = self.inner_list.len() - n;
        self.inner_list.make_contiguous().reverse();
    }

    pub fn pop(&mut self) -> SharedItem {
        self.remove(0)
    }

    pub fn pop_typed(&mut self) -> StackItem {
        self.remove(0)
    }

    pub fn remove(&mut self, index: i32) -> T {
        let index = index as isize;
        if index >= self.inner_list.len() as isize {
            panic!("Argument out of range");
        }
        if index < 0 {
            let index = self.inner_list.len() as isize + index;
            if index < 0 {
                panic!("Argument out of range");
            }
        }
        let index = self.inner_list.len() as isize - index - 1;
        let item = self.inner_list.remove(index as usize).unwrap();
        if !item.borrow().is::<T>() {
            panic!("Invalid cast");
        }
        self.reference_counter.borrow_mut().remove(&item);
        item.try_into().unwrap()
    }

    pub fn size(&self) -> usize {
        self.inner_list.len()
    }
}
