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
    #[inline]
    #[must_use]
    pub fn new(reference_counter: ReferenceCounter) -> Self {
        Self {
            stack: Vec::with_capacity(32), // Pre-allocate for typical stack usage
            reference_counter,
        }
    }

    /// Creates an evaluation stack that uses a recycled backing allocation.
    ///
    /// `buffer` must be empty; a non-empty buffer would carry entries that the
    /// reference counter has never seen. It is cleared defensively so the
    /// invariant cannot be violated by a caller.
    #[inline]
    #[must_use]
    pub(crate) fn with_backing_buffer(
        reference_counter: ReferenceCounter,
        mut buffer: Vec<StackItem>,
    ) -> Self {
        buffer.clear();
        Self {
            stack: buffer,
            reference_counter,
        }
    }

    /// Detaches and returns the backing allocation, leaving the stack with no
    /// storage at all.
    ///
    /// This is only sound for a stack that is about to disappear: the entries
    /// move out with the vector and are released when the caller drops it,
    /// which is exactly what dropping the whole stack would have done. The
    /// reference counter is deliberately not touched, so an unloaded context
    /// keeps the same reference accounting as before.
    #[inline]
    pub(crate) fn take_backing_buffer(&mut self) -> Vec<StackItem> {
        std::mem::take(&mut self.stack)
    }

    /// Returns the allocated element capacity of the backing buffer.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.stack.capacity()
    }

    /// Returns the reference counter for this evaluation stack.
    #[inline]
    #[must_use]
    pub const fn reference_counter(&self) -> &ReferenceCounter {
        &self.reference_counter
    }

    /// Returns the number of items on the stack.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Indicates whether the stack is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Pushes an item onto the top of the stack.
    #[inline(always)]
    pub fn push(&mut self, item: StackItem) -> VmResult<()> {
        // neo-vm v3.10.1 `EvaluationStack.Push`: add to the list, then
        // `AddStackReference(item)`.
        self.stack.push(item);
        if let Some(item) = self.stack.last() {
            self.reference_counter.add_stack_reference(item, 1);
        }
        Ok(())
    }

    /// Removes and returns the item at the top of the stack.
    #[inline(always)]
    pub fn pop(&mut self) -> VmResult<StackItem> {
        self.remove_internal(0)
    }

    /// Removes and returns the item at the top of the stack without releasing
    /// its stack reference.
    ///
    /// neo-vm v3.11.2 `EvaluationStack.PopNoRef`. The caller becomes
    /// responsible for the reference the removed item held.
    #[inline(always)]
    pub fn pop_no_ref(&mut self) -> VmResult<StackItem> {
        self.remove_no_ref(0)
    }

    /// Pushes an item and adds exactly `count` to the reference counter.
    ///
    /// neo-vm v3.11.2 `EvaluationStack.PushItemCounted`: the item's own
    /// `StackReferences` is *not* adjusted, so the caller must already have
    /// accounted for it.
    #[inline(always)]
    pub fn push_item_counted(&mut self, item: StackItem, count: i64) {
        self.stack.push(item);
        self.reference_counter.inc(count);
    }

    /// Moves the item `index` places below the top to the top of the stack.
    ///
    /// neo-vm v3.11.2 `EvaluationStack.Roll`. Nothing is popped or pushed, so
    /// no reference is released or added.
    #[inline(always)]
    pub fn roll(&mut self, index: usize) -> VmResult<()> {
        if index >= self.stack.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Out of stack bounds: {index}/{}",
                self.stack.len()
            )));
        }
        if index == 0 {
            return Ok(());
        }
        let position = self.stack.len() - 1 - index;
        let item = self.stack.remove(position);
        self.stack.push(item);
        Ok(())
    }

    /// Returns the item at the specified index counting from the top of the
    /// stack (0-based) without removing it.
    #[inline(always)]
    pub fn peek(&self, index_from_top: usize) -> VmResult<&StackItem> {
        // Fast path: bounds check and index calculation
        if index_from_top >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        // SAFETY: We just verified the index is within bounds
        unsafe {
            Ok(self
                .stack
                .get_unchecked(self.stack.len() - index_from_top - 1))
        }
    }

    /// Mutable version of [`Self::peek`].
    #[inline(always)]
    pub fn peek_mut(&mut self, index_from_top: usize) -> VmResult<&mut StackItem> {
        // Fast path: bounds check and index calculation
        if index_from_top >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        let idx = self.stack.len() - index_from_top - 1;
        // SAFETY: We just verified the index is within bounds
        unsafe { Ok(self.stack.get_unchecked_mut(idx)) }
    }

    /// Inserts an item at the specified index counting from the top of the
    /// stack (0-based). Passing `0` is equivalent to `push`, while passing
    /// `len()` inserts the item at the bottom of the stack.
    pub fn insert(&mut self, index_from_top: usize, item: StackItem) -> VmResult<()> {
        if index_from_top > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Insert index out of range"));
        }

        // neo-vm v3.10.1 `EvaluationStack.Insert`: insert into the list, then
        // `AddStackReference(item)`.
        let insert_pos = self.stack.len().saturating_sub(index_from_top);
        self.stack.insert(insert_pos, item);
        self.reference_counter
            .add_stack_reference(&self.stack[insert_pos], 1);
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
    ///
    /// As in neo-vm v3.10.1 `EvaluationStack.CopyTo`, the reference counter is
    /// not involved: the copied entries carry no stack reference of their own.
    pub fn copy_to(&self, target: &mut Self, count: Option<usize>) -> VmResult<()> {
        let count = count.unwrap_or(self.stack.len());
        if count > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Copy count out of range"));
        }
        if count == 0 {
            return Ok(());
        }

        let start = self.stack.len() - count;
        target.stack.extend_from_slice(&self.stack[start..]);
        Ok(())
    }

    /// Moves `count` items (default: all) from the top of this stack to the
    /// target stack.
    ///
    /// As in neo-vm v3.10.1 `EvaluationStack.MoveTo`, the reference counter is
    /// not involved: the stack references move with the entries.
    pub fn move_to(&mut self, target: &mut Self, count: Option<usize>) -> VmResult<()> {
        let count = count.unwrap_or(self.stack.len());
        if count > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Move count out of range"));
        }
        if count == 0 {
            return Ok(());
        }

        let start = self.stack.len() - count;
        target.stack.extend(self.stack.drain(start..));
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
    pub fn iter(&self) -> std::slice::Iter<'_, StackItem> {
        self.stack.iter()
    }

    /// Mutable iterator over the stack items from bottom to top.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, StackItem> {
        self.stack.iter_mut()
    }

    /// Returns a cloned vector of the stack contents in bottom-to-top order.
    #[must_use]
    pub fn to_vec(&self) -> Vec<StackItem> {
        self.stack.clone()
    }

    #[inline(always)]
    fn resolve_top_index(&self, index_from_top: usize) -> VmResult<usize> {
        if index_from_top >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        Ok(self.stack.len() - index_from_top - 1)
    }

    #[inline(always)]
    fn remove_internal(&mut self, index_from_top: usize) -> VmResult<StackItem> {
        let item = self.remove_no_ref(index_from_top)?;
        self.reference_counter.remove_stack_reference(&item);
        Ok(item)
    }

    /// Removes the item at `index_from_top` without touching the reference
    /// counter.
    ///
    /// neo-vm v3.11.2 `EvaluationStack.RemoveNoRef`.
    #[inline(always)]
    fn remove_no_ref(&mut self, index_from_top: usize) -> VmResult<StackItem> {
        let idx = self.resolve_top_index(index_from_top)?;
        Ok(self.stack.remove(idx))
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
        stack
            .push(StackItem::from_int(1))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(2))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(3))
            .expect("push should succeed");

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
        stack
            .push(StackItem::from_int(1))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(2))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(3))
            .expect("push should succeed");

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
        stack
            .push(StackItem::from_int(1))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(3))
            .expect("push should succeed");

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
        stack
            .push(StackItem::from_int(1))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(2))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(3))
            .expect("push should succeed");

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
        stack
            .push(StackItem::from_int(1))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(2))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(3))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(4))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(5))
            .expect("push should succeed");

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
        stack
            .push(StackItem::from_int(1))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(2))
            .expect("push should succeed");
        stack
            .push(StackItem::from_int(3))
            .expect("push should succeed");

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
        stack1
            .push(StackItem::from_int(1))
            .expect("push should succeed");
        stack1
            .push(StackItem::from_int(2))
            .expect("push should succeed");
        stack1
            .push(StackItem::from_int(3))
            .expect("push should succeed");

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
