# Core Module Conversion

This document details the conversion of the Neo N3 C# Core module to Rust.

## Module Overview

The Core module provides fundamental types and utilities for the Neo blockchain, including:

- Basic blockchain types (UInt160, UInt256)
- Transaction building and signing
- Event handling
- System configuration
- Extension methods

## Type Mappings

| C# Type | Rust Type | Notes |
|---------|-----------|-------|
| `UInt160` | `UInt160` struct | 160-bit unsigned integer (20 bytes) |
| `UInt256` | `UInt256` struct | 256-bit unsigned integer (32 bytes) |
| `BigDecimal` | Custom `BigDecimal` struct | Decimal with arbitrary precision |
| `TransactionBuilder` | `TransactionBuilder` struct | Builder pattern for transactions |
| `NeoSystem` | `NeoSystem` struct | Core system for Neo blockchain |
| Extension methods | Trait implementations | C# extension methods become trait implementations |

## File Mappings

| C# File | Rust File | Implementation Status |
|---------|-----------|------------------------|
| `BigDecimal.cs` | `big_decimal.rs` | 🔴 Not Started |
| `Builders/TransactionBuilder.cs` | `builders/transaction_builder.rs` | 🔴 Not Started |
| `Builders/SignerBuilder.cs` | `builders/signer_builder.rs` | 🔴 Not Started |
| `Builders/WitnessBuilder.cs` | `builders/witness_builder.rs` | 🔴 Not Started |
| `ContainsTransactionType.cs` | `transaction_type.rs` | 🔴 Not Started |
| `Extensions/ByteExtensions.cs` | `extensions/byte_extensions.rs` | 🔴 Not Started |
| `Extensions/UInt160Extensions.cs` | `extensions/uint160_extensions.rs` | 🔴 Not Started |
| `Hardfork.cs` | `hardfork.rs` | 🔴 Not Started |
| `IEventHandlers/*.cs` | `events/*.rs` | 🔴 Not Started |
| `NeoSystem.cs` | `neo_system.rs` | 🔴 Not Started |

## Detailed Conversion Notes

### UInt160/UInt256

**C# Implementation:**
- Fixed-size unsigned integers
- Used for addresses, transaction hashes, etc.
- Serialization/deserialization support

**Rust Implementation:**
- Custom structs with fixed-size arrays
- Implement `From`/`TryFrom` for conversions
- Implement serialization traits
- Optimize for performance

### BigDecimal

**C# Implementation:**
- Arbitrary precision decimal
- Used for token balances and calculations

**Rust Implementation:**
- Custom implementation or use `rust_decimal` crate
- Ensure compatibility with Neo serialization format
- Implement mathematical operations

### Transaction Building

**C# Implementation:**
- Builder pattern for transaction creation
- Support for different transaction types
- Witness and signer management

**Rust Implementation:**
- Implement builder pattern with Rust idioms
- Use method chaining
- Ensure type safety with Rust's type system

### NeoSystem

**C# Implementation:**
- Core system for Neo blockchain
- Component registration and management
- Event handling

**Rust Implementation:**
- Struct with component management
- Event system using callbacks or observers
- Consider using dependency injection pattern

### Event Handling

**C# Implementation:**
- Interface-based event handlers
- Event subscription and notification

**Rust Implementation:**
- Trait-based event handlers
- Consider using `tokio::sync::broadcast` for event distribution
- Implement observer pattern

## Dependencies

- `num-bigint`: For arbitrary-precision integers
- `rust_decimal` (optional): For decimal arithmetic
- `thiserror`: For error handling
- `derive_more`: For deriving common traits

## Testing Strategy

1. Convert all C# unit tests to Rust
2. Add additional tests for Rust-specific edge cases
3. Test serialization/deserialization compatibility
4. Benchmark performance against C# implementation
