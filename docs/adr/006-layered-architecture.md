# ADR-006: Layered Architecture with Unidirectional Dependencies

## Status

Accepted (In Progress)

## Date

2024-09

## Context

As the neo-core decoupling progressed, a clear layered architecture emerged. However, without explicit documentation of the dependency rules, there was risk of:

- Accidentally introducing circular dependencies
- Lower layers depending on higher layers
- Infrastructure code depending on business logic
- Unclear boundaries between architectural layers

A well-structured layered architecture needs:
- Explicit layer definitions
- Clear dependency direction rules
- Documentation of what belongs in each layer

## Decision

Formalize the neo-rs codebase as a strict layered architecture where dependencies only flow downward, never upward or sideways within a layer.

### Layer 0: Foundation (Zero neo-* Dependencies)

**Purpose**: Pure data types, no behavior, no dependencies

- **neo-primitives**: UInt160, UInt256, BigDecimal, CallFlags, ContractParameterType, Hardfork, InventoryType, TriggerType, FindOptions, ContractBasicMethod, SerializablePayload, Inventory, Witness, VerificationContext, Verifiable, BlockLike, HeaderLike, TransactionLike, NetworkMessage, RpcException, UnhandledExceptionPolicy, PeerInfo, PeerId, StorageValue, VerifyResult, WitnessConditionType, WitnessRuleAction, ContainsTransactionType, TransactionRemovalReason, OracleResponseCode, NodeCapabilityType, TransactionAttributeType, constants

- **neo-json**: JToken, JObject, JArray (JSON representation)

- **neo-config**: Settings, ProtocolSettings, NetworkConfig

**Dependency Rule**: May only depend on standard library and common external crates (serde, etc.). No neo-* dependencies.

### Layer 0.5: Foundation with Inter-Dependencies

**Purpose**: Foundation libraries that need each other but not business logic

- **neo-io**: BinaryWriter, MemoryReader, Serializable, compression, WitnessCondition, WitnessRule, IO extensions, caching, var_int
  - Depends on: neo-primitives, neo-json

- **neo-storage**: StorageKey, StorageItem, DataCache, Store, StoreSnapshot
  - Depends on: neo-primitives, neo-io

- **neo-crypto**: Crypto, MerkleTree, ECC, ECPoint, MPT Trie, BloomFilter
  - Depends on: neo-primitives, neo-io

**Dependency Rule**: May depend on Layer 0 and other Layer 0.5 crates. Must not depend on Layer 1+.

### Layer 1: Core Business Logic

**Purpose**: Application-specific blockchain logic

- **neo-vm**: ScriptBuilder, ExecutionEngine, StackItem, JumpTable
  - Depends on: neo-primitives, neo-io, neo-json

- **neo-core**: ApplicationEngine, NativeContracts, Block, Transaction, Header, Verifiable implementations, ProtocolSettings, NeoSystem, wallets, oracle, tokens_tracker
  - Depends on: neo-primitives, neo-io, neo-storage, neo-crypto, neo-vm, neo-json, neo-config

**Dependency Rule**: May depend on Layer 0, 0.5, and other Layer 1 crates. Must not depend on Layer 2 or Infrastructure.

### Layer 2: Protocol Services

**Purpose**: Network protocols and external interfaces

- **neo-consensus**: dBFT 2.0 consensus algorithm
  - Depends on: neo-primitives, neo-io, neo-crypto
  - **Zero dependency on neo-core** (operates on traits)

- **neo-p2p**: P2P networking protocol
  - Depends on: neo-primitives, neo-io, neo-crypto
  - **Zero dependency on neo-core**

- **neo-rpc**: JSON-RPC server and client
  - Depends on: neo-primitives, neo-json, neo-core (optional)

**Dependency Rule**: May depend on Layer 0 and 0.5. Should avoid neo-core dependency where possible (use traits). Must not depend on Infrastructure.

### Infrastructure: Cross-Cutting Concerns

**Purpose**: Telemetry, security enclaves, external system integration

- **neo-telemetry**: Metrics, tracing, logging
- **neo-hsm**: Hardware security module integration
- **neo-tee**: Trusted execution environment support

**Dependency Rule**: May depend on Layer 0 only. Must have zero dependencies on business logic (neo-core, neo-vm, neo-consensus, neo-p2p). This ensures infrastructure can observe without affecting behavior.

## Consequences

### Positive

- **No Circular Dependencies**: Impossible by construction
- **Predictable Builds**: Changes propagate upward only
- **Clear Mental Model**: Developers know where code belongs
- **Testability**: Lower layers testable without upper layers
- **Reusability**: Foundation layers usable in any project
- **Compilation Speed**: Parallel builds of independent layers

### Negative

- **Strictness**: Sometimes inconvenient to enforce (e.g., neo-rpc wants neo-core for full functionality)
- **Trait Complexity**: Protocol layer uses traits instead of concrete types
- **Refactoring Cost**: Moving code between layers requires updating dependencies
- **Learning Curve**: New contributors need to understand layer rules

### Enforcement

**Compile-Time**:
- Cargo dependency graph enforces most rules
- Circular dependencies cause compile errors

**Review-Time**:
- PRs adding dependencies between layers must justify the need
- New dependencies from higher to lower layers are questioned

**Documentation**:
- This ADR serves as the authoritative reference
- Each crate's Cargo.toml should document its layer

### Violations and Exceptions

**Accepted Violations**:
- neo-rpc optionally depends on neo-core for full-featured server (acceptable: optional dependency)

**Resolved Violations**:
- neo-consensus previously depended on neo-core → resolved by traits in neo-primitives
- neo-p2p previously embedded in neo-core → resolved by extraction

## Verification

The layered architecture is verified by:
1. Cargo build graph has no cycles
2. neo-consensus compiles without neo-core
3. neo-p2p compiles without neo-core
4. Infrastructure crates have minimal dependencies

## Related ADRs

This ADR synthesizes the architectural principles from:
- ADR-001: Decouple neo-core
- ADR-002: IO/persistence/crypto facades
- ADR-003: Consensus independence
- ADR-004: P2P layer extraction
- ADR-005: Verifiable trait primitives

## References

- Memory: project_refactoring_progress.md (architectural state diagram)
