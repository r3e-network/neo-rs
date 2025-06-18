//! Evaluation stack module for the Neo Virtual Machine.
//!
//! This module represents a stack used by the Neo VM for executing scripts.

use crate::reference_counter::ReferenceCounter;
use crate::stack_item::StackItem;
use crate::Error;
use crate::Result;

/// Represents the evaluation stack in the VM.
#[derive(Clone)]
pub struct EvaluationStack {
    /// The underlying stack storage
    stack: Vec<StackItem>,

    /// The reference counter for managing object lifetimes
    reference_counter: ReferenceCounter,
}

impl EvaluationStack {
    /// Creates a new evaluation stack with the specified reference counter.
    pub fn new(reference_counter: ReferenceCounter) -> Self {
        Self {
            stack: Vec::new(),
            reference_counter,
        }
    }

    /// Returns the reference counter for this evaluation stack.
    pub fn reference_counter(&self) -> &ReferenceCounter {
        &self.reference_counter
    }

    /// Pushes an item onto the stack.
    pub fn push(&mut self, item: StackItem) {
        // Add a stack reference to the item (matches C# AddStackReference exactly)
        self.reference_counter.add_stack_reference(&item);
        self.stack.push(item);
    }

    /// Pops an item from the stack.
    pub fn pop(&mut self) -> Result<StackItem> {
        match self.stack.pop() {
            Some(item) => {
                // Remove a stack reference from the item (matches C# RemoveStackReference exactly)
                self.reference_counter.remove_stack_reference(&item);
                Ok(item)
            }
            None => Err(Error::StackUnderflow),
        }
    }

    /// Returns the item at the top of the stack without removing it.
    pub fn peek(&self, n: isize) -> Result<&StackItem> {
        let mut index = n;
        if index >= self.stack.len() as isize {
            return Err(Error::StackUnderflow);
        }

        if index < 0 {
            index += self.stack.len() as isize;
            if index < 0 {
                return Err(Error::StackUnderflow);
            }
        }

        // Get the item at the specified index from the top of the stack
        let stack_index = self.stack.len() - 1 - (index as usize);
        Ok(&self.stack[stack_index])
    }

    /// Returns the item at the top of the stack without removing it (mutable).
    pub fn peek_mut(&mut self, n: isize) -> Result<&mut StackItem> {
        let mut index = n;
        if index >= self.stack.len() as isize {
            return Err(Error::StackUnderflow);
        }

        if index < 0 {
            index += self.stack.len() as isize;
            if index < 0 {
                return Err(Error::StackUnderflow);
            }
        }

        // Get the item at the specified index from the top of the stack
        let stack_index = self.stack.len() - 1 - (index as usize);
        Ok(&mut self.stack[stack_index])
    }

    /// Returns the number of items on the stack.
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Returns true if the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Removes the item at the specified index from the stack.
    pub fn remove(&mut self, index: usize) -> Result<StackItem> {
        if index >= self.stack.len() {
            return Err(Error::StackUnderflow);
        }

        // Remove the item at the specified index
        let item = self.stack.remove(index);

        // Remove a stack reference from the item (matches C# RemoveStackReference exactly)
        self.reference_counter.remove_stack_reference(&item);

        Ok(item)
    }

    /// Inserts an item at the specified index in the stack.
    pub fn insert(&mut self, index: usize, item: StackItem) -> Result<()> {
        if index > self.stack.len() {
            return Err(Error::InvalidOperation("Insert index out of range".into()));
        }

        // Add a stack reference to the item (matches C# AddStackReference exactly)
        self.reference_counter.add_stack_reference(&item);

        // Insert the item at the specified index
        self.stack.insert(index, item);

        Ok(())
    }

    /// Swaps the positions of two items on the stack.
    pub fn swap(&mut self, i: usize, j: usize) -> Result<()> {
        if i >= self.stack.len() || j >= self.stack.len() {
            return Err(Error::StackUnderflow);
        }

        // Swap the items at the specified indices
        self.stack.swap(i, j);

        Ok(())
    }

    /// Reverses the order of n items at the top of the stack.
    pub fn reverse(&mut self, n: usize) -> Result<()> {
        if n > self.stack.len() {
            return Err(Error::InvalidOperation("Reverse count out of range".into()));
        }

        if n <= 1 {
            return Ok(());
        }

        // Reverse the top n items
        let start = self.stack.len() - n;
        let end = self.stack.len();
        self.stack[start..end].reverse();

        Ok(())
    }

