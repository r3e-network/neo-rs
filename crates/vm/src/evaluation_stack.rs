//! Evaluation stack module for the Neo Virtual Machine.
//!
//! This module represents a stack used by the Neo VM for executing scripts.

use crate::error::{VmError, VmResult};
use crate::reference_counter::ReferenceCounter;
use crate::stack_item::StackItem;

/// Represents the evaluation stack in the VM.
#[derive(Clone)]
pub struct EvaluationStack {
    /// The underlying storage for the stack. The top of the stack is the
    /// element at the end of the vector, matching the C# implementation.
    stack: Vec<StackItem>,

    /// Reference counter responsible for tracking stack references.
    reference_counter: ReferenceCounter,
}

impl EvaluationStack {
    /// Creates a new evaluation stack with the specified reference counter.
    pub fn new(reference_counter: ReferenceCounter) -> Self {
        Self {
            stack: Vec::with_capacity(32), // Pre-allocate for typical stack usage
            reference_counter,
        }
    }

    /// Returns the reference counter for this evaluation stack.
    pub fn reference_counter(&self) -> &ReferenceCounter {
        &self.reference_counter
    }

    /// Returns the number of items on the stack.
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Indicates whether the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Pushes an item onto the top of the stack.
    pub fn push(&mut self, item: StackItem) {
        self.reference_counter.add_stack_reference(&item, 1);
        self.stack.push(item);
    }

    /// Removes and returns the item at the top of the stack.
    pub fn pop(&mut self) -> VmResult<StackItem> {
        self.remove_internal(0)
    }

    /// Returns the item at the specified index counting from the top of the
    /// stack (0-based) without removing it.
    pub fn peek(&self, index_from_top: usize) -> VmResult<&StackItem> {
        let idx = self.resolve_top_index(index_from_top)?;
        Ok(&self.stack[idx])
    }

    /// Mutable version of [`peek`].
    pub fn peek_mut(&mut self, index_from_top: usize) -> VmResult<&mut StackItem> {
        let idx = self.resolve_top_index(index_from_top)?;
        Ok(&mut self.stack[idx])
    }

    /// Inserts an item at the specified index counting from the top of the
    /// stack (0-based). Passing `0` is equivalent to `push`, while passing
    /// `len()` inserts the item at the bottom of the stack.
    pub fn insert(&mut self, index_from_top: usize, item: StackItem) -> VmResult<()> {
        if index_from_top > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Insert index out of range"));
        }

        self.reference_counter.add_stack_reference(&item, 1);
        let insert_pos = self.stack.len().saturating_sub(index_from_top);
        self.stack.insert(insert_pos, item);
        Ok(())
    }

    /// Swaps the items located at the supplied top-based indices.
    pub fn swap(&mut self, index_a: usize, index_b: usize) -> VmResult<()> {
        if index_a >= self.stack.len() || index_b >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        if index_a == index_b {
            return Ok(());
        }

        let a = self.resolve_top_index(index_a)?;
        let b = self.resolve_top_index(index_b)?;
        self.stack.swap(a, b);
        Ok(())
    }

    /// Removes and returns the item at the specified index counting from the
    /// top of the stack (0-based).
    pub fn remove(&mut self, index_from_top: usize) -> VmResult<StackItem> {
        self.remove_internal(index_from_top)
    }

    /// Reverses the order of the `count` items at the top of the stack.
    pub fn reverse(&mut self, count: usize) -> VmResult<()> {
        if count > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Reverse count out of range"));
        }
        if count <= 1 {
            return Ok(());
        }

        let start = self.stack.len() - count;
        self.stack[start..].reverse();
        Ok(())
    }

    /// Copies `count` items (default: all) from the top of this stack to the
    /// target stack without removing them from the source stack.
    pub fn copy_to(&self, target: &mut EvaluationStack, count: Option<usize>) -> VmResult<()> {
        let count = count.unwrap_or(self.stack.len());
        if count > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Copy count out of range"));
        }
        if count == 0 {
            return Ok(());
        }

        let start = self.stack.len() - count;
        for item in &self.stack[start..] {
            target.reference_counter.add_stack_reference(item, 1);
            target.stack.push(item.clone());
        }
        Ok(())
    }

    /// Moves `count` items (default: all) from the top of this stack to the
    /// target stack.
    pub fn move_to(&mut self, target: &mut EvaluationStack, count: Option<usize>) -> VmResult<()> {
        let count = count.unwrap_or(self.stack.len());
        if count > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Move count out of range"));
        }
        if count == 0 {
            return Ok(());
        }

        let start = self.stack.len() - count;

        // Transfer ownership of the tail slice to the target stack.
        let mut moved = self.stack.split_off(start);
        for item in &moved {
            self.reference_counter.remove_stack_reference(item);
            target.reference_counter.add_stack_reference(item, 1);
        }
        target.stack.append(&mut moved);
        Ok(())
    }

    /// Clears the stack, removing all elements and releasing their references.
    pub fn clear(&mut self) {
        for item in &self.stack {
            self.reference_counter.remove_stack_reference(item);
        }
        self.stack.clear();
    }

    /// Iterates over the stack items from bottom to top.
    pub fn iter(&self) -> std::slice::Iter<StackItem> {
        self.stack.iter()
    }

    /// Mutable iterator over the stack items from bottom to top.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<StackItem> {
        self.stack.iter_mut()
    }

    fn resolve_top_index(&self, index_from_top: usize) -> VmResult<usize> {
        if index_from_top >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        Ok(self.stack.len() - index_from_top - 1)
    }

    fn remove_internal(&mut self, index_from_top: usize) -> VmResult<StackItem> {
        let idx = self.resolve_top_index(index_from_top)?;
        let item = self.stack.remove(idx);
        self.reference_counter.remove_stack_reference(&item);
        Ok(item)
    }
}

