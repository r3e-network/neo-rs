// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{vec, vec::Vec};
use core::fmt;

use crate::{stack_item::SharedItem, References};

/// Represents the evaluation stack in the VM.
pub struct EvaluationStack {
    inner_list: Vec<SharedItem>,
    reference_counter: References,
}

impl EvaluationStack {
    pub fn new(reference_counter: References) -> Self {
        Self {
            inner_list: vec![],
            reference_counter,
        }
    }

    /// Gets the number of items on the stack.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner_list.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner_list.is_empty()
    }

    #[inline]
    pub fn references(&self) -> &References {
        &self.reference_counter
    }

    pub fn clear(&mut self) {
        for item in &self.inner_list {
            self.reference_counter.remove(item);
        }
        self.inner_list.clear();
    }

    pub fn copy_to(&self, stack: &mut EvaluationStack, count: i32) {
        if count < -1 || count > self.inner_list.len() as i32 {
            panic!("Argument out of range");
        }
        if count == 0 {
            return;
        }
        if count == -1 || count as usize == self.inner_list.len() {
            stack.inner_list.extend(self.inner_list.iter().cloned());
        } else {
            stack.inner_list.extend(
                self.inner_list[self.inner_list.len() - count as usize..].iter().cloned()
            );
        }
    }

    pub fn insert(&mut self, index: usize, item: SharedItem) {
        if index > self.inner_list.len() {
            panic!("Insert index is out of stack bounds: {}/{}", index, self.inner_list.len());
        }
        self.inner_list.insert(self.inner_list.len() - index, item.clone());
        self.reference_counter.add(&item);
    }

    pub fn move_to(&mut self, stack: &mut EvaluationStack, count: i32) {
        if count == 0 {
            return;
        }
        self.copy_to(stack, count);
        if count == -1 || count as usize == self.inner_list.len() {
            self.inner_list.clear();
        } else {
            let start = self.inner_list.len() - count as usize;
            self.inner_list.truncate(start);
        }
    }

    pub fn peek(&self, index: i32) -> SharedItem {
        let mut idx = index;
        if idx >= self.inner_list.len() as i32 {
            panic!("Peek index is out of stack bounds: {}/{}", idx, self.inner_list.len());
        }
        if idx < 0 {
            idx += self.inner_list.len() as i32;
            if idx < 0 {
                panic!("Peek index is out of stack bounds: {}/{}", idx, self.inner_list.len());
            }
        }
        self.inner_list[self.inner_list.len() - idx as usize - 1].clone()
    }

    pub fn push(&mut self, item: SharedItem) {
        self.inner_list.push(item.clone());
        self.reference_counter.add(&item);
    }

    pub fn reverse(&mut self, n: usize) {
        if n > self.inner_list.len() {
            panic!("Argument out of range");
        }
        if n <= 1 {
            return;
        }
        let start = self.inner_list.len() - n;
        self.inner_list[start..].reverse();
    }

    pub fn pop(&mut self) -> SharedItem {
        let item = self.inner_list.pop().expect("Stack is empty");
        self.reference_counter.remove(&item);
        item
    }
}

impl fmt::Display for EvaluationStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;
        for (i, item) in self.inner_list.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}({})", item.type_(), item)?;
        }
        write!(f, "]")
    }
}

#[cfg(test)]
mod test {
    //
}
