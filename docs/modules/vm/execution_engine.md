# ExecutionEngine Module

## Overview

The ExecutionEngine module represents the core of the Neo Virtual Machine (NeoVM). It is responsible for executing scripts, managing execution contexts, and handling the evaluation stack.

## Implementation Details

### ExecutionEngine Structure

The ExecutionEngine struct contains the state of the VM and methods for execution:

```rust
pub struct ExecutionEngine {
    /// The current state of the VM
    state: VMState,
    
    /// Flag indicating if the engine is in the middle of a jump
    is_jumping: bool,
    
    /// The jump table used to execute instructions
    jump_table: JumpTable,
    
    /// Restrictions on the VM
    limits: ExecutionEngineLimits,
    
    /// Used for reference counting of objects in the VM
    reference_counter: ReferenceCounter,
    
    /// The invocation stack of the VM
    invocation_stack: Vec<ExecutionContext>,
    
    /// The stack to store the return values
    result_stack: EvaluationStack,
    
    /// The VM object representing the uncaught exception
    uncaught_exception: Option<StackItem>,
}
```

### Core Functionality

The ExecutionEngine module provides the following core functionality:

1. **Script Execution**: Executing scripts in the VM
2. **Instruction Handling**: Processing individual instructions
3. **Context Management**: Loading and unloading execution contexts
4. **State Management**: Tracking the state of the VM
5. **Error Handling**: Handling exceptions and errors during execution

### API

```rust
pub enum VMState {
    NONE,
    HALT,
    BREAK,
    FAULT,
}

impl ExecutionEngine {
    /// Creates a new execution engine with the specified jump table
    pub fn new(jump_table: Option<JumpTable>) -> Self;
    
    /// Creates a new execution engine with the specified reference counter and limits
    pub fn new_with_limits(
        jump_table: Option<JumpTable>,
        reference_counter: ReferenceCounter,
        limits: ExecutionEngineLimits,
    ) -> Self;
    
    /// Returns the current state of the VM
    pub fn state(&self) -> VMState;
    
    /// Sets the state of the VM
    pub fn set_state(&mut self, state: VMState);
    
    /// Returns the reference counter
    pub fn reference_counter(&self) -> &ReferenceCounter;
    
    /// Returns the invocation stack
    pub fn invocation_stack(&self) -> &[ExecutionContext];
    
    /// Returns the current context, if any
    pub fn current_context(&self) -> Option<&ExecutionContext>;
    
    /// Returns the current context (mutable), if any
    pub fn current_context_mut(&mut self) -> Option<&mut ExecutionContext>;
    
    /// Returns the entry context, if any
    pub fn entry_context(&self) -> Option<&ExecutionContext>;
    
    /// Returns the result stack
    pub fn result_stack(&self) -> &EvaluationStack;
    
    /// Returns the result stack (mutable)
    pub fn result_stack_mut(&mut self) -> &mut EvaluationStack;
    
    /// Returns the uncaught exception, if any
    pub fn uncaught_exception(&self) -> Option<&StackItem>;
    
    /// Sets the uncaught exception
    pub fn set_uncaught_exception(&mut self, exception: Option<StackItem>);
    
    /// Starts execution of the VM
    pub fn execute(&mut self) -> VMState;
    
    /// Executes the next instruction
    pub fn execute_next(&mut self) -> Result<()>;
    
    /// Loads a context into the invocation stack
    pub fn load_context(&mut self, context: ExecutionContext) -> Result<()>;
    
    /// Unloads a context from the invocation stack
    pub fn unload_context(&mut self, context: ExecutionContext);
    
    /// Creates a new context with the specified script
    pub fn create_context(&self, script: Script, rvcount: i32, initial_position: usize) -> ExecutionContext;
    
    /// Loads a script and creates a new context
    pub fn load_script(&mut self, script: Script, rvcount: i32, initial_position: usize) -> Result<ExecutionContext>;
}
```

## Usage Examples

```rust
// Create a new execution engine
let jump_table = JumpTable::default();
let mut engine = ExecutionEngine::new(Some(jump_table));

// Create and load a script
let script_bytes = vec![
    OpCode::PUSH1 as u8,
    OpCode::PUSH2 as u8,
    OpCode::ADD as u8,
    OpCode::RET as u8,
];
let script = Script::new(script_bytes, false).unwrap();
let context = engine.load_script(script, -1, 0).unwrap();

// Execute the script
let state = engine.execute();
println!("VM State: {:?}", state);

// Check the result
let result = engine.result_stack().pop().unwrap();
println!("Result: {:?}", result);
```

## Considerations

1. **State Management**: The engine must properly track and update its state.

2. **Instruction Execution**: The engine must correctly execute each instruction.

3. **Context Management**: The engine must properly manage execution contexts.

4. **Resource Limits**: The engine should enforce resource limits to prevent infinite loops and excessive resource usage.

5. **Error Handling**: The engine must handle errors and exceptions gracefully.

6. **Compatibility**: The behavior must match the C# implementation to ensure consistent script execution.

## Implementation Approach

The ExecutionEngine implementation follows these principles:

1. Use a jump table to dispatch instruction execution
2. Implement reference counting for proper object lifetime management
3. Handle errors and exceptions gracefully
4. Enforce resource limits
5. Ensure compatibility with the C# implementation 