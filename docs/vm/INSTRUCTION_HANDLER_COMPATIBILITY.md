# Instruction Handler Compatibility Guide

This document provides a detailed specification for ensuring instruction handlers in the Rust VM implementation produce exactly the same behavior as the C# reference implementation.

## Verification Process

For each instruction handler, the verification process should:

1. Execute the same operation in both the C# and Rust VMs
2. Compare stack state before and after execution
3. Compare VM state before and after execution
4. Verify identical behavior for edge cases
5. Verify identical error handling

## Critical Instruction Categories

### Arithmetic Operations

All arithmetic operations must behave identically, particularly:
- Integer overflow handling
- Division by zero behavior
- Behavior with large integers
- Type conversions during operations

### Type Conversions

Type conversion instructions must produce identical results, including:
- Conversions between primitive types
- Array/struct/map conversions
- Behavior with invalid conversions

### Control Flow

Control flow instructions must behave identically:
- Jump conditions
- Call and return semantics
- Exception handling flow

### Stack Manipulation

Stack operations must maintain identical stack state:
- Push and pop operations
- Duplicate and remove operations
- Swap and roll operations

## Implementation Verification

For each instruction handler, implement a verification test that:
1. Creates identical initial state in both VMs
2. Executes the instruction in both VMs
3. Compares resulting state for exact equivalence

## Testing Methodology

For the 212 OpCodes defined in Neo VM, prioritize verification as follows:

1. **Priority 1**: Arithmetic, flow control, and exception-related operations
2. **Priority 2**: Stack manipulation operations
3. **Priority 3**: Type conversion operations
4. **Priority 4**: Specialized operations

## Script-Based Verification Approach

Create test scripts that exercise each OpCode and run them on both VMs, comparing:
1. Final stack contents
2. VM state
3. Execution count/gas usage
4. Memory usage pattern

## Implementation Timeline

1. Develop verification framework (1 day)
2. Implement Priority 1 operation verification (2 days)
3. Implement Priority 2-4 operation verification (3 days)
4. Address any discrepancies found (2 days)

## Discrepancy Resolution Process

When a discrepancy is found:
1. Document the exact nature of the discrepancy
2. Determine which implementation is correct according to the NEO whitepaper
3. Update the Rust implementation to match C# behavior
4. Re-verify to ensure complete compatibility

## Reference Materials

- C# code repository: `neo-sharp/src/Neo.VM/JumpTable/*.cs`
- Rust code repository: `neo-rs/crates/vm/src/jump_table/*.rs`
- Neo VM Technical Specification