    /// Copies items from this stack to another stack.
    pub fn copy_to(&self, target: &mut EvaluationStack) {
        for item in &self.stack {
            // Add a stack reference for each item (matches C# AddStackReference exactly)
            target.reference_counter.add_stack_reference(item);

            target.stack.push(item.clone());
        }
    }

    /// Clears the stack.
    pub fn clear(&mut self) {
        // Remove a stack reference for each item (matches C# RemoveStackReference exactly)
        for item in &self.stack {
            self.reference_counter.remove_stack_reference(item);
        }

        self.stack.clear();
    }

    /// Returns an iterator over the items on the stack - C# API compatibility
    /// This matches the C# IEnumerable<StackItem> interface exactly
    pub fn iter(&self) -> std::slice::Iter<StackItem> {
        self.stack.iter()
    }

    /// Returns a mutable iterator over the items on the stack
    pub fn iter_mut(&mut self) -> std::slice::IterMut<StackItem> {
        self.stack.iter_mut()
    }
}

impl Drop for EvaluationStack {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stack_item::StackItem;

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
        let item = stack.pop().unwrap();
        assert_eq!(item.as_int().unwrap(), 3.into());

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
        let item0 = stack.peek(0).unwrap();
        let item1 = stack.peek(1).unwrap();
        let item2 = stack.peek(2).unwrap();

        assert_eq!(item0.as_int().unwrap(), 3.into());
        assert_eq!(item1.as_int().unwrap(), 2.into());
        assert_eq!(item2.as_int().unwrap(), 1.into());

        // Check stack size (peek doesn't change the stack)
        assert_eq!(stack.len(), 3);
    }

    #[test]
    fn test_insert_remove() {
        let reference_counter = ReferenceCounter::new();
        let mut stack = EvaluationStack::new(reference_counter);

        // Push some items
        stack.push(StackItem::from_int(1));
        stack.push(StackItem::from_int(3));

        // Insert an item
        stack.insert(1, StackItem::from_int(2)).unwrap();

        // Check stack
        assert_eq!(stack.peek(2).unwrap().as_int().unwrap(), 1.into());
        assert_eq!(stack.peek(1).unwrap().as_int().unwrap(), 2.into());
        assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 3.into());

        // Remove an item
        let item = stack.remove(1).unwrap();
        assert_eq!(item.as_int().unwrap(), 2.into());

        // Check stack
        assert_eq!(stack.peek(1).unwrap().as_int().unwrap(), 1.into());
        assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 3.into());
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
        stack.swap(0, 2).unwrap();

        // Check stack
        assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 1.into());
        assert_eq!(stack.peek(1).unwrap().as_int().unwrap(), 2.into());
        assert_eq!(stack.peek(2).unwrap().as_int().unwrap(), 3.into());
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
        stack.reverse(3).unwrap();

        // Check stack
        assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 3.into());
        assert_eq!(stack.peek(1).unwrap().as_int().unwrap(), 4.into());
        assert_eq!(stack.peek(2).unwrap().as_int().unwrap(), 5.into());
        assert_eq!(stack.peek(3).unwrap().as_int().unwrap(), 2.into());
        assert_eq!(stack.peek(4).unwrap().as_int().unwrap(), 1.into());

        // Reverse all items
        stack.reverse(5).unwrap();

        // Check stack
        assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 1.into());
        assert_eq!(stack.peek(1).unwrap().as_int().unwrap(), 2.into());
        assert_eq!(stack.peek(2).unwrap().as_int().unwrap(), 5.into());
        assert_eq!(stack.peek(3).unwrap().as_int().unwrap(), 4.into());
        assert_eq!(stack.peek(4).unwrap().as_int().unwrap(), 3.into());

        // Reverse 0 items (no change)
        stack.reverse(0).unwrap();

        // Check stack (unchanged)
        assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 1.into());
        assert_eq!(stack.peek(1).unwrap().as_int().unwrap(), 2.into());

        // Reverse 1 item (no change)
        stack.reverse(1).unwrap();

        // Check stack (unchanged)
        assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 1.into());

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
        stack1.copy_to(&mut stack2);

        // Check stacks
        assert_eq!(stack1.len(), 3);
        assert_eq!(stack2.len(), 3);

        assert_eq!(stack1.peek(0).unwrap().as_int().unwrap(), 3.into());
        assert_eq!(stack2.peek(0).unwrap().as_int().unwrap(), 3.into());
    }
}