impl Drop for EvaluationStack {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_push_pop() {
        let reference_counter = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(reference_counter);

        // Push some items
        stack.push(StackItem::from_int(1));
        stack.push(StackItem::from_int(2));
        stack.push(StackItem::from_int(3));

        // Check stack size
        assert_eq!(stack.len(), 3);

        // Pop an item
        let item = stack.pop().expect("pop should succeed");
        assert_eq!(
            item.as_int().expect("as_int should succeed"),
            num_bigint::BigInt::from(3)
        );

        // Check updated stack size
        assert_eq!(stack.len(), 2);
    }

    #[test]
    fn test_peek() {
        let reference_counter = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(reference_counter);

        // Push some items
        stack.push(StackItem::from_int(1));
        stack.push(StackItem::from_int(2));
        stack.push(StackItem::from_int(3));

        // Peek at items
        let item0 = stack.peek(0).expect("peek should succeed");
        let item1 = stack.peek(1).expect("peek should succeed");
        let item2 = stack.peek(2).expect("peek should succeed");

        assert_eq!(
            item0.as_int().expect("as_int should succeed"),
            num_bigint::BigInt::from(3)
        );
        assert_eq!(
            item1.as_int().expect("as_int should succeed"),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            item2.as_int().expect("as_int should succeed"),
            num_bigint::BigInt::from(1)
        );

        assert_eq!(stack.len(), 3);
    }

    #[test]
    fn test_insert_remove() -> Result<(), Box<dyn std::error::Error>> {
        let reference_counter = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(reference_counter);

        // Push some items
        stack.push(StackItem::from_int(1));
        stack.push(StackItem::from_int(3));

        // Insert an item
        stack
            .insert(1, StackItem::from_int(2))
            .expect("insert should succeed");

        // Check stack
        assert_eq!(
            stack
                .peek(2)
                .expect("intermediate value should exist")
                .as_int()
                .expect("as_int should succeed"),
            num_bigint::BigInt::from(1)
        );
        assert_eq!(
            stack
                .peek(1)
                .expect("intermediate value should exist")
                .as_int()
                .expect("as_int should succeed"),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            stack
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()
                .expect("as_int should succeed"),
            num_bigint::BigInt::from(3)
        );

        // Remove an item
        let item = stack.remove(1)?;
        assert_eq!(item.as_int()?, num_bigint::BigInt::from(2));

        // Check stack
        assert_eq!(
            stack
                .peek(1)
                .expect("intermediate value should exist")
                .as_int()?,
            num_bigint::BigInt::from(1)
        );
        assert_eq!(
            stack
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()?,
            num_bigint::BigInt::from(3)
        );
        Ok(())
    }

