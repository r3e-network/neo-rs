# Core Module Implementation

This document details the implementation of the Neo N3 Core module in Rust.

## Overview

The Core module provides fundamental types and utilities for the Neo blockchain, including:

- Basic blockchain types (UInt160, UInt256)
- BigDecimal for arbitrary precision decimal arithmetic
- Transaction building and signing
- Event handling
- System configuration
- Extension methods

## Implementation Status

| Component | Status | Notes |
|-----------|--------|-------|
| UInt160 | ✅ Complete | Fully implemented with tests |
| UInt256 | ✅ Complete | Fully implemented with tests |
| BigDecimal | ✅ Complete | Fully implemented with tests |
| ContainsTransactionType | ✅ Complete | Enum implementation complete |
| Extensions | ✅ Complete | ByteExtensions and UInt160Extensions implemented |
| Hardfork | ✅ Complete | Basic implementation with manager |
| EventHandlers | ✅ Complete | Implemented as EventManager |
| NeoSystem | 🟡 In Progress | Basic structure implemented |
| Builders | 🟡 In Progress | Basic structure for TransactionBuilder, SignerBuilder, and WitnessBuilder |

## Implementation Details

### UInt160/UInt256

The UInt160 and UInt256 types are implemented as structs containing fixed-size byte arrays. They provide:

- Serialization/deserialization
- Comparison and equality
- Conversion to/from hexadecimal strings
- Ordering for use in collections

### BigDecimal

BigDecimal is implemented as a struct containing a BigInt value and a decimals field. It provides:

- Arbitrary precision decimal arithmetic
- Parsing from strings
- Formatting to strings
- Comparison and equality

### ContainsTransactionType

ContainsTransactionType is implemented as an enum with three variants:

- NotExist
- ExistsInPool
- ExistsInLedger

### Extensions

Extensions are implemented as traits:

- ByteExtensions: Extensions for byte arrays
- UInt160Extensions: Extensions for UInt160

### Hardfork

Hardfork support is implemented with:

- HardforkName enum for different hardforks
- HardforkManager for registering and checking hardforks
- Global instance for system-wide hardfork checks

### EventHandlers

Event handling is implemented with:

- EventHandler trait for event handlers
- EventManager for registering and triggering events

### NeoSystem

NeoSystem is implemented as a struct that:

- Manages protocol settings
- Provides service registration and retrieval
- Checks transaction existence and conflicts

### Builders

Builder patterns are implemented for:

- TransactionBuilder: For building transactions
- SignerBuilder: For building transaction signers
- WitnessBuilder: For building transaction witnesses

## Conversion Notes

### C# to Rust Type Mappings

| C# Type | Rust Type | Notes |
|---------|-----------|-------|
| `UInt160` | `UInt160` struct | 160-bit unsigned integer (20 bytes) |
| `UInt256` | `UInt256` struct | 256-bit unsigned integer (32 bytes) |
| `BigDecimal` | `BigDecimal` struct | Decimal with arbitrary precision |
| `ContainsTransactionType` | `ContainsTransactionType` enum | Transaction existence check result |
| Extension methods | Trait implementations | C# extension methods become trait implementations |
| `NeoSystem` | `NeoSystem` struct | Core system for Neo blockchain |
| Event handlers | Trait-based event system | C# events become trait-based event system |

### Differences from C# Implementation

- Rust implementation uses traits instead of interfaces
- Memory management is handled by Rust's ownership system
- Error handling uses Result instead of exceptions
- Concurrency is handled with RwLock instead of locks
- Builder pattern uses method chaining with self returns

## Future Work

- Complete transaction types and serialization
- Implement full transaction building and signing
- Add more comprehensive tests
- Integrate with other modules (VM, Ledger, etc.)
- Optimize performance for critical operations
