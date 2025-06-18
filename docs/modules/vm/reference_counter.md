# ReferenceCounter Module

## Overview

The ReferenceCounter module provides reference counting functionality for objects in the Neo Virtual Machine (NeoVM). It is used to track object lifetimes and ensure proper cleanup when objects are no longer in use.

## Implementation Details

### ReferenceCounter Structure

The ReferenceCounter struct manages reference counts for objects:

```rust
pub struct ReferenceCounter {
    /// A map of object IDs to their reference counts
    references: HashMap<usize, u32>,
    
    /// The next available object ID
    next_id: AtomicUsize,
}
```

### Core Functionality

The ReferenceCounter module provides the following core functionality:

1. **Object Registration**: Registering new objects for reference counting
2. **Reference Management**: Incrementing and decrementing reference counts
3. **Object Cleanup**: Cleaning up objects when their reference count reaches zero
4. **Garbage Collection**: Identifying and cleaning up objects with no references

### API

```rust
impl ReferenceCounter {
    /// Creates a new reference counter
    pub fn new() -> Self;
    
    /// Registers a new object and returns its ID
    pub fn register(&self) -> usize;
    
    /// Increments the reference count for an object
    pub fn add_reference(&self, id: usize);
    
    /// Decrements the reference count for an object
    pub fn remove_reference(&self, id: usize) -> bool;
    
    /// Returns the reference count for an object
    pub fn get_reference_count(&self, id: usize) -> u32;
    
    /// Clears all references
    pub fn clear(&self);
}

impl Clone for ReferenceCounter { /* ... */ }
```

## Usage Examples

```rust
// Create a new reference counter
let counter = ReferenceCounter::new();

// Register some objects
let obj1_id = counter.register();
let obj2_id = counter.register();

// Add references
counter.add_reference(obj1_id);
counter.add_reference(obj1_id);
counter.add_reference(obj2_id);

// Check reference counts
assert_eq!(counter.get_reference_count(obj1_id), 2);
assert_eq!(counter.get_reference_count(obj2_id), 1);

// Remove references
counter.remove_reference(obj1_id);
assert_eq!(counter.get_reference_count(obj1_id), 1);

counter.remove_reference(obj1_id);
assert_eq!(counter.get_reference_count(obj1_id), 0);

// Clear all references
counter.clear();
```

## Considerations

1. **Thread Safety**: The reference counter must be thread-safe.

2. **Efficiency**: Reference counting operations should be efficient, as they are frequently performed.

3. **Memory Management**: The reference counter must handle memory management properly to avoid leaks.

4. **Cyclic References**: The reference counter should handle cyclic references appropriately.

5. **Compatibility**: The behavior must match the C# implementation to ensure consistent script execution.

## Implementation Approach

The ReferenceCounter implementation follows these principles:

1. Use atomic operations for thread safety
2. Implement reference counting with a hash map
3. Provide methods for managing references
4. Ensure compatibility with the C# implementation 