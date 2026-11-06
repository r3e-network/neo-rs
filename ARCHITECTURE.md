# Neo-rs Architecture

> **Version**: 0.17.0  
> **Last Updated**: 2026-09-05  
> **Target Compatibility**: Neo N3 v3.10.1

This document describes the architecture of the neo-rs project, a professional Rust implementation of the Neo N3 blockchain node. Crate composition below reflects the actual workspace members in the root `Cargo.toml`.

📖 **For comprehensive architecture documentation, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** which includes:
- Detailed system overview with architecture diagrams
- Complete core component documentation (VM, Storage, P2P, Consensus)
- Data flow diagrams (Transaction lifecycle, Block processing, State management)
- Module structure and dependency graphs
- Security architecture (Cryptography, Verification pipelines)
- Native contract reference and glossary

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Layered Architecture](#layered-architecture)
- [Dependency Rules](#dependency-rules)
- [Crate Organization](#crate-organization)
- [API Design Principles](#api-design-principles)
- [Error Handling Strategy](#error-handling-strategy)
- [Module Organization Guidelines](#module-organization-guidelines)
- [C# Compatibility Matrix](#c-compatibility-matrix)

---

## Architecture Overview

Neo-rs follows a **strict layered architecture** with clear dependency boundaries. The architecture is designed to:

1. **Maintain C# compatibility**: Byte-for-byte serialization parity with the official Neo C# implementation
2. **Enable modularity**: Each layer can be tested and developed independently
3. **Support multiple deployment scenarios**: From lightweight clients to full consensus nodes
4. **Ensure type safety**: Leverage Rust's type system for compile-time correctness guarantees

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         APPLICATION LAYER (Layer 3)                         │
│                                                                             │
│   ┌─────────────────────┐         ┌─────────────────────┐                   │
│   │     neo-node        │         │  neo-rpc (client)   │                   │
│   │  (Node Daemon)      │         │  (RpcClient API)    │                   │
│   │                     │         │                     │                   │
│   │  • P2P networking   │         │  • JSON-RPC calls   │                   │
│   │  • RPC server       │         │  • Wallet helpers   │                   │
│   │  • Consensus        │         │  • Node queries     │                   │
│   └─────────────────────┘         └─────────────────────┘                   │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         SERVICE LAYER (Layer 2)                             │
│                                                                             │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│   │ neo-system   │  │ neo-config   │  │  neo-tee     │  │  neo-hsm     │    │
│   │              │  │              │  │  (optional)  │  │  (optional)  │    │
│   │ Node Runtime │  │ Configuration│  │              │  │              │    │
│   │ Mempool/State│  │ Protocol     │  │              │  │              │    │
│   │ /Oracle/Logs │  │              │  │              │  │              │    │
│   └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘    │
│   ┌──────────────┐                                                          │
│   │neo-telemetry │                                                          │
│   │  (optional)  │                                                          │
│   └──────────────┘                                                          │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         CORE LAYER (Layer 1)                                │
│                                                                             │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│   │neo-protocol  │  │   neo-vm     │  │   neo-p2p    │  │neo-consensus │   │
│   │              │  │              │  │              │  │              │    │
│   │ • Protocol   │  │ • Stack VM   │  │ • Messages   │  │ • dBFT 2.0   │    │
│   │ • Network    │  │ • OpCodes    │  │ • Payload    │  │ • Consensus  │    │
│   │ • Payload    │  │ • Debugging  │  │ • Peers      │  │   state      │    │
│   │              │  │ • Interop    │  │              │  │              │    │
│   └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘    │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│   │  neo-ledger  │  │neo-smart-    │  │ neo-wallets  │  │   neo-rpc    │    │
│   │              │  │  contract    │  │              │  │              │    │
│   │ • Block      │  │ • Contract   │  │ • KeyPair    │  │ • JSON-RPC   │    │
│   │ • Transaction│  │ • Manifest   │  │ • Signing    │  │ • Client/    │    │
│   │ • Witness    │  │ • Native     │  │ • NEP-6      │  │   Server     │    │
│   └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                      FOUNDATION LAYER (Layer 0)                             │
│                                                                             │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│   │neo-primitives│  │  neo-crypto  │  │ neo-storage  │  │    neo-io    │    │
│   │              │  │              │  │              │  │              │    │
│   │ • UInt160    │  │ • SHA256     │  │ • IStore     │  │ • Binary RW  │    │
│   │ • UInt256    │  │ • Hash160/256│  │ • Snapshot   │  │ • Serialize  │    │
│   │ • BigDecimal │  │ • ECC types  │  │ • Cache      │  │ • Caching    │    │
│   │ • Hardfork   │  │ • MPT Trie   │  │ • Seek       │  │              │    │
│   └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘    │
│                                                                             │
│   ┌──────────────┐                                                          │
│   │  neo-json    │                                                          │
│   │              │                                                          │
│   │ • JToken     │                                                          │
│   │ • JObject    │                                                          │
│   │ • JPath      │                                                          │
│   └──────────────┘                                                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Layered Architecture

### Layer 0: Foundation Layer

The foundation layer contains **zero dependencies** on other neo-* crates. These are pure, reusable building blocks.

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `neo-primitives` | Core types | `UInt160`, `UInt256`, `BigDecimal`, `Hardfork` |
| `neo-crypto` | Cryptography | `Crypto`, `ECPoint`, `HashAlgorithm`, `MPT Trie` |
| `neo-storage` | Storage traits | `IReadOnlyStore`, `IWriteStore`, `IStore`, `StorageKey` |
| `neo-io` | Serialization | `BinaryReader`, `BinaryWriter`, `Serializable` |
| `neo-json` | JSON handling | `JToken`, `JObject`, `JArray`, `JPath` |

### Layer 1: Core Layer

The core layer implements blockchain protocol logic. It depends only on the Foundation layer.

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `neo-protocol` | Protocol types and network payload | `ProtocolSettings`, `Payload`, `NetworkMessage` |
| `neo-ledger` | Blockchain ledger | `Block`, `Transaction`, `Witness`, `Header` |
| `neo-smart-contract` | Smart contract types | `Contract`, `ContractManifest`, `NativeContract` |
| `neo-wallets` | Wallet and key management | `Wallet`, `KeyPair`, `Account`, `NEP6Wallet` |
| `neo-vm` | Virtual machine (opcodes, interpreter, limits) | `OpCode`, `ExecutionEngine`, `StackItem`, `Script` |
| `neo-p2p` | P2P networking | `MessageCommand`, `InventoryType`, `VerifyResult` |
| `neo-consensus` | dBFT consensus | `ConsensusService`, `ConsensusContext`, `ConsensusMessage` |
| `neo-rpc` | RPC communication | `RpcServer`, `RpcClient`, `RpcErrorCode` |

### Layer 2: Service Layer

The service layer provides higher-level blockchain services and orchestration. Transaction pool, state service, oracle, and application-log services live inside `neo-system` which provides the node runtime.

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `neo-system` | Node runtime, blockchain orchestration, P2P networking | `NeoSystem`, `Blockchain`, `MemoryPool`, `LocalNode`, `RemoteNode` |
| `neo-config` | Configuration | `ProtocolSettings`, `NodeConfig` |
| `neo-telemetry` | Observability | `Metrics`, `HealthCheck`, `Tracing` |
| `neo-tee` | TEE support | `EnclaveClient` (feature-gated) |
| `neo-hsm` | HSM support | `HsmSigner` (feature-gated) |

### Layer 3: Application Layer

The application layer contains user-facing binaries.

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `neo-node` | Full node daemon | `NeoNode`, `RpcServer` |

---

## Dependency Rules

### The Golden Rule

> **Each layer may only depend on layers below it. Never depend upward.**

### Allowed Dependencies

```
Layer 3 (Application)  ───────────────────► Layer 2, Layer 1, Layer 0
Layer 2 (Service)      ───────────────────► Layer 1, Layer 0
Layer 1 (Core)         ───────────────────► Layer 0
Layer 0 (Foundation)   ───────────────────► No neo-* dependencies
```

### Forbidden Patterns

```rust
// ❌ WRONG: Layer 0 depending on Layer 1
// neo-primitives/Cargo.toml:
[dependencies]
neo-ledger = { path = "../neo-ledger" }  // FORBIDDEN

// ❌ WRONG: Circular dependencies
// neo-ledger/Cargo.toml:
neo-p2p = { path = "../neo-p2p" }

// neo-p2p/Cargo.toml:
neo-ledger = { path = "../neo-ledger" }  // FORBIDDEN - creates cycle

// ❌ WRONG: Layer jumping
// neo-node (Layer 3) directly using neo-primitives (Layer 0)
// While technically allowed, prefer going through Layer 2 abstractions
```

### Correct Patterns

```rust
// ✅ CORRECT: Foundation layer is dependency-free
// neo-primitives/Cargo.toml:
[dependencies]
serde = "1.0"  // External crates only

// ✅ CORRECT: Core layer depends on Foundation
// neo-ledger/Cargo.toml:
[dependencies]
neo-primitives = { path = "../neo-primitives" }
neo-crypto = { path = "../neo-crypto" }
neo-io = { path = "../neo-io" }

// ✅ CORRECT: Service layer depends on Core and Foundation
// neo-system/Cargo.toml:
[dependencies]
neo-ledger = { path = "../neo-ledger" }
neo-protocol = { path = "../neo-protocol" }
neo-primitives = { path = "../neo-primitives" }
```

---

## Crate Organization

### Foundation Crates

#### neo-primitives

Core primitive types used throughout the Neo ecosystem.

```rust
// Key types
pub struct UInt160([u8; 20]);  // Script hashes, addresses
pub struct UInt256([u8; 32]);  // Transaction/block hashes
pub struct BigDecimal { ... }   // Financial calculations
pub enum Hardfork { ... }       // Protocol upgrades

// Zero external neo-* dependencies
```

#### neo-crypto

Cryptographic primitives with CSPRNG requirements.

```rust
// Hash functions
pub struct Crypto;
impl Crypto {
    pub fn sha256(data: &[u8]) -> [u8; 32];
    pub fn hash160(data: &[u8]) -> [u8; 20];
    pub fn hash256(data: &[u8]) -> [u8; 32];
}

// Elliptic curve types
pub enum ECCurve { Secp256r1, Secp256k1, Ed25519 }
pub struct ECPoint { ... }

// MPT Trie for state storage
pub struct Trie { ... }
```

#### neo-storage

Storage abstractions that break circular dependencies.

```rust
// Core traits
pub trait IReadOnlyStore { ... }
pub trait IWriteStore { ... }
pub trait IStore: IReadOnlyStore + IWriteStore { ... }

// Storage primitives
pub struct StorageKey { ... }
pub struct StorageItem { ... }
pub enum SeekDirection { Forward, Backward }
```

### Core Crates

#### neo-protocol

Protocol types and network payload definitions.

```rust
// Protocol settings and configuration
pub struct ProtocolSettings { ... }

// Network payload types
pub enum Payload { ... }
pub struct NetworkMessage { ... }
```

#### neo-ledger

Blockchain ledger primitives.

```rust
// Core ledger types
pub struct Block { ... }
pub struct Transaction { ... }
pub struct Witness { ... }
pub struct Header { ... }
```

#### neo-smart-contract

Smart contract types and native contract definitions.

```rust
// Contract types
pub struct Contract { ... }
pub struct ContractManifest { ... }

// Native contracts
pub mod native {
    pub struct NeoToken { ... }
    pub struct GasToken { ... }
}
```

#### neo-wallets

Wallet and key management.

```rust
// Wallet types
pub struct Wallet { ... }
pub struct KeyPair { ... }
pub struct Account { ... }

// NEP-6 wallet format
pub struct NEP6Wallet { ... }
```

#### neo-vm

Neo Virtual Machine implementation.

```rust
// Core VM types
pub struct ExecutionEngine { ... }
pub struct EvaluationStack { ... }
pub struct ExecutionContext { ... }

// Script handling
pub struct Script { ... }
pub struct ScriptBuilder { ... }

// VM values
pub enum StackItem { ... }
pub enum VMState { HALT, FAULT, BREAK, ... }
pub enum OpCode { ... }
```

#### neo-p2p

P2P networking protocol types.

```rust
// P2P message types
pub enum MessageCommand { ... }
pub enum InventoryType { ... }
pub enum VerifyResult { ... }

// Protocol messages
pub struct VersionPayload { ... }
pub struct AddrPayload { ... }
```

#### neo-consensus

dBFT 2.0 consensus implementation.

```rust
// Consensus service
pub struct ConsensusService { ... }
pub struct ConsensusContext { ... }

// Consensus messages
pub enum ConsensusMessage { ... }
```

#### neo-rpc

JSON-RPC client and server.

```rust
// RPC server
pub struct RpcServer { ... }

// RPC client
pub struct RpcClient { ... }

// Error codes
pub enum RpcErrorCode { ... }
```

### Service Crates

#### neo-system

Node runtime providing blockchain orchestration, mempool, state management, and P2P networking.

```rust
// Core system
pub struct NeoSystem { ... }
pub struct Blockchain { ... }

// Memory pool
pub struct MemoryPool { ... }

// P2P networking
pub struct LocalNode { ... }
pub struct RemoteNode { ... }

// Services
pub struct StateService { ... }
pub struct OracleService { ... }
```

---

## API Design Principles

### 1. Type Safety First

Use newtypes to prevent unit confusion and ensure compile-time correctness.

```rust
// ✅ GOOD: Type-safe wrappers
pub struct BlockHeight(pub u32);
pub struct TimestampMs(pub u64);
pub struct Gas(pub i64);

// Usage prevents mixing incompatible values
fn process_block(height: BlockHeight, timestamp: TimestampMs) { ... }

process_block(BlockHeight(1000), Gas(500));  // Compile error!
```

### 2. Explicit Error Types

Each crate defines its own error enum using `thiserror`.

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LedgerError {
    #[error("Invalid block: {0}")]
    InvalidBlock(String),
    
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
    
    #[error(transparent)]
    Crypto(#[from] neo_crypto::CryptoError),
    
    #[error(transparent)]
    Io(#[from] neo_io::IoError),
}

pub type LedgerResult<T> = Result<T, LedgerError>;
```

### 3. Feature-Gated Complexity

Keep protocol crates lightweight and feature-gate heavy dependencies.

```rust
// In Cargo.toml
[features]
default = []
runtime = ["tokio", "async-trait"]  // Async runtime
monitoring = ["prometheus"]          // Metrics
storage = ["rocksdb"]                // Storage backends

// In lib.rs
#[cfg(feature = "runtime")]
pub mod async_api;

#[cfg(feature = "storage")]
pub mod storage_providers;
```

### 4. Trait-Based Abstractions

Define clear interfaces for pluggable components.

```rust
// Service trait pattern
#[async_trait]
pub trait LedgerService: Send + Sync {
    async fn get_block(&self, hash: &UInt256) -> Option<Block>;
    async fn get_block_by_height(&self, height: u32) -> Option<Block>;
    async fn get_current_height(&self) -> u32;
    async fn contains_block(&self, hash: &UInt256) -> bool;
}

// Implementation can be swapped for testing
pub struct RocksDbLedger { ... }
pub struct InMemoryLedger { ... }
```

### 5. Builder Pattern for Complex Types

```rust
// Complex construction
let transaction = TransactionBuilder::new()
    .version(0)
    .nonce(12345)
    .sender(sender_hash)
    .script(invocation_script)
    .witness_scope(WitnessScope::CALLED_BY_ENTRY)
    .build()?;
```

---

## Error Handling Strategy

### Layer-Specific Error Types

```
┌─────────────────────────────────────────────────────────────────┐
│                    Error Hierarchy                               │
├─────────────────────────────────────────────────────────────────┤
│  Application (neo-node)                                         │
│  └── NodeError                                                  │
│                                                                 │
│  Service (neo-system, neo-config, neo-tee, neo-hsm)             │
│  └── SystemError, ConfigError, TeeError, HsmError               │
│                                                                 │
│  Core (neo-ledger, neo-protocol, neo-p2p, neo-rpc, etc.)        │
│  └── LedgerError, ProtocolError, VmError, P2PError, RpcError    │
│                                                                 │
│  Foundation (neo-primitives, neo-crypto, neo-storage, neo-io)   │
│  └── PrimitiveError, CryptoError, StorageError, IoError         │
└─────────────────────────────────────────────────────────────────┘
```

### Error Conversion

```rust
// At layer boundaries, convert errors explicitly
impl From<CryptoError> for LedgerError {
    fn from(err: CryptoError) -> Self {
        LedgerError::Crypto(err)
    }
}

// RPC layer maps to JSON-RPC error codes
impl From<LedgerError> for RpcError {
    fn from(err: LedgerError) -> Self {
        match err {
            LedgerError::InvalidBlock(_) => RpcError::invalid_params(),
            LedgerError::VerificationFailed(_) => RpcError::internal_error(),
            _ => RpcError::internal_error(),
        }
    }
}
```

### Avoid Stringly-Typed Errors

```rust
// ❌ BAD: Vague error
return Err("something went wrong".into());

// ✅ GOOD: Structured error
return Err(LedgerError::VerificationFailed {
    hash: block_hash,
    reason: VerificationFailureReason::InvalidWitness,
});
```

---

## Module Organization Guidelines

### Module Naming

```rust
// Use snake_case for modules
pub mod smart_contract;  // ✅ GOOD
pub mod smartContract;   // ❌ BAD
pub mod SmartContract;   // ❌ BAD

// Module content organization
pub mod ledger {
    // Public API
    pub use block::Block;
    pub use transaction::Transaction;
    
    // Internal modules
    mod block;
    mod transaction;
    mod validation;
}
```

### Visibility Guidelines

```rust
// Default to private, expose intentionally
pub mod contracts {
    // Public types
    pub struct Contract { ... }
    pub struct ContractManifest { ... }
    
    // Implementation details - private
    mod parser {
        pub(super) fn parse_manifest(bytes: &[u8]) -> ContractManifest;
    }
    
    // Internal API - pub(crate)
    pub(crate) fn validate_contract(contract: &Contract) -> bool;
}
```

### Documentation Requirements

```rust
//! # Module Level Documentation
//!
//! One-sentence summary of the module's purpose.
//!
//! ## Detailed Description
//!
//! Longer explanation of what this module does and why it exists.
//!
//! ## Examples
//!
//! ```rust
//! use neo_ledger::Block;
//!
//! let block = Block::new(...);
//! ```

/// Short description of the item.
///
/// # Examples
///
/// ```rust
/// let value = my_function();
/// ```
///
/// # Errors
///
/// Returns an error if...
///
/// # Panics
///
/// Panics if...
pub fn my_function() -> Result<(), Error> { ... }
```

---

## C# Compatibility Matrix

### Namespace to Crate Mapping

| C# Namespace | Rust Crate | Rust Module |
|--------------|------------|-------------|
| `Neo` | `neo-protocol` | `neo_protocol` |
| `Neo.Cryptography` | `neo-crypto` | `neo_crypto` |
| `Neo.IO` | `neo-io` | `neo_io` |
| `Neo.Json` | `neo-json` | `neo_json` |
| `Neo.Ledger` | `neo-ledger` | `neo_ledger` |
| `Neo.Network.P2P` | `neo-p2p` | `neo_p2p` |
| `Neo.Network.P2P.Payloads` | `neo-protocol` | `neo_protocol::payloads` |
| `Neo.SmartContract` | `neo-smart-contract` | `neo_smart_contract` |
| `Neo.SmartContract.Native` | `neo-smart-contract` | `neo_smart_contract::native` |
| `Neo.VM` | `neo-vm` | `neo_vm` |
| `Neo.Wallets` | `neo-wallets` | `neo_wallets` |
| `Neo.Plugins.RpcServer` | `neo-rpc` | `neo_rpc::server` |
| `Neo.Plugins.DBFTPlugin` | `neo-consensus` | `neo_consensus` |
| `Neo` (node runtime) | `neo-system` | `neo_system` |

### Type Name Conversions

| C# Convention | Rust Convention |
|---------------|-----------------|
| `PascalCase` types | `PascalCase` types |
| `PascalCase` interfaces (`IVerifiable`) | `PascalCase` traits (`IVerifiable`) |
| `PascalCase` methods | `snake_case` methods |
| `PascalCase` properties | `snake_case` methods or fields |
| `PascalCase` enum variants | `PascalCase` enum variants |

### Protocol consistency (state roots)

Crate/API naming parity is not enough for node interoperability. Authoritative
rules for claiming Neo N3 state equivalence live in
[`docs/PROTOCOL_CONSISTENCY.md`](docs/PROTOCOL_CONSISTENCY.md). Current verified
range and open divergences:
[`docs/protocol-consistency/STATUS.md`](docs/protocol-consistency/STATUS.md).

Orchestrator: `scripts/verify-protocol-consistency.py`  
Goldens: `tests/fixtures/protocol_consistency/csharp_stateroot_goldens.jsonl`

---

## Testing Architecture

### Test Organization

```
neo-ledger/
├── src/
│   └── ...
└── tests/
    ├── unit/           # Unit tests
    ├── integration/    # Integration tests
    └── compatibility/  # C# parity tests
```

### Testing Layers

| Layer | Test Focus | Examples |
|-------|------------|----------|
| Foundation | Serialization, math correctness | `UInt256` parsing, hash functions |
| Core | State transitions, VM execution | Contract execution, block validation |
| Service | Component interaction | Mempool + chain integration |
| Application | End-to-end scenarios | Full node sync, CLI commands |

---

## Lock Ordering (Deadlock Prevention)

When multiple locks are needed, always acquire them in this order:

### NeoSystem Locks

1. `service_registry.services_by_name` (RwLock)
2. `service_registry.typed_services` (RwLock)
3. `service_registry.services` (RwLock)
4. `service_added_handlers` (RwLock)
5. `plugin_manager` (RwLock)
6. `self_ref` (Mutex)

### NeoSystemContext Locks

1. `system` (RwLock)
2. `current_wallet` (RwLock)
3. `wallet_changed_handlers` (RwLock)
4. `committing_handlers` (RwLock)
5. `committed_handlers` (RwLock)
6. `transaction_added_handlers` (RwLock)
7. `transaction_removed_handlers` (RwLock)
8. `log_handlers` (RwLock)
9. `logging_handlers` (RwLock)
10. `notify_handlers` (RwLock)
11. `memory_pool` (Mutex)

> **Critical**: Never hold a lock across an `.await` point.

---

## Conclusion

This architecture enables:

- **Modularity**: Each layer can be developed, tested, and deployed independently
- **Safety**: Rust's type system prevents common blockchain implementation errors
- **Compatibility**: Byte-for-byte parity with C# Neo N3 implementation
- **Performance**: Zero-cost abstractions and efficient data structures
- **Maintainability**: Clear boundaries and comprehensive documentation

For questions or clarifications, refer to the crate-specific documentation in each `lib.rs` file or the `/docs` directory.
