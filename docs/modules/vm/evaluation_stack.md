# EvaluationStack Module

## Overview

The EvaluationStack module represents a stack used by the Neo Virtual Machine (NeoVM) for executing scripts. This stack holds values during script execution, such as operands for arithmetic operations, method arguments, and return values.

## Implementation Details

### EvaluationStack Structure

The EvaluationStack is a stack implementation that provides methods for pushing and popping items:

```rust
pub struct EvaluationStack {
    /// The underlying stack storage
    stack: Vec<StackItem>,
    
    /// The reference counter for managing object lifetimes
    reference_counter: ReferenceCounter,
}
```

### Core Functionality

The EvaluationStack module provides the following core functionality:

1. **Stack Operations**: Pushing and popping items from the stack
2. **Peek Operations**: Examining items on the stack without removing them
3. **Stack Manipulation**: Inserting, removing, and swapping items in the stack
4. **Reference Counting**: Managing object lifetimes through reference counting

### API

```rust
impl EvaluationStack {
    /// Creates a new evaluation stack with the specified reference counter
    pub fn new(reference_counter: ReferenceCounter) -> Self;
    
    /// Pushes an item onto the stack
    pub fn push(&mut self, item: StackItem);
    
    /// Pops an item from the stack
    pub fn pop(&mut self) -> Result<StackItem>;
    
    /// Returns the item at the top of the stack without removing it
    pub fn peek(&self, n: usize) -> Result<&StackItem>;
    
    /// Returns the item at the top of the stack without removing it (mutable)
    pub fn peek_mut(&mut self, n: usize) -> Result<&mut StackItem>;
    
    /// Returns the number of items on the stack
    pub fn len(&self) -> usize;
    
    /// Returns true if the stack is empty
    pub fn is_empty(&self) -> bool;
    
    /// Removes the item at the specified index from the stack
    pub fn remove(&mut self, index: usize) -> Result<StackItem>;
    
    /// Inserts an item at the specified index in the stack
    pub fn insert(&mut self, index: usize, item: StackItem) -> Result<()>;
    
    /// Swaps the positions of two items on the stack
    pub fn swap(&mut self, i: usize, j: usize) -> Result<()>;
    
    /// Copies items from this stack to another stack
    pub fn copy_to(&self, target: &mut EvaluationStack);
    
    /// Clears the stack
    pub fn clear(&mut self);
}

impl Drop for EvaluationStack {
    fn drop(&mut self);
}
```

## Usage Examples

```rust
// Create a new evaluation stack
let reference_counter = ReferenceCounter::new();
let mut stack = EvaluationStack::new(reference_counter);

// Push some items onto the stack
stack.push(StackItem::Integer(1));
stack.push(StackItem::Integer(2));
stack.push(StackItem::Integer(3));

// Pop an item from the stack
let item = stack.pop().unwrap();
println!("Popped item: {:?}", item); // Prints "Popped item: Integer(3)"

// Peek at the top item
let top = stack.peek(0).unwrap();
println!("Top item: {:?}", top); // Prints "Top item: Integer(2)"

// Check stack size
println!("Stack size: {}", stack.len()); // Prints "Stack size: 2"
```

## Considerations

1. **Reference Counting**: The stack must properly manage references to ensure memory safety.

2. **Stack Safety**: The stack must handle overflows and underflows gracefully.

3. **Error Handling**: Stack operations should handle errors properly.

4. **Efficiency**: Stack operations should be efficient, as they are frequently performed during execution.

5. **Compatibility**: The behavior must match the C# implementation to ensure consistent script execution.

## Implementation Approach

The EvaluationStack implementation follows these principles:

1. Use a vector as the underlying storage for stack items
2. Implement reference counting to manage object lifetimes
3. Provide methods for stack manipulation
4. Handle errors gracefully
5. Ensure compatibility with the C# implementation 