# Instruction Module

## Overview

The Instruction module represents a single instruction in the Neo Virtual Machine (NeoVM). An instruction consists of an opcode and optional operands. This module is responsible for parsing, representing, and working with individual instructions in NeoVM scripts.

## Implementation Details

### Instruction Structure

The Instruction struct contains an opcode and its operands:

```rust
pub struct Instruction {
    /// The operation code of the instruction
    opcode: OpCode,
    
    /// The operand data (if any)
    operand: Option<Vec<u8>>,
    
    /// The size of the instruction in bytes (opcode + operands)
    size: usize,
}
```

### Core Functionality

The Instruction module provides the following core functionality:

1. **Instruction Parsing**: Parsing instructions from a byte array
2. **Opcode Access**: Retrieving the opcode of an instruction
3. **Operand Access**: Retrieving the operands of an instruction
4. **Size Calculation**: Determining the total size of an instruction
5. **Operand Interpretation**: Converting operands to various data types

### API

```rust
impl Instruction {
    /// Parses an instruction from a byte array at the specified position
    pub fn parse(script: &[u8], position: usize) -> Result<Self>;
    
    /// Returns the opcode of the instruction
    pub fn opcode(&self) -> OpCode;
    
    /// Returns the operand data, if any
    pub fn operand(&self) -> Option<&[u8]>;
    
    /// Returns the total size of the instruction in bytes (opcode + operands)
    pub fn size(&self) -> usize;
    
    /// Returns the offset operand for jump instructions as a signed integer
    pub fn jump_offset(&self) -> Result<i32>;
    
    /// Returns the token operand for token-related instructions
    pub fn token_value(&self) -> Result<u16>;
    
    /// Converts the operand to a signed integer
    pub fn operand_to_i32(&self) -> Result<i32>;
    
    /// Converts the operand to an unsigned integer
    pub fn operand_to_u32(&self) -> Result<u32>;
}
```

## Usage Examples

```rust
// Parse an instruction from a byte array
let script = vec![OpCode::PUSH1 as u8, 0x01, 0x02, 0x03, 0x04];
let instruction = Instruction::parse(&script, 0)?;

// Get the opcode
let opcode = instruction.opcode();
println!("OpCode: {:?}", opcode);

// Get the size
let size = instruction.size();
println!("Size: {}", size);

// Get the operand
if let Some(operand) = instruction.operand() {
    println!("Operand: {:?}", operand);
}

// For jump instructions, get the jump offset
if opcode == OpCode::JMP || opcode == OpCode::JMPIF {
    let offset = instruction.jump_offset()?;
    println!("Jump offset: {}", offset);
}
```

## Considerations

1. **Error Handling**: The module should handle parsing errors gracefully, especially for out-of-bounds access and invalid opcodes.

2. **Efficiency**: Instruction parsing should be efficient, as it's frequently performed during script execution.

3. **Operand Interpretation**: Different opcodes interpret their operands differently, and the module should provide helper methods for common interpretations.

4. **Compatibility**: The behavior must match the C# implementation to ensure consistent script execution.

## Implementation Approach

The Instruction implementation follows these principles:

1. Parse instructions based on the opcode and its operand size
2. Handle both fixed-size operands and variable-size operands with size prefixes
3. Provide methods to access and interpret operands in various ways
4. Implement error handling for invalid instructions
5. Ensure compatibility with the C# implementation 