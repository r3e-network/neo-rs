# UInt160 and UInt256 Implementation

This document details the implementation of UInt160 and UInt256 types in Rust.

## Overview

UInt160 and UInt256 are fixed-size unsigned integers used throughout the Neo blockchain:

- UInt160: 160-bit (20-byte) unsigned integer, used for addresses and script hashes
- UInt256: 256-bit (32-byte) unsigned integer, used for transaction and block hashes

## C# Implementation

In the C# implementation, UInt160 and UInt256 are classes with:

- Fixed-size byte arrays stored as multiple ulong fields
- Serialization/deserialization support
- Comparison and equality methods
- Conversion to/from hexadecimal strings
- Implicit conversion from strings and byte arrays

## Rust Implementation

In the Rust implementation, UInt160 and UInt256 are structs with:

- Fixed-size byte arrays stored directly
- Serialization/deserialization via the Serializable trait
- Comparison and equality via standard traits
- Conversion to/from hexadecimal strings
- FromStr implementation for string parsing

### UInt160 Structure

```rust
pub struct UInt160 {
    pub data: [u8; UINT160_SIZE],
}
```

### UInt256 Structure

```rust
pub struct UInt256 {
    pub data: [u8; UINT256_SIZE],
}
```

## Key Features

### Creation and Conversion

Both types provide methods for:

- Creating from byte arrays
- Creating from slices
- Parsing from hexadecimal strings
- Converting to hexadecimal strings

### Serialization

Both types implement the Serializable trait for:

- Serializing to binary format
- Deserializing from binary format

### Comparison and Equality

Both types implement standard Rust traits:

- PartialEq for equality comparison
- Eq for reflexive equality
- PartialOrd for ordering comparison
- Ord for total ordering

### String Conversion

Both types provide:

- FromStr implementation for parsing from strings
- Display implementation for formatting to strings
- Debug implementation for debug formatting

## Usage Examples

### Creating UInt160/UInt256

```rust
// From byte array
let data = [1u8; 20];
let uint160 = UInt160::new(data);

// From slice
let data = [1u8; 32];
let uint256 = UInt256::from_slice(&data).unwrap();

// From hex string
let uint160 = UInt160::parse("0x0000000000000000000000000000000000000001").unwrap();
let uint256 = UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000001").unwrap();
```

### Converting to String

```rust
let uint160 = UInt160::new([1u8; 20]);
let hex_string = uint160.to_hex_string();
println!("{}", hex_string); // 0x0101010101010101010101010101010101010101

let uint256 = UInt256::new([1u8; 32]);
println!("{}", uint256); // Uses Display implementation
```

### Comparison

```rust
let uint1 = UInt160::new([1u8; 20]);
let uint2 = UInt160::new([2u8; 20]);

if uint1 < uint2 {
    println!("uint1 is less than uint2");
}

if uint1 == UInt160::new([1u8; 20]) {
    println!("uint1 equals the new UInt160");
}
```

## Implementation Differences

### Memory Layout

- C#: Uses multiple ulong fields with explicit offsets
- Rust: Uses a single byte array

### Null Handling

- C#: Can be null (reference type)
- Rust: Cannot be null (value type)

### String Parsing

- C#: Uses TryParse pattern
- Rust: Uses FromStr trait and Result

### Serialization

- C#: Uses ISerializable interface
- Rust: Uses Serializable trait

## Performance Considerations

- Byte order handling for little-endian vs big-endian platforms
- Optimized comparison by comparing from most significant byte
- Efficient serialization without unnecessary copying

## Testing

Both types have comprehensive tests for:

- Creation and conversion
- Parsing from strings
- Formatting to strings
- Comparison and equality
- Error handling
