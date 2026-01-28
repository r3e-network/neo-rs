# neo-rs Architecture

> **Note**: For comprehensive architectural documentation including detailed dependency rules,
> API design principles, error handling strategy, and module organization guidelines,
> see the main [`ARCHITECTURE.md`](../ARCHITECTURE.md) at the project root.

This document captures the current layering and service boundaries so new changes stay coherent.

## Layered Architecture

The codebase follows a strict layered architecture with clear dependency rules:

```
┌─────────────────────────────────────────────────────────────────┐
│                    APPLICATION LAYER                             │
│  neo-cli (CLI client)    neo-node (daemon runtime)              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     SERVICE LAYER                                │
│  neo-chain (chain state)   neo-mempool   neo-telemetry          │
│  neo-config (settings)     neo-tee (TEE support)                │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                       CORE LAYER                                 │
│  neo-core      neo-vm       neo-p2p      neo-consensus          │
│  neo-rpc (server + client) neo-state (world state)              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    FOUNDATION LAYER                              │
│  neo-primitives (UInt160, UInt256, BigDecimal)                  │
│  neo-crypto (hashing, ECC, signatures, MPT)                     │
│  neo-storage (storage traits and abstractions)                  │
│  neo-io (serialization, caching)                                │
│  neo-json (JSON handling)                                       │
└─────────────────────────────────────────────────────────────────┘
```

**Dependency Rule**: Each layer may only depend on layers below it. Never depend upward.

## Application Layer (binaries)

The application layer is where user-facing binaries live. These binaries orchestrate lower-level crates but
should not blur boundaries by directly reaching into internal node state when an RPC boundary exists.

- **neo-node**: The node daemon. Runs P2P sync and (optionally) the JSON-RPC server.
  Consensus (dBFT) is not wired into `neo-node` yet. It is designed to be deployed and managed like a service.
- **neo-cli**: The RPC client. A user-facing command line tool that connects to a running `neo-node`
  (or any compatible Neo JSON-RPC endpoint) to query state and submit transactions. It does not run a node.

## Crates and Responsibilities

### Foundation Layer (no neo-\* dependencies except within layer)

- **neo-primitives**: Core primitive types (`UInt160`, `UInt256`, `BigDecimal`), constants, and basic error types. Zero external neo-\* dependencies.
- **neo-crypto**: Cryptographic primitives (SHA256, RIPEMD160, Keccak256, Blake2, Hash160, Hash256), elliptic curve types (`ECCurve`, `ECPoint`). Depends only on `neo-primitives`.
- **neo-storage**: Storage trait abstractions (`IReadOnlyStore`, `IWriteStore`, `IStore`, `ISnapshot`), `StorageKey`, `StorageItem`, `SeekDirection`. Breaks circular dependencies between persistence and smart contracts.
- **neo-io**: Binary serialization (`BinaryReader`, `BinaryWriter`, `MemoryReader`), caching utilities. Matches C# `Neo.IO`.
- **neo-json**: JSON handling (`JToken`, `JObject`, `JArray`, `JPath`). Matches C# `Neo.Json`.

### Core Layer

- **neo-core**: Consensus-neutral protocol logic (ledger, state service, VM host integration, persistence adapters, smart contract execution engine, native contracts, service traits). Keep public APIs small; prefer `pub(crate)` where possible.
- **neo-vm**: NeoVM execution engine (stack machine, opcodes, script execution). Matches C# `Neo.VM`.
- **neo-p2p**: P2P networking layer (message types, peer management, connection handling). Matches C# `Neo.Network.P2P`.
- **neo-rpc**: Unified RPC server and client. JSON-RPC 2.0 implementation for Neo node communication. Use the `client` feature flag to enable RPC client functionality.
- **neo-consensus**: dBFT (Delegated Byzantine Fault Tolerance) consensus implementation. Matches C# consensus plugin.
- **neo-state**: World state abstraction layer providing account state, contract storage, snapshots, and rollback semantics. Storage-agnostic design allows pluggable backends.

> **Note**: BLS12-381 cryptographic operations use the `blst` crate directly (via neo-core) instead of a separate wrapper crate.
> **Note**: Extension utilities (formerly neo-extensions) are now integrated into `neo-core::extensions`.
> **Note**: Smart contract types (formerly neo-contract) are now integrated into `neo-core::smart_contract`.
> **Note**: Service traits (formerly neo-services) are now integrated into `neo-core::services`.

### Service Layer

- **neo-chain**: Chain state controller with block index management, fork choice rules, and chain reorganization handling. Uses pure tokio channels for inter-component communication.
- **neo-mempool**: Transaction mempool with priority-based ordering, fee validation, and capacity management.
- **neo-config**: Configuration management for network settings, protocol parameters, and genesis block definitions.
- **neo-telemetry**: Logging, metrics (Prometheus-compatible), and health check infrastructure.
- **neo-tee**: Enclave-facing utilities and optional mempool/wallet. Feature-gated; avoid leaking into core.

