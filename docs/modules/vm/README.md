# Neo VM Module

This module provides an implementation of the Neo Virtual Machine (NeoVM) used by the Neo blockchain to execute smart contracts. It is a port of the C# NeoVM implementation to Rust.

## Overview

The Neo VM is a stack-based virtual machine designed to execute Neo smart contract bytecode. It provides a set of operations for manipulating data on the stack, performing arithmetic and logical operations, and controlling execution flow.

## Components

The VM module consists of the following main components:

### OpCode

The `OpCode` module defines all the operation codes used by the Neo VM. Each opcode represents a specific instruction that can be executed by the VM. The opcodes are organized into categories based on their purpose, such as stack operations, arithmetic operations, logical operations, and control flow operations.

### Instruction

The `Instruction` module represents a single instruction in the Neo VM. It consists of an opcode and optional operands. It also provides methods for parsing instructions from a byte array and retrieving information about the instruction.

### Script

The `Script` module represents a script that can be executed by the Neo VM. It consists of a sequence of instructions. It provides methods for loading scripts from a byte array and iterating over the instructions.

### StackItem

The `StackItem` module defines the types of items that can be stored on the VM stack. These include primitive types such as integers, booleans, and byte arrays, as well as more complex types such as arrays, maps, and interop interfaces.

### EvaluationStack

The `EvaluationStack` module represents the stack used by the Neo VM to evaluate expressions and store temporary values during execution. It provides methods for pushing, popping, and peeking at items on the stack.

### ExecutionContext

The `ExecutionContext` module represents the execution context for a script. It includes the script, the current instruction pointer, and the evaluation stack. It also provides methods for navigating through the script and accessing the evaluation stack.

### ReferenceCounter

The `ReferenceCounter` module provides reference counting functionality for managing the lifetime of objects in the VM. It helps ensure that objects are properly cleaned up when they are no longer needed.

### JumpTable

The `JumpTable` module maps opcodes to handler functions that implement the behavior of each operation. It provides a way to customize the behavior of the VM by adding or replacing operation handlers.

### ExecutionEngine

The `ExecutionEngine` module is the core of the Neo VM. It is responsible for executing scripts, managing execution contexts, and handling state changes. It uses a jump table to dispatch instructions to the appropriate handler functions.

### InteropService

The `InteropService` module provides interoperability between the VM and external services. It allows smart contracts to call into functionality implemented outside the VM, such as blockchain operations, system calls, and other native functions.

### ScriptBuilder

The `ScriptBuilder` module helps construct VM scripts programmatically. It offers methods to emit opcodes and operands, allowing developers to build complex scripts without manually constructing bytecode.

### ApplicationEngine

The `ApplicationEngine` module extends the VM with Neo blockchain-specific functionality. It provides a specialized execution engine for running smart contracts in the Neo blockchain environment, including gas tracking, permission management, and blockchain state access.

## Usage

To use the Neo VM, you typically:

1. Create a script from a byte array or use a ScriptBuilder to build one
2. Create an execution engine or application engine
3. Load the script into the engine
4. Execute the script
5. Retrieve the result from the result stack

Here's a simple example:

```rust
use neo_vm::{ExecutionEngine, Script, VMState, JumpTable, OpCode, StackItem};

// Create a jump table with handlers for basic operations
let mut jump_table = JumpTable::new();
jump_table.set(OpCode::ADD, |engine, _instruction| {
    let context = engine.current_context_mut().unwrap();
    let stack = context.evaluation_stack_mut();
    
    // Pop the operands
    let b = stack.pop()?;
    let a = stack.pop()?;
    
    // Perform the addition
    let result = a.as_int()? + b.as_int()?;
    
    // Push the result
    stack.push(StackItem::from_int(result));
    
    Ok(())
});

// Create a script that adds 1 and 2
let script_bytes = vec![
    OpCode::PUSH1 as u8,
    OpCode::PUSH2 as u8,
    OpCode::ADD as u8,
    OpCode::RET as u8,
];
let script = Script::new_relaxed(script_bytes);

// Create an execution engine
let mut engine = ExecutionEngine::new(Some(jump_table));

// Load the script
engine.load_script(script, -1, 0).unwrap();

// Execute the script
let state = engine.execute();

// Check the result
assert_eq!(state, VMState::HALT);
```

For Neo blockchain applications, use the ApplicationEngine:

```rust
use neo_vm::{ApplicationEngine, TriggerType, Script, StackItem, ScriptBuilder, OpCode};

// Create an application engine
let mut engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);

// Create a script that performs operations
let mut builder = ScriptBuilder::new();
builder
    .emit_push_int(10)
    .emit_push_int(20)
    .emit_opcode(OpCode::ADD)
    .emit_opcode(OpCode::RET);
let script = builder.to_script();

// Execute the script
let state = engine.execute(script);

// Check the result
assert_eq!(state, VMState::HALT);

// Check gas consumption
println!("Gas consumed: {}", engine.gas_consumed());
```

## Implementation Status

The Neo VM module is now complete with all components implemented:

- Core VM functionality
- Instruction parsing and execution
- Script loading and validation
- Execution context management 
- Stack item type system
- Reference counting for memory management
- Interoperability with external systems
- Neo blockchain-specific extensions (ApplicationEngine)
- Script building capabilities

Opcode handlers need to be fully implemented to support all Neo VM operations, but the framework is in place to add them according to your application's needs.

## References

- [Neo VM Documentation](https://developers.neo.org/docs/n3/vm)
- [Neo C# VM Repository](https://github.com/neo-project/neo-vm)
- [Neo VM Opcodes](https://developers.neo.org/docs/n3/reference/vm/opcodes) 