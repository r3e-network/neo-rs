# Complex Script Compatibility Guide

This document defines how the Rust VM must handle complex scripts to exactly match the C# VM's behavior. Complex scripts involve multiple operations, control flow, and multi-faceted state management that all need to be guaranteed identical across both implementations.

## Overview

Complex scripts in Neo VM involve combinations of arithmetic operations, control flow, function calls, exception handling, and data structure manipulation. Each component must execute in an identical manner across implementations to ensure complete compatibility.

## Control Flow Requirements

### Conditional Jumps

| Instruction | Behavior                                                                              | Edge Cases                                    |
|-------------|--------------------------------------------------------------------------------------|-----------------------------------------------|
| `JMP`       | Unconditional jump to the specified offset                                           | Must handle jumping to beginning/end of script |
| `JMPIF`     | Jump if top stack item evaluates to true (pop top item)                              | Handle all truthy/falsy evaluation rules      |
| `JMPIFNOT`  | Jump if top stack item evaluates to false (pop top item)                             | Handle all truthy/falsy evaluation rules      |
| `CALL`      | Jump to subroutine, pushing current position to call stack                           | Nested calls must match exact call stack depth|
| `RET`       | Return from subroutine, popping position from call stack                             | Handle return from top-level script           |

### Truthy/Falsy Evaluation Rules

The following rules must be applied identically when evaluating values for conditional jumps:

1. `Boolean`: Direct value (`true` or `false`)
2. `Integer`: Zero is `false`, all other values are `true`
3. `ByteString/Buffer`: Empty is `false`, all others are `true`
4. `Array/Struct`: Always evaluates to `true` regardless of contents
5. `Map`: Always evaluates to `true` regardless of contents
6. `InteropInterface`: Null is `false`, non-null is `true`

## Exception Handling Requirements

### TRY-CATCH-FINALLY Blocks

| Instruction | Behavior                                                                              | Edge Cases                                    |
|-------------|--------------------------------------------------------------------------------------|-----------------------------------------------|
| `TRY`       | Start a try block with specified catch and finally offsets                           | Catch offset can be 0xFF for no catch block   |
| `ENDTRY`    | End a try, catch, or finally block                                                   | Must handle nested exception contexts correctly|

### Exception Propagation Rules

1. **Uncaught Exceptions**: If an exception is thrown and not caught, the VM must transition to FAULT state
2. **Catch Block Execution**: When an exception is thrown, execution must jump to the catch block if available
3. **Finally Block Execution**: Finally blocks must always execute, even if an exception is caught or not thrown
4. **Nested Exception Handling**: Exceptions in catch/finally blocks follow the same rules for outer exception contexts
5. **Context Unwinding**: When an exception crosses execution context boundaries, all finally blocks in exited contexts must execute

## Function Call Requirements

### Call Stack Management

1. **Call Depth**: The Rust VM must maintain identical call stack depth and structure
2. **Local Variables**: Local variable scope and lifetime must match exactly
3. **Argument Passing**: Arguments must be passed and accessed identically
4. **Return Values**: Return value handling must be identical, particularly for the RVCount mechanism
5. **Static Fields**: Static field access and lifetime must match between implementations

### Function Call Edge Cases

1. **Recursive Calls**: Must handle recursive calls with identical stack usage and overflow behavior
2. **Tail Calls**: Must handle tail call optimization identically if implemented
3. **Cross-Context Calls**: Calls between different execution contexts must behave identically

## Data Structure Operation Requirements

### Array Operations

| Operation   | Compatibility Requirements                                                          |
|-------------|------------------------------------------------------------------------------------|
| Creation    | Array creation and initialization must match C# implementation                      |
| Access      | Array element access and bounds checking must be identical                          |
| Modification| Array element modification must preserve reference semantics identically            |

### Map Operations

| Operation   | Compatibility Requirements                                                          |
|-------------|------------------------------------------------------------------------------------|
| Creation    | Map creation and initialization must match C# implementation                        |
| Key Handling| Key comparison and hashing must be identical                                        |
| Value Access| Map value access must match C# implementation, including missing key behavior       |
| Modification| Map value modification must preserve reference semantics identically                |

### Struct Operations

Struct operations must match Array operations, with the addition of reference vs. value semantics rules specific to Struct types.

## Implementation Verification

To verify that complex script execution is identical between the C# and Rust VMs, we employ the following methods:

1. **Script Test Suite**: A comprehensive set of test scripts that exercise all complex operations
2. **State Verification**: Verification of VM state, stack contents, and execution results after each operation
3. **Cross-Implementation Testing**: Running identical scripts on both VMs and comparing results
4. **Edge Case Testing**: Specific tests for edge cases in control flow, exception handling, and data structures

## Implementation Status

| Feature Area                 | Status      | Notes                                             |
|------------------------------|-------------|---------------------------------------------------|
| Arithmetic Operations        | Complete    | Verified with unit and integration tests          |
| Control Flow Operations      | Complete    | Verified with extensive testing                   |
| Function Calls               | Complete    | Verified with recursive and complex call patterns |
| Exception Handling           | Complete    | Verified with nested exception handling tests     |
| Array/Struct Operations      | Complete    | Verified with complex data structure tests        |
| Map Operations               | Complete    | Verified with complex key/value scenarios         |
| Cross-Implementation Testing | In Progress | Ongoing for complex scenarios                     |

## Known Differences

At present, there are no known behavioral differences between the C# and Rust implementations for complex script execution. Any differences discovered during testing will be documented here along with their resolution status.

## Next Steps

1. **Complete verification testing** for all complex script scenarios
2. **Document any intentional differences** if they arise due to language-specific optimizations
3. **Create a benchmark suite** for performance comparison while maintaining identical behavior
