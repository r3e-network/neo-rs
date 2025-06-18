# Exception Handling Compatibility Guide

This document outlines the exact requirements for ensuring the Rust VM's exception handling behavior matches the C# VM implementation perfectly.

## Core Requirements

The exception handling in Neo VM includes several critical components:

1. **Exception Types**: The hierarchy and types of exceptions must be identical
2. **Exception Propagation**: The way exceptions bubble up through call stacks must match
3. **VM State Transitions**: State changes triggered by exceptions must be identical
4. **Try-Catch Mechanism**: Implementation of TRY, ENDTRY, and ENDFINALLY opcodes must behave the same
5. **Uncaught Exception Handling**: Behavior when exceptions are not caught must be identical

## Exception Types and Hierarchy

The C# Neo VM implements several exception types that must be exactly matched:

1. **BadScriptException**: Thrown when the script is malformed
2. **InvalidOperationException**: Thrown when an operation is invalid in the current context
3. **StackUnderflowException**: Thrown when pop is called on an empty stack
4. **NotSupportedException**: Thrown when an operation is not supported

## Exception Handling State and Context

The C# VM maintains exception handling state through:

1. **ExceptionHandlingContext**: Records the TRY scope and handler positions
2. **ExceptionHandlingState**: Indicates the current state of exception handling
3. **TryStack**: Maintains a stack of nested exception handlers

All of these must be implemented identically in the Rust VM.

## VM State Transitions

When an exception occurs, the VM state transitions follow these rules:

1. **Uncaught Exception**: If an exception is not caught, VM state transitions to FAULT
2. **Caught Exception**: If an exception is caught, execution continues at the catch handler
3. **Finally Blocks**: Finally blocks must always execute, even when exceptions occur

## Try-Catch Flow Implementation

The implementation must handle these scenarios exactly like the C# VM:

### Normal Execution Flow

1. ENDTRY is reached without exceptions → skip catch block, execute finally block
2. After finally block, execution continues after the corresponding ENDFINALLY

### Exception Flow

1. Exception occurs in try block → find matching catch handler
2. Execute catch handler
3. Execute finally block
4. Continue execution after ENDFINALLY

### Nested Exception Handling

1. Properly handle nested TRY blocks
2. Maintain correct exception handler stack
3. Ensure proper unwinding of nested handlers

## Verification Approach

Implement test cases that verify identical exception behavior:

1. **Basic Exception Tests**: Test simple throw and catch scenarios
2. **Nested TRY Tests**: Test nested try-catch-finally constructs
3. **Finally Block Tests**: Verify finally blocks always execute
4. **Exception Propagation Tests**: Test how exceptions bubble up through calls
5. **VM State Tests**: Verify VM state transitions match C# implementation

## Implementation Requirements

The Rust implementation should:

1. Implement ExceptionHandlingContext and ExceptionHandlingState enums/structs
2. Add TryStack to ExecutionContext matching C# implementation
3. Implement exception propagation logic identical to C#
4. Handle VM state transitions correctly
5. Ensure opcodes TRY, ENDTRY, THROW, and ENDFINALLY behave identical to C#

## Testing Methodology

For each test scenario, execute the same script in both C# and Rust VMs and verify:

1. VM final state matches
2. Result stack contents match
3. Exception state and type match
4. Call stack unwinding behavior matches

## Reference Materials

1. C# Implementation: 
   - `neo-sharp/src/Neo.VM/ExceptionHandlingContext.cs`
   - `neo-sharp/src/Neo.VM/ExceptionHandlingState.cs`
   - Exception handling in JumpTable control flow opcodes

2. Rust Implementation:
   - `neo-rs/crates/vm/src/exception_handling.rs`
   - Exception handling in JumpTable control flow handlers
