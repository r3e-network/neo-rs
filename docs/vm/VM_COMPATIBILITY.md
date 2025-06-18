# Neo VM Compatibility Documentation

## Overview

This document outlines the compatibility requirements and implementation status between the Neo N3 C# VM (`neo-sharp`) and the Rust VM (`neo-rs`). It serves as a guide for ensuring feature parity and behavioral consistency across implementations.

## VM Architecture

The Neo Virtual Machine is a stack-based VM designed to execute Neo smart contracts. Its key components include:

1. **Execution Engine**: Core VM execution logic
2. **Stack Management**: Evaluation stack for operand storage
3. **OpCode Implementation**: Instruction set implementation
4. **Type System**: Implementation of VM data types
5. **Interoperability Layer**: System calls and native functionality

## Implementation Status

| Component | C# Status | Rust Status | Compatibility Notes |
|-----------|-----------|-------------|---------------------|
| VM State | Complete | Complete | Values match between implementations |
| OpCode | Complete | Complete | Opcodes and values match |
| Stack Items | Complete | Complete | All types implemented with comparable semantics |
| Execution Engine | Complete | Complete | Core execution logic matches |
| Script | Complete | Complete | Script handling matches |
| Reference Counting | Complete | Complete | Memory management approach consistent |
| Exceptions | Complete | Complete | Exception handling semantics match |

## Conversion Tasks

1. **Documentation Review**:
   - [x] Compare C# and Rust VM implementations
   - [ ] Document any intentional divergences
   - [ ] Create comprehensive API documentation

2. **Compatibility Testing**:
   - [ ] Develop cross-implementation test suite
   - [ ] Verify identical results for standard operations
   - [ ] Test edge cases and error handling

3. **Performance Optimization**:
   - [ ] Benchmark critical VM operations
   - [ ] Identify performance bottlenecks
   - [ ] Implement optimizations while maintaining compatibility

## VM Type System Mapping

| C# Type | Rust Type | Notes |
|---------|-----------|-------|
| `Boolean` | `Boolean` | Both represent a boolean value |
| `Integer` | `Integer` | Big integer implementation |
| `ByteString` | `ByteString` | Immutable byte sequence |
| `Buffer` | `Buffer` | Mutable byte sequence |
| `Array` | `Array` | Sequence of stack items |
| `Struct` | `Struct` | Similar to Array with value semantics |
| `Map` | `Map` | Key-value collection |
| `Pointer` | `Pointer` | Script position reference |
| `InteropInterface` | `InteropInterface` | Native function interface |
| `Null` | `Null` | Null value representation |

## OpCode Compatibility

The OpCode enumeration values are maintained exactly the same between the C# and Rust implementations to ensure bytecode compatibility. Any differences in OpCode handling are documented here:

*No known divergences at this time*

## Behavioral Differences

This section documents any intentional behavioral differences between the implementations:

*No known behavioral differences at this time*

## Testing Strategy

To ensure compatibility between implementations:

1. **Unit Tests**: Each VM component has dedicated unit tests
2. **Integration Tests**: End-to-end tests exercising the entire VM
3. **Compatibility Tests**: Tests specifically designed to verify behavior matches the C# implementation
4. **Contract Tests**: Common Neo smart contracts are executed on both implementations and results compared

## Future Enhancements

Potential future improvements to the Rust VM implementation:

1. Performance optimizations specific to Rust
2. Enhanced error reporting
3. Additional debugging capabilities
4. Memory usage optimizations

## References

- [Neo C# VM Documentation](https://docs.neo.org/docs/en-us/reference/vm/overview.html)
- [Neo VM Technical Specifications](https://github.com/neo-project/proposals/blob/master/nep-11.mediawiki)
