# Neo VM Technical Specification

## Introduction

This document provides a detailed technical specification for the Neo Virtual Machine (VM) implementation in Rust, with explicit mapping to the C# reference implementation. It serves as the technical foundation for ensuring equivalent behavior across language implementations.

## 1. VM States

The VM can exist in one of the following states, which must be identical across implementations:

| State | C# Value | Rust Value | Description |
|-------|----------|------------|-------------|
| NONE | 0x00 | 0x00 | VM is not running or has not yet begun execution |
| HALT | 0x01 | 0x01 | VM has completed execution successfully |
| FAULT | 0x02 | 0x02 | VM has encountered an unhandled exception |
| BREAK | 0x04 | 0x04 | VM has hit a breakpoint |

## 2. OpCode Specification

The OpCode enumeration must use identical byte values across implementations. Each operation must maintain the same stack effects (items pushed and popped) and behavior.

### 2.1 Constants

| OpCode | Value | Operand Size | Description | Stack Effect |
|--------|-------|--------------|-------------|-------------|
| PUSHINT8 | 0x00 | 1 | Push a 1-byte signed integer | +1, 0 |
| PUSHINT16 | 0x01 | 2 | Push a 2-byte signed integer | +1, 0 |
| PUSHINT32 | 0x02 | 4 | Push a 4-byte signed integer | +1, 0 |
| PUSHINT64 | 0x03 | 8 | Push an 8-byte signed integer | +1, 0 |
| PUSHINT128 | 0x04 | 16 | Push a 16-byte signed integer | +1, 0 |
| PUSHINT256 | 0x05 | 32 | Push a 32-byte signed integer | +1, 0 |
| PUSHT | 0x08 | 0 | Push the boolean value true | +1, 0 |
| PUSHF | 0x09 | 0 | Push the boolean value false | +1, 0 |
| PUSHA | 0x0A | 4 | Push the address (uint) onto the stack | +1, 0 |
| PUSHNULL | 0x0B | 0 | Push the null reference onto the stack | +1, 0 |
| PUSHDATA1 | 0x0C | 1 + n | Push n bytes (1-byte length prefix) | +1, 0 |
| PUSHDATA2 | 0x0D | 2 + n | Push n bytes (2-byte length prefix) | +1, 0 |
| PUSHDATA4 | 0x0E | 4 + n | Push n bytes (4-byte length prefix) | +1, 0 |
| PUSHM1 | 0x0F | 0 | Push the integer -1 onto the stack | +1, 0 |
| PUSH0 - PUSH16 | 0x10-0x20 | 0 | Push the integer 0-16 onto the stack | +1, 0 |

### 2.2 Flow Control

| OpCode | Value | Operand Size | Description | Stack Effect |
|--------|-------|--------------|-------------|-------------|
| NOP | 0x21 | 0 | No operation | 0, 0 |
| JMP | 0x22 | 1 | Unconditionally jump to the target | 0, 0 |
| JMP_L | 0x23 | 4 | Unconditionally jump to the target (long) | 0, 0 |
| JMPIF | 0x24 | 1 | Jump if condition is true | 0, 1 |
| JMPIF_L | 0x25 | 4 | Jump if condition is true (long) | 0, 1 |
| JMPIFNOT | 0x26 | 1 | Jump if condition is false | 0, 1 |
| JMPIFNOT_L | 0x27 | 4 | Jump if condition is false (long) | 0, 1 |
| JMPEQ | 0x28 | 1 | Jump if equal | 0, 2 |
| JMPEQ_L | 0x29 | 4 | Jump if equal (long) | 0, 2 |
| JMPNE | 0x2A | 1 | Jump if not equal | 0, 2 |
| JMPNE_L | 0x2B | 4 | Jump if not equal (long) | 0, 2 |
| JMPGT | 0x2C | 1 | Jump if first > second | 0, 2 |
| JMPGT_L | 0x2D | 4 | Jump if first > second (long) | 0, 2 |
| JMPGE | 0x2E | 1 | Jump if first >= second | 0, 2 |
| JMPGE_L | 0x2F | 4 | Jump if first >= second (long) | 0, 2 |
| JMPLT | 0x30 | 1 | Jump if first < second | 0, 2 |
| JMPLT_L | 0x31 | 4 | Jump if first < second (long) | 0, 2 |
| JMPLE | 0x32 | 1 | Jump if first <= second | 0, 2 |
| JMPLE_L | 0x33 | 4 | Jump if first <= second (long) | 0, 2 |
| CALL | 0x34 | 1 | Call a method | *, * |
| CALL_L | 0x35 | 4 | Call a method (long) | *, * |
| RET | 0x36 | 0 | Return from method | 0, * |
| SYSCALL | 0x41 | 4 | Call interop method | *, * |

