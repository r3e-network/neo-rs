# OpCode Module

## Overview

The OpCode module defines all the instructions supported by the Neo Virtual Machine (NeoVM). Each instruction has:

- A byte value
- A name
- An optional operand size (0, 1, 2, 4, or 8 bytes)
- Documentation about its behavior, stack effects, and usage

## Implementation Details

### OpCode Enum

The OpCode enum contains all the instruction codes supported by the NeoVM. It is implemented as a Rust enum with explicit byte values to ensure compatibility with the C# implementation.

```rust
pub enum OpCode {
    // Constants
    PUSHINT8 = 0x00,
    PUSHINT16 = 0x01,
    PUSHINT32 = 0x02,
    PUSHINT64 = 0x03,
    PUSHINT128 = 0x04,
    PUSHINT256 = 0x05,
    // ... other opcodes
}
```

### Operand Size Information

Since Rust doesn't have attributes in the same way as C#, we implement the operand size information through a method on the OpCode enum:

```rust
impl OpCode {
    pub fn operand_size(&self) -> usize {
        match self {
            OpCode::PUSHINT8 => 1,
            OpCode::PUSHINT16 => 2,
            OpCode::PUSHINT32 => 4,
            OpCode::PUSHINT64 => 8,
            OpCode::PUSHINT128 => 16,
            OpCode::PUSHINT256 => 32,
            // ... other cases
            _ => 0,
        }
    }
    
    pub fn size_prefix(&self) -> usize {
        match self {
            OpCode::PUSHDATA1 => 1,
            OpCode::PUSHDATA2 => 2,
            OpCode::PUSHDATA4 => 4,
            // ... other cases
            _ => 0,
        }
    }
}
```

### Helper Functions

The OpCode module provides several helper functions to work with opcodes:

```rust
impl OpCode {
    // Convert a byte to an OpCode
    pub fn from_byte(byte: u8) -> Option<Self> { ... }
    
    // Get the name of an OpCode
    pub fn name(&self) -> &'static str { ... }
    
    // Check if an OpCode is a branch instruction
    pub fn is_branch(&self) -> bool { ... }
    
    // Check if an OpCode is a return instruction
    pub fn is_return(&self) -> bool { ... }
    
    // Get the number of stack items pushed by this instruction
    pub fn stack_items_pushed(&self) -> isize { ... }
    
    // Get the number of stack items popped by this instruction
    pub fn stack_items_popped(&self) -> isize { ... }
}
```

## Usage Examples

```rust
// Create an OpCode from a byte
let opcode = OpCode::from_byte(0x00).unwrap(); // PUSHINT8

// Get the name of an OpCode
assert_eq!(opcode.name(), "PUSHINT8");

// Get the operand size
let size = opcode.operand_size(); // 1

// Check if an OpCode is a branch instruction
let is_branch = OpCode::JMP.is_branch(); // true

// Get stack effects
let pushed = OpCode::ADD.stack_items_pushed(); // 1
let popped = OpCode::ADD.stack_items_popped(); // 2
```

## Considerations

1. **Compatibility**: The implementation must be compatible with the C# implementation, using the same byte values and behavior.

2. **Safety**: The Rust implementation should avoid panics by using Option/Result types for operations that might fail.

3. **Documentation**: Each OpCode has comprehensive documentation including its effects on the stack and usage examples.

4. **Extensibility**: The design should allow for future extensions to the instruction set.

## Implementation Approach

The OpCode implementation follows these principles:

1. Use a Rust enum with explicit discriminants to represent the opcodes
2. Implement helper methods to provide operand size information
3. Use match expressions for handling different opcodes
4. Provide comprehensive documentation for each opcode
5. Use extensive unit tests to ensure compatibility with the C# implementation 