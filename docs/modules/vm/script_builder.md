# ScriptBuilder Module

## Overview

The ScriptBuilder module provides a way to programmatically construct scripts for the Neo Virtual Machine (NeoVM). It offers methods to emit opcodes and operands, allowing developers to build complex scripts without manually constructing bytecode.

## Implementation Details

### ScriptBuilder Structure

The ScriptBuilder struct is used to build VM scripts:

```rust
pub struct ScriptBuilder {
    /// The script being built
    script: Vec<u8>,
}
```

### Core Functionality

The ScriptBuilder module provides the following core functionality:

1. **Opcode Emission**: Methods for emitting VM opcodes
2. **Operand Emission**: Methods for emitting operands with proper encoding
3. **High-Level Methods**: Convenience methods for common script patterns
4. **Script Finalization**: Methods for creating a Script from the builder

### API

```rust
impl ScriptBuilder {
    /// Creates a new script builder
    pub fn new() -> Self;
    
    /// Emits a single byte to the script
    pub fn emit(&mut self, op: u8) -> &mut Self;
    
    /// Emits an opcode to the script
    pub fn emit_opcode(&mut self, op: OpCode) -> &mut Self;
    
    /// Emits a push operation with the given data
    pub fn emit_push(&mut self, data: &[u8]) -> &mut Self;
    
    /// Emits a push operation for an integer
    pub fn emit_push_int(&mut self, value: i64) -> &mut Self;
    
    /// Emits a push operation for a boolean
    pub fn emit_push_bool(&mut self, value: bool) -> &mut Self;
    
    /// Emits a jump operation
    pub fn emit_jump(&mut self, op: OpCode, offset: i16) -> &mut Self;
    
    /// Emits a call operation
    pub fn emit_call(&mut self, offset: i16) -> &mut Self;
    
    /// Emits a syscall operation
    pub fn emit_syscall(&mut self, api: &str) -> &mut Self;
    
    /// Emits an append operation
    pub fn emit_append(&mut self) -> &mut Self;
    
    /// Emits a pack operation
    pub fn emit_pack(&mut self) -> &mut Self;
    
    /// Converts the builder to a script
    pub fn to_script(&self) -> Script;
    
    /// Converts the builder to a byte array
    pub fn to_array(&self) -> Vec<u8>;
}
```

## Usage Examples

```rust
// Create a new script builder
let mut builder = ScriptBuilder::new();

// Build a script that adds two numbers
builder
    .emit_push_int(10)      // Push 10 onto the stack
    .emit_push_int(20)      // Push 20 onto the stack
    .emit_opcode(OpCode::ADD)   // Add the top two items
    .emit_opcode(OpCode::RET);  // Return from the script

// Convert to a script
let script = builder.to_script();

// Execute the script
let mut engine = ExecutionEngine::new(None);
engine.load_script(script, -1, 0).unwrap();
engine.execute();
```

More complex example:

```rust
// Create a script that performs a conditional jump
let mut builder = ScriptBuilder::new();

// Build the script
builder
    .emit_push_int(10)           // Push 10 onto the stack
    .emit_push_int(5)            // Push 5 onto the stack
    .emit_opcode(OpCode::GT)     // 10 > 5?
    .emit_jump(OpCode::JMPIF, 5) // Jump 5 bytes if true
    .emit_push_int(0)            // Push 0 (false path)
    .emit_opcode(OpCode::RET)    // Return
    .emit_push_int(1)            // Push 1 (true path)
    .emit_opcode(OpCode::RET);   // Return

// Convert to a script
let script = builder.to_script();
```

## Considerations

1. **Script Validity**: The builder should ensure that the generated script is valid.

2. **Operand Encoding**: The builder should correctly encode operands based on opcode requirements.

3. **Jump Calculation**: Jump offsets must be calculated correctly to ensure proper control flow.

4. **Script Size**: The builder should manage script size efficiently.

5. **Compatibility**: The behavior must match the C# implementation to ensure consistent script generation.

## Implementation Approach

The ScriptBuilder implementation follows these principles:

1. Provide methods for emitting opcodes and operands
2. Offer high-level methods for common script patterns
3. Ensure proper encoding of operands
4. Calculate jump offsets correctly
5. Ensure compatibility with the C# implementation 