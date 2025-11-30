# neo-rs Architecture

This document captures the current layering and service boundaries so new changes stay coherent.

## Layered Architecture

The codebase follows a strict layered architecture with clear dependency rules:

```
┌─────────────────────────────────────────────────────────────────┐
│                    APPLICATION LAYER                             │
│  neo-cli (CLI client)    neo-node (daemon)                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   INFRASTRUCTURE LAYER                           │
│  neo-akka (actors)   neo-services   neo-rpc-client   neo-plugins│
│                      neo-tee (TEE support)                       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                       CORE LAYER                                 │
│  neo-core      neo-vm       neo-contract    neo-p2p             │
│  neo-rpc       neo-consensus    neo-extensions                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    FOUNDATION LAYER                              │
│  neo-primitives (UInt160, UInt256, BigDecimal)                  │
│  neo-crypto (hashing, ECC, signatures)                          │
│  neo-storage (storage traits and abstractions)                  │
│  neo-io (serialization, caching)                                │
│  neo-json (JSON handling)                                       │
└─────────────────────────────────────────────────────────────────┘
```

**Dependency Rule**: Each layer may only depend on layers below it. Never depend upward.

## Crates and Responsibilities

### Foundation Layer (no neo-* dependencies except within layer)

- **neo-primitives**: Core primitive types (`UInt160`, `UInt256`, `BigDecimal`), constants, and basic error types. Zero external neo-* dependencies.
- **neo-crypto**: Cryptographic primitives (SHA256, RIPEMD160, Keccak256, Blake2, Hash160, Hash256), elliptic curve types (`ECCurve`, `ECPoint`). Depends only on `neo-primitives`.
- **neo-storage**: Storage trait abstractions (`IReadOnlyStore`, `IWriteStore`, `IStore`, `ISnapshot`), `StorageKey`, `StorageItem`, `SeekDirection`. Breaks circular dependencies between persistence and smart contracts.
- **neo-io**: Binary serialization (`BinaryReader`, `BinaryWriter`, `MemoryReader`), caching utilities. Matches C# `Neo.IO`.
- **neo-json**: JSON handling (`JToken`, `JObject`, `JArray`, `JPath`). Matches C# `Neo.Json`.

### Core Layer

- **neo-core**: Consensus-neutral protocol logic (ledger, state service, VM host integration, persistence adapters). Keep public APIs small; prefer `pub(crate)` where possible.
- **neo-vm**: NeoVM execution engine (stack machine, opcodes, script execution). Matches C# `Neo.VM`.
- **neo-contract**: Smart contract execution engine, native contracts (NEO, GAS, Policy, Oracle, etc.), contract manifest, NEF files. Depends on `neo-vm`.
- **neo-p2p**: P2P networking layer (message types, peer management, connection handling). Matches C# `Neo.Network.P2P`.
- **neo-rpc**: Unified RPC server and client. JSON-RPC 2.0 implementation for Neo node communication.
- **neo-consensus**: dBFT (Delegated Byzantine Fault Tolerance) consensus implementation. Matches C# consensus plugin.

> **Note**: BLS12-381 cryptographic operations use the `blst` crate directly (via neo-core) instead of a separate wrapper crate.
> **Note**: Extension utilities (formerly neo-extensions) are now integrated into `neo-core::extensions`.

### Infrastructure Layer

- **neo-akka**: Actor runtime (Akka-inspired) for concurrent message passing.
- **neo-services**: Service traits and abstractions for dependency injection.
- **neo-rpc-client**: Client-side RPC bindings with typed models and helpers. Keep it UI-agnostic so both CLI and external callers can reuse it.
- **neo-plugins**: Node-side extensions (RocksDB storage, token trackers). Interact with core through typed handles.
- **neo-tee**: Enclave-facing utilities and optional mempool/wallet. Feature-gated; avoid leaking into core.

### Application Layer

- **neo-cli**: Thin command wrappers over `neo-rpc-client`, no business logic.
- **neo-node**: Daemon composition (config, wiring actors, plugin loading). Owns service registration and lifecycle; avoids protocol logic.

## Service Access

