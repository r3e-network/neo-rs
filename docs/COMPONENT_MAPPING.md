# Component Mapping: C# to Rust

## Overview
This document provides a detailed mapping between Neo C# components and their Rust equivalents, ensuring functional parity while leveraging Rust's type system and memory safety features.

## Core Type Mappings

### Fundamental Types

| C# Type | Rust Equivalent | Notes |
|---------|----------------|-------|
| `UInt160` | `neo_core::UInt160` | 20-byte hash type |
| `UInt256` | `neo_core::UInt256` | 32-byte hash type |
| `BigInteger` | `num_bigint::BigInt` | Arbitrary precision integer |
| `byte[]` | `Vec<u8>` or `&[u8]` | Dynamic or borrowed byte array |
| `ReadOnlyMemory<byte>` | `&[u8]` | Immutable byte slice |
| `Memory<byte>` | `&mut [u8]` | Mutable byte slice |
| `ECPoint` | `neo_cryptography::ECPoint` | Elliptic curve point |
| `KeyPair` | `neo_cryptography::KeyPair` | Public/private key pair |

### Collection Types

| C# Type | Rust Equivalent | Notes |
|---------|----------------|-------|
| `List<T>` | `Vec<T>` | Dynamic array |
| `Dictionary<K,V>` | `HashMap<K,V>` | Hash map |
| `HashSet<T>` | `HashSet<T>` | Hash set |
| `ReadOnlyCollection<T>` | `&[T]` | Immutable slice |
| `IEnumerable<T>` | `Iterator<Item=T>` | Iterator trait |

## Module Mappings

### Neo Core → neo-core

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `UInt160` | `neo_core::UInt160` | `crates/core/src/uint160.rs` |
| `UInt256` | `neo_core::UInt256` | `crates/core/src/uint256.rs` |
| `Fixed8` | `neo_core::Fixed8` | `crates/core/src/fixed8.rs` |
| `Helper` | `neo_core::helper` | `crates/core/src/helper.rs` |
| `Utility` | `neo_core::utility` | `crates/core/src/utility.rs` |

### Neo.VM → neo-vm

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `ExecutionEngine` | `neo_vm::ExecutionEngine` | `crates/vm/src/execution_engine.rs` |
| `ExecutionContext` | `neo_vm::ExecutionContext` | `crates/vm/src/execution_context.rs` |
| `EvaluationStack` | `neo_vm::EvaluationStack` | `crates/vm/src/evaluation_stack.rs` |
| `StackItem` | `neo_vm::StackItem` | `crates/vm/src/stack_item.rs` |
| `OpCode` | `neo_vm::OpCode` | `crates/vm/src/opcode.rs` |
| `Script` | `neo_vm::Script` | `crates/vm/src/script.rs` |
| `VMState` | `neo_vm::VMState` | `crates/vm/src/vm_state.rs` |

### Neo.Cryptography → neo-cryptography

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `ECDsa` | `neo_cryptography::ECDsa` | `crates/cryptography/src/ecdsa.rs` |
| `ECPoint` | `neo_cryptography::ECPoint` | `crates/cryptography/src/ec_point.rs` |
| `KeyPair` | `neo_cryptography::KeyPair` | `crates/cryptography/src/key_pair.rs` |
| `Crypto` | `neo_cryptography::crypto` | `crates/cryptography/src/crypto.rs` |
| `Helper` | `neo_cryptography::helper` | `crates/cryptography/src/helper.rs` |
| `Murmur32` | `neo_cryptography::Murmur32` | `crates/cryptography/src/murmur32.rs` |
| `BloomFilter` | `neo_cryptography::BloomFilter` | `crates/cryptography/src/bloom_filter.rs` |

### Neo.IO → neo-io

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `ISerializable` | `neo_io::Serializable` | `crates/io/src/serializable.rs` |
| `BinaryReader` | `neo_io::BinaryReader` | `crates/io/src/binary_reader.rs` |
| `BinaryWriter` | `neo_io::BinaryWriter` | `crates/io/src/binary_writer.rs` |
| `MemoryReader` | `neo_io::MemoryReader` | `crates/io/src/memory_reader.rs` |
| `MemoryWriter` | `neo_io::MemoryWriter` | `crates/io/src/memory_writer.rs` |

### Neo.Network → neo-network

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `LocalNode` | `neo_network::LocalNode` | `crates/network/src/local_node.rs` |
| `RemoteNode` | `neo_network::RemoteNode` | `crates/network/src/remote_node.rs` |
| `Message` | `neo_network::Message` | `crates/network/src/message.rs` |
| `Payload` | `neo_network::Payload` | `crates/network/src/payload.rs` |
| `ProtocolSettings` | `neo_network::ProtocolSettings` | `crates/network/src/protocol_settings.rs` |

### Neo.Persistence → neo-persistence

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `IStore` | `neo_persistence::Store` | `crates/persistence/src/store.rs` |
| `ISnapshot` | `neo_persistence::Snapshot` | `crates/persistence/src/snapshot.rs` |
| `DataCache` | `neo_persistence::DataCache` | `crates/persistence/src/data_cache.rs` |
| `StoreView` | `neo_persistence::StoreView` | `crates/persistence/src/store_view.rs` |

