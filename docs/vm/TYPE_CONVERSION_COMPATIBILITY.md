# Type Conversion Compatibility Guide

This document defines the exact behavior that the Rust VM implementation must follow to match the C# VM's type conversion behavior. Type conversion is a critical operation in the VM, and ensuring exact compatibility is essential for guaranteeing that both VM implementations produce identical results.

## Overview

Type conversion in the Neo VM is performed using the `CONVERT` opcode, which takes a target type parameter. The VM must follow precise rules for converting between different types to ensure consistent behavior across implementations.

## Conversion Rules by Source Type

### Boolean to Other Types

| Source       | Target       | Result                               | Special Cases                      |
|--------------|--------------|--------------------------------------|-----------------------------------|
| `true`       | Integer      | `1`                                  |                                   |
| `false`      | Integer      | `0`                                  |                                   |
| `true`       | ByteString   | `0x01` (single byte with value 1)    |                                   |
| `false`      | ByteString   | `0x00` (single byte with value 0)    |                                   |
| Boolean      | Buffer       | Same as ByteString                   |                                   |
| Boolean      | Array/Struct | Invalid conversion, results in FAULT |                                   |
| Boolean      | Map          | Invalid conversion, results in FAULT |                                   |

### Integer to Other Types

| Source       | Target       | Result                               | Special Cases                      |
|--------------|--------------|--------------------------------------|-----------------------------------|
| Non-zero     | Boolean      | `true`                               |                                   |
| Zero         | Boolean      | `false`                              |                                   |
| Integer      | ByteString   | Little-endian byte representation     | Remove trailing zeros             |
| Integer      | Buffer       | Little-endian byte representation     | Preserve fixed size               |
| Integer      | Array/Struct | Invalid conversion, results in FAULT |                                   |
| Integer      | Map          | Invalid conversion, results in FAULT |                                   |

### ByteString/Buffer to Other Types

| Source       | Target       | Result                               | Special Cases                      |
|--------------|--------------|--------------------------------------|-----------------------------------|
| Non-empty    | Boolean      | `true`                               |                                   |
| Empty        | Boolean      | `false`                              |                                   |
| ByteString   | Integer      | Interpreted as little-endian number   | Empty string becomes 0            |
| ByteString   | Buffer       | Direct copy of bytes                 |                                   |
| Buffer       | ByteString   | Direct copy of bytes                 |                                   |
| ByteString   | Array/Struct | Invalid conversion, results in FAULT |                                   |
| ByteString   | Map          | Invalid conversion, results in FAULT |                                   |

### Array to Other Types

| Source       | Target       | Result                               | Special Cases                      |
|--------------|--------------|--------------------------------------|-----------------------------------|
| Array        | Boolean      | Invalid conversion, results in FAULT |                                   |
| Array        | Integer      | Invalid conversion, results in FAULT |                                   |
| Array        | ByteString   | Invalid conversion, results in FAULT |                                   |
| Array        | Struct       | Deep copy of all items as a Struct   | Preserves reference semantics     |
| Array        | Map          | Invalid conversion, results in FAULT |                                   |

### Struct to Other Types

| Source       | Target       | Result                               | Special Cases                      |
|--------------|--------------|--------------------------------------|-----------------------------------|
| Struct       | Boolean      | Invalid conversion, results in FAULT |                                   |
| Struct       | Integer      | Invalid conversion, results in FAULT |                                   |
| Struct       | ByteString   | Invalid conversion, results in FAULT |                                   |
| Struct       | Array        | Deep copy of all items as an Array   | Preserves reference semantics     |
| Struct       | Map          | Invalid conversion, results in FAULT |                                   |

### Map to Other Types

| Source       | Target       | Result                               | Special Cases                      |
|--------------|--------------|--------------------------------------|-----------------------------------|
| Map          | Boolean      | Invalid conversion, results in FAULT |                                   |
| Map          | Integer      | Invalid conversion, results in FAULT |                                   |
| Map          | ByteString   | Invalid conversion, results in FAULT |                                   |
| Map          | Array/Struct | Invalid conversion, results in FAULT |                                   |

## Implementation Requirements

The Rust VM implementation must adhere to the following requirements to ensure exact compatibility with the C# VM:

1. **Same Type Conversion:** Converting to the same type should return the original item.
2. **Error Handling:** Invalid conversions must result in a FAULT state with an appropriate exception.
3. **Integer Representation:** When converting between integers and byte strings, the little-endian byte order must be preserved.
4. **Reference Semantics:** When converting between Array and Struct types, reference semantics must be preserved.
5. **Exception Propagation:** Exceptions must propagate correctly during type conversion operations.

## Verification Strategy

Our verification strategy consists of:

1. **Unit Tests:** Comprehensive unit tests for each type conversion scenario.
2. **Compatibility Tests:** Tests that execute identical scripts in both C# and Rust VMs to validate consistent behavior.
3. **Edge Cases:** Tests for boundary conditions like empty collections, very large integers, and invalid conversions.

## Implementation Status

The following table summarizes the implementation status of type conversion compatibility:

| Conversion Type            | Status    | Notes                                       |
|----------------------------|-----------|---------------------------------------------|
| Boolean to other types     | Complete  | Verified with unit and integration tests    |
| Integer to other types     | Complete  | Verified with unit and integration tests    |
| ByteString to other types  | Complete  | Verified with unit and integration tests    |
| Array/Struct conversions   | Complete  | Verified with unit and integration tests    |
| Map conversions            | Complete  | Verified with unit and integration tests    |
| Invalid conversions        | Complete  | All properly trigger FAULT state            |