- Use the typed service traits in `neo_core::services` (e.g., `LedgerService`, `StateStoreService`, `MempoolService`, `PeerManagerService`, `RpcService`) instead of `Any` downcasts or string keys.
- `NeoSystemContext` registers core services in the internal registry by `TypeId` so callers should prefer the typed accessors (`ledger_service()`, `state_store()`, `mempool_service()`, `local_node_service()`) before falling back to names.
- Plugins should continue to use helpers under `neo_plugins::service_access`, which now resolve through the typed registry first; prefer the typed helpers (`rpc_server_typed`, `ledger_typed`, `mempool_typed`, `peer_manager_typed`) for readiness/metrics.
- Block import treats store commit failures as fatal (no plugin notifications or cache updates happen after a failed commit), preventing RAM/disk divergence.
- Use `StoreTransaction` when persisting a batch of changes; avoid calling `StoreCache::commit` directly so commit failures propagate and stay observable. For state-service batching, use `StateStoreTransaction`.

## Services and Context

- Core service access is via `NeoSystemContext`; use typed accessors (e.g., `state_store()`, `ledger_service()`, `rpc_service()`) instead of `Any` downcasts. When introducing a new service, provide:
  - A trait for the required behaviour.
  - A typed accessor on `NeoSystemContext`/`NeoSystem` that also falls back to the canonical in-memory handle so callers aren't forced to know about registry names.
  - A readiness/health hook if it has external dependencies.
- Plugins should go through `neo_plugins::service_access` helpers to avoid duplicating registry lookup logic.
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
- Golden compatibility tests (C# parity) must not be broken without updating fixtures.

## Observability

- Use `tracing` with clear targets (`neo`, `rpc`, `state`, `tee`). Wrap significant operations (block import, state-root verification, RPC handlers) in spans.
- Metrics: Prometheus gauges are exported from `neo-node` for header/blocks, mempool size, RPC counts, state-root indices/lag, and state-root ingest accepted/rejected totals. Keep metric names stable for dashboards.

## Configuration

- Keep configs serde-driven with explicit defaults and validation. Avoid silent fallbacks for protocol-critical values (network magic, validator counts, storage paths).
- For new components, add a config struct, validation, and a TOML example snippet under `docs/`.

## C# Neo Compatibility

The crate structure mirrors the C# Neo implementation:

| Rust Crate | C# Project |
|------------|------------|
| neo-primitives | Neo (partial) |
| neo-crypto | Neo.Cryptography |
| neo-io | Neo.IO |
| neo-json | Neo.Json |
| neo-vm | Neo.VM |
| neo-core | Neo |
| neo-contract | Neo.SmartContract |
| neo-p2p | Neo.Network.P2P |
| neo-rpc | Neo.Plugins.RpcServer + RpcClient |
| neo-consensus | Neo.Plugins.DBFTPlugin |
| neo-core::extensions | Neo.Extensions |
| (blst crate) | Neo.Cryptography.BLS12_381 |

## New Crates Test Coverage

The following new crates were created during the architecture refactoring:

| Crate | Description | Unit Tests |
|-------|-------------|------------|
| neo-primitives | UInt160, UInt256, Hardfork, constants | 34 |
| neo-crypto | Hash functions, ECC types, NamedCurveHash | 22 |
| neo-storage | Storage traits, KeyBuilder, StorageKey, StorageItem | 21 |
| neo-contract | TriggerType, ContractParameterType, FindOptions, MethodToken, Role, StorageContext, ContractBasicMethod | 40 |
| neo-p2p | MessageCommand, InventoryType, WitnessScope, VerifyResult, etc. | 71 |
| neo-consensus | ConsensusMessageType, ChangeViewReason | 9 |
| neo-rpc | RpcErrorCode (JSON-RPC and Neo-specific error codes) | 6 |
| **Total** | | **203** |

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

**neo-contract:**
- `TriggerType` - Contract trigger types (Application, Verification, etc.)
- `ContractParameterType` - Parameter types for contract methods
- `FindOptions` - Storage iteration options
- `MethodToken` - Static method call tokens for contracts
- `Role` - Network roles (StateValidator, Oracle, NeoFSAlphabetNode, P2PNotary)
- `StorageContext` - Storage context for contract data access
- `ContractBasicMethod` - Standard contract method names and parameter counts
- `CallFlags` - Re-exported from neo-vm

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