> **Note**: The actor framework (formerly neo-akka) has been replaced with pure tokio channels for better performance and simpler debugging. Plugin functionality (formerly neo-plugins) has been consolidated into neo-core and neo-node.

### Application Layer

- **neo-cli**: Thin command wrappers over `neo-rpc` (with `client` feature), no business logic.
- **neo-node**: Node runtime daemon integrating P2P networking, RPC server, and chain state management. Uses tokio channels for component communication. Supports:
    - Block synchronization from network peers
    - State root calculation and validation
    - Health checks and Prometheus metrics
    - Configurable via TOML files or environment variables

## Service Access

- Use the typed service traits in `neo_core::services` (e.g., `LedgerService`, `StateStoreService`, `MempoolService`, `PeerManagerService`, `RpcService`) instead of `Any` downcasts or string keys.
- `NeoSystemContext` registers core services in the internal registry by `TypeId` so callers should prefer the typed accessors (`ledger_service()`, `state_store()`, `mempool_service()`, `local_node_service()`) before falling back to names.
- Code that used to live in “plugin” crates has been consolidated into first-class crates (notably `neo-rpc` and `neo-consensus`). Avoid duplicating registry lookup logic; prefer typed accessors on `NeoSystemContext` and crate-local helpers where available.
- Block import treats store commit failures as fatal (no plugin notifications or cache updates happen after a failed commit), preventing RAM/disk divergence.
- Use `StoreTransaction` when persisting a batch of changes; avoid calling `StoreCache::commit` directly so commit failures propagate and stay observable. For state-service batching, use `StateStoreTransaction`.

## Services and Context

- Core service access is via `NeoSystemContext`; use typed accessors (e.g., `state_store()`, `ledger_service()`, `rpc_service()`) instead of `Any` downcasts. When introducing a new service, provide:
    - A trait for the required behaviour.
    - A typed accessor on `NeoSystemContext`/`NeoSystem` that also falls back to the canonical in-memory handle so callers aren't forced to know about registry names.
    - A readiness/health hook if it has external dependencies.
- State flow: blocks persist via `LedgerContext` → `StateStore` updates local root → network state-root extensible payloads verify/persist via the shared `StateStore` (with `StateRootVerifier` backed by `StoreCache`).
- Ledger hydration on startup is bounded (last ~2000 headers/blocks) to avoid loading the full chain into memory; older data stays accessible through the store accessors.
- Use `StoreTransaction` when persisting a batch of changes; avoid calling `StoreCache::commit` directly so commit failures propagate and stay observable.

## Error Handling

- Use `thiserror` enums per crate and convert at boundaries (e.g., RPC -> JSON-RPC codes, actor -> `CoreError`). Avoid stringly-typed errors.
- Prefer domain newtypes (`BlockHeight`, `TimestampMs`, `Gas`) to reduce unit mixups when adding new APIs.
- Each crate defines its own `Error` enum and `Result<T>` type alias.

## Concurrency and Safety

- Locks: `parking_lot` locks preferred; document lock order when multiple locks are taken. Avoid holding locks across async/await.
- IO/persistence: any blocking store calls from async paths should be pushed to `spawn_blocking` or actor threads.
- Avoid global mutable singletons; pass handles explicitly.

### Lock Ordering for NeoSystem

To prevent deadlocks, acquire locks in this order:

**NeoSystem locks:**

1. `service_registry.services_by_name` (RwLock)
2. `service_registry.typed_services` (RwLock)
3. `service_registry.services` (RwLock)
4. `service_added_handlers` (RwLock)
5. `plugin_manager` (RwLock)
6. `self_ref` (Mutex)

**NeoSystemContext locks:**

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

**Important**: Never hold a lock across an `.await` point.

## Testing Strategy

