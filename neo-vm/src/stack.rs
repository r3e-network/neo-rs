// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{rc::Rc, vec::Vec};
use crate::{StackItem, References};


// i.e. EvaluationStack
pub struct ExecStack {
    limit: usize,
    items: Vec<Rc<StackItem>>,
    references: Rc<References>,
}


impl ExecStack {
    pub fn new(limit: usize, references: Rc<References>) -> Self {
        Self { limit, items: Vec::new(), references }
    }

    #[inline]
    pub fn len(&self) -> usize { self.items.len() }

    #[inline]
    pub fn push(&mut self, item: Rc<StackItem>) -> bool {
        if self.items.len() >= self.limit {
            return false;
        }

        self.references.add(&item);
        self.items.push(item);
        true
    }

    #[inline]
    pub fn pop(&mut self) -> Option<Rc<StackItem>> {
        self.items.pop()
            .inspect(|x| self.references.remove(x))
    }

    #[inline]
    pub fn top(&self) -> Option<Rc<StackItem>> {
        self.items.last().cloned()
    }
}


#[cfg(test)]
mod test {

    //
}