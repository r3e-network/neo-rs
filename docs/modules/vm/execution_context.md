# ExecutionContext Module

## Overview

The ExecutionContext module represents an execution context in the Neo Virtual Machine (NeoVM). An execution context is created when a script is loaded into the VM and contains the script, instruction pointer, evaluation stack, and other state information needed for execution.

## Implementation Details

### ExecutionContext Structure

The ExecutionContext struct contains the script, instruction pointer, and other state information:

```rust
pub struct ExecutionContext {
    /// The script being executed
    script: Script,
    
    /// The current instruction pointer
    instruction_pointer: usize,
    
    /// The number of values to return when the context is unloaded (-1 for all)
    rvcount: i32,
    
    /// The evaluation stack for this context
    evaluation_stack: EvaluationStack,
    
    /// The static fields for this context
    static_fields: Option<Slot>,
    
    /// The local variables for this context
    local_variables: Option<Slot>,
    
    /// The arguments for this context
    arguments: Option<Slot>,
}
```

### Core Functionality

The ExecutionContext module provides the following core functionality:

1. **Context Creation**: Creating new execution contexts for scripts
2. **Instruction Execution**: Managing the execution of instructions in the script
3. **Stack Management**: Providing an evaluation stack for script execution
4. **State Management**: Tracking the state of execution
5. **Context Unloading**: Handling the unloading of contexts when execution completes

### API

```rust
impl ExecutionContext {
    /// Creates a new execution context
    pub fn new(script: Script, rvcount: i32, reference_counter: &ReferenceCounter) -> Self;
    
    /// Returns the script for this context
    pub fn script(&self) -> &Script;
    
    /// Returns the current instruction pointer
    pub fn instruction_pointer(&self) -> usize;
    
    /// Sets the instruction pointer
    pub fn set_instruction_pointer(&mut self, position: usize);
    
    /// Returns the current instruction or None if at the end of the script
    pub fn current_instruction(&self) -> Option<Instruction>;
    
    /// Returns the number of values to return when the context is unloaded (-1 for all)
    pub fn rvcount(&self) -> i32;
    
    /// Returns the evaluation stack for this context
    pub fn evaluation_stack(&self) -> &EvaluationStack;
    
    /// Returns the evaluation stack for this context (mutable)
    pub fn evaluation_stack_mut(&mut self) -> &mut EvaluationStack;
    
    /// Returns the static fields for this context
    pub fn static_fields(&self) -> Option<&Slot>;
    
    /// Sets the static fields for this context
    pub fn set_static_fields(&mut self, static_fields: Option<Slot>);
    
    /// Returns the local variables for this context
    pub fn local_variables(&self) -> Option<&Slot>;
    
    /// Sets the local variables for this context
    pub fn set_local_variables(&mut self, local_variables: Option<Slot>);
    
    /// Returns the arguments for this context
    pub fn arguments(&self) -> Option<&Slot>;
    
    /// Sets the arguments for this context
    pub fn set_arguments(&mut self, arguments: Option<Slot>);
    
    /// Moves to the next instruction
    pub fn move_next(&mut self);
    
    /// Clones the context with a new reference counter
    pub fn clone_for_reference_counter(&self, reference_counter: &ReferenceCounter) -> Self;
}
```

## Usage Examples

```rust
// Create a new execution context
let script = Script::new(vec![OpCode::PUSH1 as u8, OpCode::RET as u8], false).unwrap();
let reference_counter = ReferenceCounter::new();
let context = ExecutionContext::new(script, -1, &reference_counter);

// Get the current instruction
let instruction = context.current_instruction().unwrap();
println!("Current OpCode: {:?}", instruction.opcode());

// Access the evaluation stack
let stack = context.evaluation_stack_mut();
stack.push(StackItem::Integer(42));

// Move to the next instruction
context.move_next();
```

## Considerations

1. **Reference Counting**: The execution context must properly manage references to ensure memory safety.

2. **Stack Safety**: The evaluation stack must handle overflows and underflows gracefully.

3. **Instruction Pointer Validation**: The instruction pointer must be validated to ensure it remains within the script bounds.

4. **Context Cloning**: The context might need to be cloned for certain operations, which should be done efficiently.

5. **Compatibility**: The behavior must match the C# implementation to ensure consistent script execution.

## Implementation Approach

The ExecutionContext implementation follows these principles:

1. Store the script and current state needed for execution
2. Provide methods to access and modify the context state
3. Implement reference counting to manage object lifetimes
4. Ensure compatibility with the C# implementation 