    #[test]
    fn test_swap() {
        let reference_counter = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(reference_counter);

        // Push some items
        stack.push(StackItem::from_int(1));
        stack.push(StackItem::from_int(2));
        stack.push(StackItem::from_int(3));

        // Swap items
        stack.swap(0, 2).expect("swap should succeed");

        // Check stack
        assert_eq!(
            stack
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()
                .expect("as_int should succeed"),
            num_bigint::BigInt::from(1)
        );
        assert_eq!(
            stack
                .peek(1)
                .expect("intermediate value should exist")
                .as_int()
                .expect("as_int should succeed"),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            stack
                .peek(2)
                .expect("intermediate value should exist")
                .as_int()
                .expect("as_int should succeed"),
            num_bigint::BigInt::from(3)
        );
    }

    #[test]
    fn test_reverse() {
        let reference_counter = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(reference_counter);

        // Push some items
        stack.push(StackItem::from_int(1));
        stack.push(StackItem::from_int(2));
        stack.push(StackItem::from_int(3));
        stack.push(StackItem::from_int(4));
        stack.push(StackItem::from_int(5));

        // Reverse the top 3 items
        stack.reverse(3).expect("reverse should succeed");

        // Check stack
        assert_eq!(
            stack
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()
                .expect("as_int should succeed"),
            num_bigint::BigInt::from(3)
        );
        assert_eq!(
            stack
                .peek(1)
                .unwrap()
                .as_int()
                .expect("as_int should succeed"),
            num_bigint::BigInt::from(4)
        );
        assert_eq!(
            stack.peek(2).unwrap().as_int().unwrap(),
            num_bigint::BigInt::from(5)
        );
        assert_eq!(
            stack.peek(3).unwrap().as_int().unwrap(),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            stack.peek(4).unwrap().as_int().unwrap(),
            num_bigint::BigInt::from(1)
        );

        // Reverse all items
        stack.reverse(5).unwrap();

        // Check stack
        assert_eq!(
            stack.peek(0).unwrap().as_int().unwrap(),
            num_bigint::BigInt::from(1)
        );
        assert_eq!(
            stack.peek(1).unwrap().as_int().expect("Operation failed"),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            stack.peek(2).unwrap().as_int().expect("Operation failed"),
            num_bigint::BigInt::from(5)
        );
        assert_eq!(
            stack.peek(3).unwrap().as_int().expect("Operation failed"),
            num_bigint::BigInt::from(4)
        );
        assert_eq!(
            stack.peek(4).unwrap().as_int().expect("Operation failed"),
            num_bigint::BigInt::from(3)
        );

        stack.reverse(0).expect("Operation failed");

        assert_eq!(
            stack.peek(0).unwrap().as_int().expect("Operation failed"),
            num_bigint::BigInt::from(1)
        );
        assert_eq!(
            stack.peek(1).unwrap().as_int().expect("Operation failed"),
            num_bigint::BigInt::from(2)
        );

        stack.reverse(1).expect("Operation failed");

        assert_eq!(
            stack.peek(0).unwrap().as_int().expect("Operation failed"),
            num_bigint::BigInt::from(1)
        );

        // Try to reverse more items than on the stack
        assert!(stack.reverse(10).is_err());
    }

    #[test]
    fn test_clear() {
        let reference_counter = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(reference_counter);

        // Push some items
        stack.push(StackItem::from_int(1));
        stack.push(StackItem::from_int(2));
        stack.push(StackItem::from_int(3));

        // Clear the stack
        stack.clear();

        // Check stack
        assert_eq!(stack.len(), 0);
        assert!(stack.is_empty());
    }

    #[test]
    fn test_copy_to() {
        let reference_counter1 = ReferenceCounter::new();
        let reference_counter2 = ReferenceCounter::new();
        let mut stack1 = EvaluationStack::new(reference_counter1);
        let mut stack2 = EvaluationStack::new(reference_counter2);

        // Push some items
        stack1.push(StackItem::from_int(1));
        stack1.push(StackItem::from_int(2));
        stack1.push(StackItem::from_int(3));

        // Copy to another stack
        stack1
            .copy_to(&mut stack2, None)
            .expect("copy_to should succeed");

        // Check stacks
        assert_eq!(stack1.len(), 3);
        assert_eq!(stack2.len(), 3);

        assert_eq!(
            stack1.peek(0).unwrap().as_int().unwrap(),
            num_bigint::BigInt::from(3)
        );
        assert_eq!(
            stack2.peek(0).unwrap().as_int().unwrap(),
            num_bigint::BigInt::from(3)
        );
    }
}