- Unit tests for serialization, VM, state store (proofs, validation), and contract/native logic remain in `neo-core`.
- Foundation crates (`neo-primitives`, `neo-crypto`, `neo-storage`) have comprehensive unit tests.
- Integration tests should exercise service registration and RPC surfaces (e.g., state-service endpoints) with in-process components where possible.
- Golden compatibility tests (C# parity, Neo N3 v3.9.0 fixtures) must not be broken without updating fixtures.

## Observability

- Use `tracing` with clear targets (`neo`, `rpc`, `state`, `tee`). Wrap significant operations (block import, state-root verification, RPC handlers) in spans.
- Metrics: Prometheus gauges are exported from `neo-node` for header/blocks, mempool size, RPC counts, state-root indices/lag, and state-root ingest accepted/rejected totals. Keep metric names stable for dashboards.

## Configuration

- Keep configs serde-driven with explicit defaults and validation. Avoid silent fallbacks for protocol-critical values (network magic, validator counts, storage paths).
- For new components, add a config struct, validation, and a TOML example snippet under `docs/`.

## C# Neo Compatibility

Target compatibility: Neo N3 v3.9.0 (C# v3.9.0 release).

The crate structure mirrors the C# Neo implementation:

| Rust Crate               | C# Project                        |
| ------------------------ | --------------------------------- |
| neo-primitives           | Neo (partial)                     |
| neo-crypto               | Neo.Cryptography                  |
| neo-io                   | Neo.IO                            |
| neo-json                 | Neo.Json                          |
| neo-vm                   | Neo.VM                            |
| neo-core                 | Neo                               |
| neo-core::smart_contract | Neo.SmartContract                 |
| neo-core::services       | (Service abstractions)            |
| neo-p2p                  | Neo.Network.P2P                   |
| neo-rpc                  | Neo.Plugins.RpcServer + RpcClient |
| neo-consensus            | Neo.Plugins.DBFTPlugin            |
| neo-core::extensions     | Neo.Extensions                    |
| (blst crate)             | Neo.Cryptography.BLS12_381        |

## Merged Crates (v0.7.0)

The following crates were merged to simplify the architecture:

| Removed Crate  | Merged Into              | Notes                                       |
| -------------- | ------------------------ | ------------------------------------------- |
| neo-contract   | neo-core::smart_contract | Smart contract types, native contracts, NEF |
| neo-services   | neo-core::services       | Service trait definitions                   |
| neo-rpc-client | neo-rpc (client feature) | RPC client with `features = ["client"]`     |

## New Crates Test Coverage

The following new crates were created during the architecture refactoring:

| Crate          | Description                                                     | Unit Tests |
| -------------- | --------------------------------------------------------------- | ---------- |
| neo-primitives | UInt160, UInt256, Hardfork, constants                           | 34         |
| neo-crypto     | Hash functions, ECC types, NamedCurveHash                       | 22         |
| neo-storage    | Storage traits, KeyBuilder, StorageKey, StorageItem             | 21         |
| neo-p2p        | MessageCommand, InventoryType, WitnessScope, VerifyResult, etc. | 71         |
| neo-consensus  | ConsensusMessageType, ChangeViewReason                          | 9          |
| neo-rpc        | RpcErrorCode, RPC client (with `client` feature)                | 6+         |
| **Total**      |                                                                 | **163+**   |

> **Note**: neo-contract tests (40) are now part of neo-core::smart_contract module tests.

### Key Types by Crate

**neo-primitives:**

- `UInt160` - 160-bit unsigned integer (script hashes, addresses)
- `UInt256` - 256-bit unsigned integer (block/transaction hashes)
- `Hardfork` - Neo blockchain hardfork enumeration (Aspidochelone, Basilisk, etc.)
- Protocol constants (network magic, ports, sizes, fees, etc.)

**neo-crypto:**

- `Crypto` - Hash functions (SHA256, SHA512, RIPEMD160, Keccak256, Blake2b/s, Hash160, Hash256)
- `ECCurve` - Elliptic curve identifiers (Secp256r1, Secp256k1, Ed25519)
- `ECPoint` - Elliptic curve point representation
- `NamedCurveHash` - Curve and hash algorithm combinations for signatures

**neo-storage:**

- `IReadOnlyStore` - Read-only storage trait
- `IWriteStore` - Write storage trait
- `IStore` - Combined read/write storage trait
- `StorageKey`, `StorageItem` - Storage primitives
- `SeekDirection`, `TrackState` - Storage utilities
- `KeyBuilder` - Builder for constructing storage keys

**neo-core::smart_contract:**

- `TriggerType` - Contract trigger types (Application, Verification, etc.)
- `ContractParameterType` - Parameter types for contract methods
- `FindOptions` - Storage iteration options
- `MethodToken` - Static method call tokens for contracts
- `Role` - Network roles (StateValidator, Oracle, NeoFSAlphabetNode, P2PNotary)
- `StorageContext` - Storage context for contract data access
- `ContractBasicMethod` - Standard contract method names and parameter counts
- `CallFlags` - Re-exported from neo-vm

**neo-core::services:**

- `LedgerService` - Ledger access trait
- `StateStoreService` - State store access trait
- `MempoolService` - Mempool access trait
- `PeerManagerService` - Peer manager access trait
- `RpcService` - RPC service access trait

**neo-p2p:**

- `MessageCommand` - P2P message command identifiers
- `InventoryType` - Inventory types (Transaction, Block, etc.)
- `MessageFlags` - Message header flags
- `NodeCapabilityType` - Node capability identifiers
- `WitnessScope` - Witness scope flags
- `WitnessConditionType` - Witness condition types
- `WitnessRuleAction` - Witness rule actions (Allow/Deny)
- `TransactionAttributeType` - Transaction attribute types
- `OracleResponseCode` - Oracle response codes
- `VerifyResult` - Transaction/inventory verification results
- `TransactionRemovalReason` - Reasons for transaction removal from mempool
- `ContainsTransactionType` - Transaction containment status

**neo-consensus:**

- `ConsensusMessageType` - dBFT message types (PrepareRequest, Commit, etc.)
- `ChangeViewReason` - Reasons for view change requests

**neo-rpc:**

- `RpcErrorCode` - JSON-RPC 2.0 standard and Neo-specific error codes
- `RpcClient` - JSON-RPC client (with `client` feature)
- `RpcClientBuilder` - Builder for configuring RPC client
- `RpcException` - RPC error type
