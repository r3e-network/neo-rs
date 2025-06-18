# Script Module

## Overview

The Script module represents a compiled script that can be executed by the Neo Virtual Machine (NeoVM). A script is essentially a sequence of instructions (opcodes and their operands) that the VM processes sequentially.

## Implementation Details

### Script Structure

The Script struct contains a binary representation of the compiled script and provides methods to work with it:

```rust
pub struct Script {
    /// The raw script data
    script: Vec<u8>,
}
```

### Core Functionality

The Script module provides the following core functionality:

1. **Creating Scripts**: Creating new scripts from byte arrays or other sources
2. **Script Validation**: Validating scripts for correct format and structure
3. **Instruction Iteration**: Iterating through the instructions in the script
4. **Instruction Parsing**: Parsing individual instructions from the script
5. **Jump Target Calculation**: Computing jump targets for branch instructions

### API

```rust
impl Script {
    /// Creates a new script from a byte array
    pub fn new(script: Vec<u8>) -> Self;
    
    /// Creates a new script from a slice
    pub fn from_slice(script: &[u8]) -> Self;
    
    /// Returns the raw script data
    pub fn data(&self) -> &[u8];
    
    /// Returns an iterator over the instructions in the script
    pub fn instructions(&self) -> InstructionIterator;
    
    /// Returns the instruction at the specified offset
    pub fn get_instruction(&self, position: usize) -> Result<Instruction>;
    
    /// Calculates the offset for a jump instruction
    pub fn get_jump_offset(&self, position: usize, offset: i32) -> Result<usize>;
    
    /// Validates the script format
    pub fn validate(&self) -> Result<()>;
}
```

### Instruction Iterator

The Script module includes an iterator for traversing the instructions in a script:

```rust
pub struct InstructionIterator<'a> {
    script: &'a Script,
    position: usize,
}

impl<'a> Iterator for InstructionIterator<'a> {
    type Item = Result<(usize, Instruction)>;
    
    fn next(&mut self) -> Option<Self::Item>;
}
```

## Usage Examples

```rust
// Create a script from a byte array
let script_bytes = vec![
    OpCode::PUSH1 as u8,
    OpCode::PUSH2 as u8,
    OpCode::ADD as u8,
];
let script = Script::new(script_bytes);

// Iterate through instructions
for instruction in script.instructions() {
    match instruction {
        Ok((offset, instruction)) => {
            println!("Offset: {}, OpCode: {:?}", offset, instruction.opcode());
        }
        Err(e) => {
            println!("Error parsing instruction: {}", e);
            break;
        }
    }
}

// Get a specific instruction
let instruction = script.get_instruction(0)?;
println!("OpCode: {:?}", instruction.opcode());

// Validate a script
match script.validate() {
    Ok(_) => println!("Script is valid"),
    Err(e) => println!("Script is invalid: {}", e),
}
```

## Considerations

1. **Safety**: The implementation should handle out-of-bounds access and invalid opcodes gracefully with proper error handling.

2. **Efficiency**: Script parsing and instruction iteration should be efficient, as these operations are frequently performed during execution.

3. **Compatibility**: The behavior must match the C# implementation to ensure consistent script execution.

4. **Jump Validation**: Branch instructions should validate that jump targets are within the script bounds.

## Implementation Approach

The Script implementation follows these principles:

1. Store the raw script data as a byte vector
2. Implement methods to create and validate scripts
3. Provide an iterator for traversing instructions
4. Implement error handling for invalid scripts
5. Ensure compatibility with the C# implementation 