(Note: The full OpCode specification is extensive; this is a representative sample)

## 3. Stack Item Type System

The following stack item types must be implemented with consistent behavior:

| Type | C# | Rust | Description |
|------|-----|------|-------------|
| Boolean | Boolean | Boolean | True/false value |
| Integer | Integer | Integer | Arbitrary-precision integer |
| ByteString | ByteString | ByteString | Immutable byte sequence |
| Buffer | Buffer | Buffer | Mutable byte sequence |
| Array | Array | Array | Sequence of stack items |
| Struct | Struct | Struct | Similar to Array with value semantics |
| Map | Map | Map | Key-value collection |
| Pointer | Pointer | Pointer | Script position pointer |
| InteropInterface | InteropInterface | InteropInterface | Interface to external functions |
| Null | Null | Null | Null/none value |

### 3.1 Type Conversion Rules

| From Type | To Type | C# Behavior | Rust Behavior | Notes |
|-----------|---------|-------------|--------------|-------|
| Any | Boolean | Defined rules | Must match C# | See detailed conversion table below |
| Any | Integer | Type-specific | Must match C# | See detailed conversion table below |
| Any | ByteString | Type-specific | Must match C# | See detailed conversion table below |
| ... | ... | ... | ... | ... |

## 4. Execution Engine

The execution engine must implement these key components:

### 4.1 Execution Context

The execution context must track:
- Script being executed
- Instruction pointer
- Evaluation stack
- Static fields
- Local variables
- Try-catch state
- Alt stack (for temporary storage)

### 4.2 Execution Flow

1. Execute instruction at current position
2. Update instruction pointer
3. Check for exceptions
4. Handle control flow changes
5. Update VM state as appropriate

### 4.3 Limits and Constraints

| Limit | C# Default | Rust Default | Description |
|-------|------------|--------------|-------------|
| MaxStackSize | 2048 | 2048 | Maximum items on stack |
| MaxItemSize | 1024 * 1024 | 1024 * 1024 | Maximum size of a single item (bytes) |
| MaxInvocationStackSize | 1024 | 1024 | Maximum call depth |

### 4.4 Exception Handling

Exception handling must match C# behavior exactly:
- Exception types must be the same
- VM state transitions must be consistent
- Exception message format should match
- Uncaught exceptions must be stored and accessible

## 5. Interoperability Services

Interop services provide a bridge between the VM and host environment:

### 5.1 Interop Method Registration

- Method registration must follow the same pattern
- Method identifiers must be computed identically
- Parameter passing conventions must be consistent

### 5.2 Parameter Conversion

- Parameter conversion from stack items to native types must be consistent
- Return value conversion from native types to stack items must be consistent

## 6. Memory Management

### 6.1 Reference Counting

- Reference counting for complex objects must be implemented
- Circular reference detection and cleanup must match C# behavior
- Memory leaks must be prevented in both implementations

### 6.2 Object Lifecycle

- Object creation and disposal rules must be consistent
- Stack item cleanup on context unloading must match

## 7. Testing Verification

To ensure compatibility between implementations, the following test scenarios must pass identically:

1. Basic arithmetic operations
2. Control flow operations
3. Stack manipulation
4. Array and collection operations
5. Exception handling
6. Nested calls and returns
7. Interop service calls
8. Edge cases (stack limits, integer overflow, etc.)

## References

- Neo C# VM Implementation
- Neo VM Technical Specification
- Neo Whitepaper