### Neo.SmartContract → neo-smart-contract

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `ApplicationEngine` | `neo_smart_contract::ApplicationEngine` | `crates/smart_contract/src/application_engine.rs` |
| `Contract` | `neo_smart_contract::Contract` | `crates/smart_contract/src/contract.rs` |
| `ContractState` | `neo_smart_contract::ContractState` | `crates/smart_contract/src/contract_state.rs` |
| `InteropService` | `neo_smart_contract::InteropService` | `crates/smart_contract/src/interop_service.rs` |
| `Manifest` | `neo_smart_contract::Manifest` | `crates/smart_contract/src/manifest.rs` |
| `NativeContract` | `neo_smart_contract::NativeContract` | `crates/smart_contract/src/native_contract.rs` |

### Neo.Ledger → neo-ledger

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `Blockchain` | `neo_ledger::Blockchain` | `crates/ledger/src/blockchain.rs` |
| `Block` | `neo_ledger::Block` | `crates/ledger/src/block.rs` |
| `Header` | `neo_ledger::Header` | `crates/ledger/src/header.rs` |
| `Transaction` | `neo_ledger::Transaction` | `crates/ledger/src/transaction.rs` |
| `MemoryPool` | `neo_ledger::MemoryPool` | `crates/ledger/src/memory_pool.rs` |
| `Witness` | `neo_ledger::Witness` | `crates/ledger/src/witness.rs` |

### Neo.Wallets → neo-wallets

| C# Class/Interface | Rust Equivalent | Location |
|-------------------|----------------|----------|
| `Wallet` | `neo_wallets::Wallet` | `crates/wallets/src/wallet.rs` |
| `WalletAccount` | `neo_wallets::WalletAccount` | `crates/wallets/src/wallet_account.rs` |
| `KeyPair` | `neo_wallets::KeyPair` | `crates/wallets/src/key_pair.rs` |
| `NEP6Wallet` | `neo_wallets::NEP6Wallet` | `crates/wallets/src/nep6_wallet.rs` |

## Design Patterns and Idioms

### Error Handling

| C# Pattern | Rust Pattern | Example |
|------------|-------------|---------|
| Exceptions | `Result<T, E>` | `fn parse() -> Result<Block, ParseError>` |
| `try-catch` | `?` operator | `let block = parse_block(data)?;` |
| `ArgumentException` | `InvalidInput` error | Custom error types |

### Memory Management

| C# Pattern | Rust Pattern | Notes |
|------------|-------------|-------|
| Garbage Collection | Ownership System | Automatic memory management |
| `IDisposable` | `Drop` trait | Resource cleanup |
| Reference types | `Box<T>`, `Rc<T>`, `Arc<T>` | Heap allocation |
| Value types | Stack allocation | Default in Rust |

### Async Programming

| C# Pattern | Rust Pattern | Notes |
|------------|-------------|-------|
| `async/await` | `async/await` | Similar syntax |
| `Task<T>` | `Future<Output=T>` | Async computation |
| `CancellationToken` | `tokio::select!` | Cancellation handling |

### Serialization

| C# Pattern | Rust Pattern | Notes |
|------------|-------------|-------|
| `ISerializable` | `Serializable` trait | Custom trait |
| `BinaryFormatter` | `bincode` | Binary serialization |
| `JsonSerializer` | `serde_json` | JSON serialization |

## Interface Adaptations

### C# Interfaces to Rust Traits

| C# Interface | Rust Trait | Purpose |
|-------------|------------|---------|
| `ISerializable` | `Serializable` | Binary serialization |
| `IEquatable<T>` | `PartialEq<T>` | Equality comparison |
| `IComparable<T>` | `PartialOrd<T>` | Ordering comparison |
| `ICloneable` | `Clone` | Object cloning |
| `IDisposable` | `Drop` | Resource cleanup |

### Abstract Classes to Traits

| C# Abstract Class | Rust Trait | Implementation |
|------------------|------------|----------------|
| `Payload` | `Payload` | Message payload trait |
| `NativeContract` | `NativeContract` | Native contract trait |
| `StackItem` | `StackItem` | VM stack item trait |

## Conversion Guidelines

### Naming Conventions

1. **Types**: PascalCase → PascalCase (unchanged)
2. **Functions**: PascalCase → snake_case
3. **Constants**: UPPER_CASE → UPPER_CASE (unchanged)
4. **Modules**: PascalCase → snake_case

### Type Safety Improvements

1. **Null Safety**: Replace nullable types with `Option<T>`
2. **Error Handling**: Replace exceptions with `Result<T, E>`
3. **Memory Safety**: Use ownership system instead of GC
4. **Thread Safety**: Use `Arc<Mutex<T>>` for shared mutable state

### Performance Considerations

1. **Zero-Copy**: Use `&[u8]` instead of `Vec<u8>` where possible
2. **Stack Allocation**: Prefer stack over heap allocation
3. **Lazy Evaluation**: Use iterators instead of collecting into vectors
4. **SIMD**: Leverage SIMD instructions for cryptographic operations

## Testing Strategy

### Unit Test Conversion

1. **Test Structure**: Convert NUnit tests to Rust `#[test]` functions
2. **Assertions**: Map C# assertions to Rust `assert!` macros
3. **Test Data**: Convert test fixtures to Rust test data
4. **Mocking**: Use `mockall` crate for mocking dependencies

### Integration Testing

1. **Network Tests**: Test P2P protocol compatibility
2. **Consensus Tests**: Validate consensus behavior
3. **Storage Tests**: Test persistence layer functionality
4. **VM Tests**: Validate virtual machine execution

This mapping document serves as the foundation for the conversion process, ensuring that all C# functionality is properly represented in the Rust implementation while taking advantage of Rust's unique features and safety guarantees. 