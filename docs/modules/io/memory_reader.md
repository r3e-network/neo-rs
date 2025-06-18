# Memory Reader

## Overview

The `MemoryReader` struct provides zero-copy, stack-based, high-performance reading operations for binary data. Unlike `BinaryReader` which uses heap-allocated data, `MemoryReader` works directly with byte slices and is designed to be used as a `ref struct` equivalent in Rust.

## Implementation

The Rust implementation will provide similar functionality to the C# version, but adapted to Rust idioms and memory safety principles. Key differences include:

1. Using `&[u8]` slices instead of `ReadOnlyMemory<byte>` and `ReadOnlySpan<byte>`
2. Implementing proper error handling with Result types
3. Adapting C# concepts like ref structs to appropriate Rust constructs

## API

```rust
pub struct MemoryReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> MemoryReader<'a> {
    /// Creates a new MemoryReader from a byte slice
    pub fn new(data: &'a [u8]) -> Self;
    
    /// Returns the current position in the data
    pub fn position(&self) -> usize;
    
    /// Returns the next byte without advancing the position
    pub fn peek(&self) -> Result<u8>;
    
    /// Reads a boolean value
    pub fn read_bool(&mut self) -> Result<bool>;
    
    /// Reads a signed byte
    pub fn read_i8(&mut self) -> Result<i8>;
    
    /// Reads an unsigned byte
    pub fn read_byte(&mut self) -> Result<u8>;
    
    /// Reads a signed 16-bit integer in little-endian format
    pub fn read_i16(&mut self) -> Result<i16>;
    
    /// Reads a signed 16-bit integer in big-endian format
    pub fn read_i16_big_endian(&mut self) -> Result<i16>;
    
    /// Reads an unsigned 16-bit integer in little-endian format
    pub fn read_u16(&mut self) -> Result<u16>;
    
    /// Reads an unsigned 16-bit integer in big-endian format
    pub fn read_u16_big_endian(&mut self) -> Result<u16>;
    
    /// Reads a signed 32-bit integer in little-endian format
    pub fn read_i32(&mut self) -> Result<i32>;
    
    /// Reads a signed 32-bit integer in big-endian format
    pub fn read_i32_big_endian(&mut self) -> Result<i32>;
    
    /// Reads an unsigned 32-bit integer in little-endian format
    pub fn read_u32(&mut self) -> Result<u32>;
    
    /// Reads an unsigned 32-bit integer in big-endian format
    pub fn read_u32_big_endian(&mut self) -> Result<u32>;
    
    /// Reads a signed 64-bit integer in little-endian format
    pub fn read_i64(&mut self) -> Result<i64>;
    
    /// Reads a signed 64-bit integer in big-endian format
    pub fn read_i64_big_endian(&mut self) -> Result<i64>;
    
    /// Reads an unsigned 64-bit integer in little-endian format
    pub fn read_u64(&mut self) -> Result<u64>;
    
    /// Reads an unsigned 64-bit integer in big-endian format
    pub fn read_u64_big_endian(&mut self) -> Result<u64>;
    
    /// Reads a variable-length integer
    pub fn read_var_int(&mut self, max: u64) -> Result<u64>;
    
    /// Reads a fixed-length string
    pub fn read_fixed_string(&mut self, length: usize) -> Result<String>;
    
    /// Reads a variable-length string
    pub fn read_var_string(&mut self, max: usize) -> Result<String>;
    
    /// Reads a fixed number of bytes
    pub fn read_bytes(&mut self, count: usize) -> Result<&'a [u8]>;
    
    /// Reads a variable-length byte array
    pub fn read_var_bytes(&mut self, max: usize) -> Result<&'a [u8]>;
    
    /// Reads all remaining bytes
    pub fn read_to_end(&mut self) -> &'a [u8];
}
```

## Differences from C#

1. In Rust, we use lifetime parameters (`'a`) to ensure the borrowed data remains valid
2. Error handling uses the `Result` type instead of throwing exceptions
3. All methods return references to the original slice instead of creating new allocations
4. Big-endian methods are implemented separately rather than using a shared implementation

## Usage Examples

```rust
// Create a new memory reader
let data = &[0x01, 0x02, 0x03, 0x04];
let mut reader = MemoryReader::new(data);

// Read values
let b = reader.read_byte()?;
let u16_val = reader.read_u16()?;

// Read a variable-length int with maximum value
let var_int = reader.read_var_int(100)?;

// Read a string
let string = reader.read_var_string(1000)?;
``` 