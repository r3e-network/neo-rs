# StackItem Module

## Overview

The StackItem module represents values that can be stored on the evaluation stack of the Neo Virtual Machine (NeoVM). It provides a type system for the VM, including primitive types, collections, and references.

## Implementation Details

### StackItem Types

The StackItem is an enum with different variants representing the various types in the NeoVM:

```rust
pub enum StackItem {
    /// Boolean value (true or false)
    Boolean(bool),
    
    /// Integer value (arbitrary precision)
    Integer(num_bigint::BigInt),
    
    /// Binary data
    ByteString(Vec<u8>),
    
    /// UTF-8 string (wrapped binary data with UTF-8 validation)
    String(String),
    
    /// Ordered collection of items
    Array(Vec<StackItem>),
    
    /// Key-value collection
    Map(BTreeMap<StackItem, StackItem>),
    
    /// Interop interface object
    InteropInterface(Box<dyn InteropInterface>),
    
    /// Pointer to another stack item (for reference counting)
    Pointer(usize),
    
    /// Buffer for in-place modification
    Buffer(Vec<u8>),
    
    /// Structure similar to Array but with field access
    Struct(Vec<StackItem>),
}
```

### Core Functionality

The StackItem module provides the following core functionality:

1. **Type Conversion**: Converting between different stack item types
2. **Comparison**: Comparing stack items for equality and ordering
3. **Clone and Deep Clone**: Creating copies of stack items
4. **Serialization**: Converting stack items to and from byte arrays
5. **Reference Management**: Managing references between stack items

### API

```rust
impl StackItem {
    /// Creates a boolean stack item
    pub fn from_bool(value: bool) -> Self;
    
    /// Creates an integer stack item
    pub fn from_int(value: impl Into<num_bigint::BigInt>) -> Self;
    
    /// Creates a byte string stack item
    pub fn from_byte_string(value: impl Into<Vec<u8>>) -> Self;
    
    /// Creates a string stack item
    pub fn from_string(value: impl Into<String>) -> Self;
    
    /// Creates an array stack item
    pub fn from_array(value: impl Into<Vec<StackItem>>) -> Self;
    
    /// Creates a map stack item
    pub fn from_map(value: impl Into<BTreeMap<StackItem, StackItem>>) -> Self;
    
    /// Creates an interop interface stack item
    pub fn from_interface(value: impl InteropInterface + 'static) -> Self;
    
    /// Creates a buffer stack item
    pub fn from_buffer(value: impl Into<Vec<u8>>) -> Self;
    
    /// Creates a struct stack item
    pub fn from_struct(value: impl Into<Vec<StackItem>>) -> Self;
    
    /// Converts the stack item to a boolean
    pub fn as_bool(&self) -> Result<bool>;
    
    /// Converts the stack item to an integer
    pub fn as_int(&self) -> Result<&num_bigint::BigInt>;
    
    /// Converts the stack item to a byte array
    pub fn as_bytes(&self) -> Result<&[u8]>;
    
    /// Converts the stack item to a string
    pub fn as_string(&self) -> Result<&str>;
    
    /// Converts the stack item to an array
    pub fn as_array(&self) -> Result<&[StackItem]>;
    
    /// Converts the stack item to a map
    pub fn as_map(&self) -> Result<&BTreeMap<StackItem, StackItem>>;
    
    /// Converts the stack item to an interop interface
    pub fn as_interface<T: InteropInterface + 'static>(&self) -> Result<&T>;
    
    /// Converts the stack item to a buffer
    pub fn as_buffer(&self) -> Result<&[u8]>;
    
    /// Converts the stack item to a struct
    pub fn as_struct(&self) -> Result<&[StackItem]>;
    
    /// Gets the type of the stack item
    pub fn stack_item_type(&self) -> StackItemType;
    
    /// Creates a deep clone of the stack item
    pub fn deep_clone(&self) -> Self;
    
    /// Gets the underlying object referenced by a Pointer
    pub fn get_interface(&self) -> Result<&dyn InteropInterface>;
    
    /// Checks if two stack items are equal
    pub fn equals(&self, other: &StackItem) -> Result<bool>;
}

impl PartialEq for StackItem { /* [Implementation complete] */ }
impl Eq for StackItem { /* [Implementation complete] */ }
impl PartialOrd for StackItem { /* [Implementation complete] */ }
impl Ord for StackItem { /* [Implementation complete] */ }
impl Clone for StackItem { /* [Implementation complete] */ }
```

## Usage Examples

```rust
// Create different types of stack items
let bool_item = StackItem::from_bool(true);
let int_item = StackItem::from_int(42);
let string_item = StackItem::from_string("Hello, Neo");
let bytes_item = StackItem::from_byte_string(vec![1, 2, 3]);

// Create an array
let array_item = StackItem::from_array(vec![
    StackItem::from_int(1),
    StackItem::from_int(2),
    StackItem::from_int(3),
]);

// Create a map
let mut map = BTreeMap::new();
map.insert(StackItem::from_string("key"), StackItem::from_int(42));
let map_item = StackItem::from_map(map);

// Convert stack items to different types
let bool_value = bool_item.as_bool().unwrap();
let int_value = int_item.as_int().unwrap();
let string_value = string_item.as_string().unwrap();
let bytes_value = bytes_item.as_bytes().unwrap();

// Compare stack items
let is_equal = bool_item.equals(&StackItem::from_bool(true)).unwrap();
```

## Considerations

1. **Type Conversion**: Type conversions should follow the NeoVM's type system rules.

2. **Comparison**: Equality and ordering should be consistent with the NeoVM's semantics.

3. **Reference Counting**: Stack items must properly handle reference counting.

4. **Deep Cloning**: Deep cloning should create a complete copy of the item and its contained items.

5. **Compatibility**: The behavior must match the C# implementation to ensure consistent script execution.

## Implementation Approach

The StackItem implementation follows these principles:

1. Use an enum to represent the different types of stack items
2. Implement type conversion methods with proper error handling
3. Implement comparison, cloning, and deep cloning
4. Ensure compatibility with the C# implementation
5. Optimize for memory usage and performance 