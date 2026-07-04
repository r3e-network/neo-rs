# Neo-RS Architecture Audit Report

**Date:** 2026-07-03
**Scope:** Full workspace (27 crates, 7 declared layers)
**Benchmarks:** reth (Paradigm) and Polkadot SDK (Parity)
**Methodology:** Dependency graph analysis, trait abstraction audit, crate boundary inspection, comparison with production-grade Rust blockchain node architectures.

---

## Executive Summary

The neo-rs architecture is **solid but incomplete**. It has a clean 7-layer hierarchy, zero circular dependencies, and rudimentary trait-based service abstractions (`neo-runtime`). Compared to reth and Polkadot SDK, it excels in simplicity but lacks the **composability traits**, **RPC decoupling**, and **engine pipeline abstraction** that make those codebases extensible and contributor-friendly.

**Overall grade:** B+ (Good foundations, significant room for composability improvement)

---

## Part 1: Current Architecture Assessment

### 1.1 Layer Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│ L7  APPLICATION     neo-node (binary)                             │
├──────────────────────────────────────────────────────────────────┤
│ L6  PLUGIN/RPC      neo-rpc ←──── neo-oracle-service              │
│                          │              │                         │
│                          ▼              ▼                         │
│ L5  COMPOSITION      neo-system (Node, NodeBuilder)               │
├──────────────────────────────────────────────────────────────────┤
│ L4  NODE SERVICE     neo-blockchain │ neo-network │ neo-wallets    │
│                      neo-indexer    │ neo-tee                      │
├──────────────────────────────────────────────────────────────────┤
│ L3  DOMAIN SERVICE   neo-runtime │ neo-execution │ neo-mempool     │
│                      neo-native-contracts │ neo-state-service      │
├──────────────────────────────────────────────────────────────────┤
│ L2  PROTOCOL         neo-payloads │ neo-consensus │ neo-hsm        │
├──────────────────────────────────────────────────────────────────┤
│ L1  INFRASTRUCTURE   neo-config, neo-crypto, neo-storage,         │
│                      neo-static-files, neo-io, neo-vm,            │
│                      neo-error, neo-serialization, neo-manifest    │
├──────────────────────────────────────────────────────────────────┤
│ L0  FOUNDATION       neo-primitives                               │
└──────────────────────────────────────────────────────────────────┘
```

### 1.2 What's Working Well

| Aspect | Status | Detail |
|--------|--------|--------|
| **Layered architecture** | ✅ Clean | 7 logical layers, no crate looks upward |
| **No circular deps** | ✅ Clean | Dependency graph is a strict DAG |
| **Trait-based services** | ✅ Basics exist | `neo-runtime` defines `Service`, `BlockExecutor`, `NetworkService`, `ConsensusService`, `NeoEngine`, `BlockImport` |
| **NodeBuilder pattern** | ✅ Basics exist | `neo-system/src/composition/builder.rs` has `NodeBuilder` |
| **Centralized lint policy** | ✅ Good | `workspace.lints.rust = {unsafe_code = "deny"}` |
| **Centralized dependencies** | ✅ Good | `workspace.dependencies` in root Cargo.toml |
| **ServiceRegistry** | ✅ Good | `neo-system/src/composition/service_registry.rs` |

### 1.3 Structural Issues Found

#### Issue 1: False "Plugin/RPC" Layer — Tight Coupling via `neo-system::Node`

**Severity: HIGH**

Both `neo-rpc` and `neo-oracle-service` declare `neo-system` as a **required** (not optional, not feature-gated) dependency and import `neo_system::Node` directly in production code:

```
neo-rpc/src/application_logs/service.rs
neo-rpc/src/server/session/mod.rs
neo-rpc/src/server/rpc_server/mod.rs
neo-rpc/src/plugins/tokens_tracker/runtime.rs
neo-rpc/src/plugins/tokens_tracker/trackers/tracker_base.rs
neo-rpc/src/plugins/tokens_tracker/trackers/nep_17/nep17_tracker.rs
neo-rpc/src/plugins/tokens_tracker/trackers/nep_11/nep11_tracker.rs
neo-oracle-service/src/service/mod.rs
neo-oracle-service/src/handlers.rs
neo-oracle-service/src/lifecycle/state.rs
```

**Impact:**
- `neo-rpc` cannot compile without the full node composition
- A plain `cargo build` compiles ~22 of 24 workspace crates because `neo-rpc` is in `default-members`
- Any change to `neo-system::Node` forces recompilation of the entire RPC subtree
- Plugin/extension pattern is impossible — these are not plugins, they are consumers

**Comparison with reth:** reth's RPC layer (`reth-rpc`, `reth-rpc-api`) does NOT depend on the node composition. It depends on `reth-provider` (storage trait) and `reth-network-api` (network trait). The `reth-rpc-builder` composes them at the binary level. You can use `reth-rpc` without the full node.

#### Issue 2: neo-rpc in default-members Is a Build Tax

**Severity: MEDIUM**

`default-members` includes `neo-rpc`, which transitively pulls in nearly the entire workspace:

```
default-members = [
    "neo-primitives",    // L0
    "neo-config",        // L1
    "neo-crypto",        // L1
    "neo-storage",       // L1
    "neo-io",            // L1
    "neo-rpc",           // L6 ← pulls everything
    "neo-indexer",       // L4
    "neo-consensus",     // L2
    "neo-node",          // L7
]
```

A `cargo build` compiles **22 crates**. On a clean build, `neo-rpc`'s transitive closure dominates build time.

**Recommendation:** Remove `neo-rpc` from `default-members` or gate heavy deps behind features. Only `neo-node` and `neo-consensus` are genuinely needed for a default build.

#### Issue 3: Dense Same-Layer Coupling in Infrastructure (L1)

**Severity: LOW-MEDIUM**

Seven of nine L1 crates form a dense sub-graph:

```
neo-crypto ──┐
neo-io ──────┤
neo-vm ──────┼──→ neo-serialization ──→ neo-manifest
neo-storage ─┤
neo-error ───┘
```

Only `neo-static-files` and `neo-config` stand apart. This makes L1 harder to understand as a cohesive "layer" — it's really two sub-layers:
1. **Core Infra**: `neo-error`, `neo-io`, `neo-primitives` (actual bottom layer)
2. **Stateful Infra**: `neo-crypto`, `neo-storage`, `neo-vm`, `neo-serialization`, `neo-manifest` (depends on core infra)

**Comparison with reth:** reth splits storage into `db`, `db-api`, `db-models`, `db-common`, `provider`, `storage-api`, `errors`, `codecs` — each a separate crate with clear responsibility boundaries. This allows downstream consumers to depend on just `storage-api` without pulling in the entire database stack.

#### Issue 4: Missing NodeTypes / NodeComponents Trait Hierarchy

**Severity: MEDIUM**

neo-system stores services as `Option<Arc<dyn Trait>>`:

```rust
// Current approach: trait objects
pub struct Node {
    pub block_executor: Option<Arc<dyn BlockExecutor>>,
    pub consensus_service: Option<Arc<dyn ConsensusService>>,
    pub engine: Option<Arc<dyn NeoEngine>>,
}
```

**Problem:** This is runtime polymorphism with no type-level guarantees. The builder cannot verify that a valid configuration has all required services at compile time. If a service is missing, you get a runtime panic.

**Comparison with reth:** reth uses the type-state pattern with associated types:

```rust
// reth's approach: type-level component specification
pub trait NodeTypes {
    type Primitives: FullNodePrimitives;
    type Engine: EngineTypes;
}

pub trait NodeComponents<Node: FullNodeTypes> {
    type Pool: TransactionPool;
    type Evm: ConfigureEvm;
    type Network: NetworkBuilder;
    type PayloadBuilder: PayloadBuilder;
}
```

This makes configuration errors compile-time errors. The node builder enforces that all required components are provided before `build()` is called.

#### Issue 5: RPC Layer Missing API/Implementation Separation

**Severity: MEDIUM**

neo-rpc is a single monolithic crate — it contains both the RPC server implementation AND the RPC method definitions. There is no equivalent to reth's `reth-rpc-api` (trait definitions only) → `reth-rpc` (implementation) → `reth-rpc-builder` (composition) separation.

**Impact:**
- You cannot depend on RPC method traits without pulling in the full implementation
- Testing RPC methods requires building the full server
- Alternative RPC implementations (gRPC, WebSocket-only, etc.) cannot reuse trait definitions

#### Issue 6: No Dedicated Engine/Pipeline Abstraction

**Severity: MEDIUM**

neo-rs has no equivalent to reth's `EngineApiTreeHandler` — a dedicated crate that orchestrates block execution, state transition, and persistence as a pipeline. Currently:
- Block processing is split between `neo-blockchain` (validation), `neo-execution` (VM execution), and `neo-system` (composition)
- There's no single "engine" that owns the pipeline lifecycle
- The `Node` struct in `neo-system` acts as a catch-all runtime container

**Comparison with reth:** reth has a dedicated `crates/engine/tree/` that handles the full Engine API lifecycle (newPayload, forkchoiceUpdated, getPayload) as well as block processing queue management, persistence, and in-memory state tracking.

#### Issue 7: neo-wallets Depends on neo-execution + neo-native-contracts

**Severity: LOW**

```rust
neo-wallets (L4, Node Service) → neo-execution (L3, Domain Service)
                                → neo-native-contracts (L3, Domain Service)
```

The wallet layer reaching into execution and native contracts is **directionally fine** (downward), but architecturally heavy. Wallet operations (NEP-2 encryption, key derivation, address formatting) shouldn't need the full VM execution engine.

**Recommendation:** Extract wallet verification logic into a `neo-wallets-verification` crate in L3, or move the witness scripting into `neo-execution` where it belongs. `neo-wallets` should only handle key management.

---

## Part 2: Comparison with reth

### 2.1 What reth Does That We Should Adopt

| reth Pattern | neo-rs Status | Priority | Action |
|---|---|---|---|
| **NodeTypes / NodeComponents traits** | Missing | HIGH | Define trait hierarchy in `neo-runtime` for compile-time component verification |
| **RPC API/impl separation** | Missing | HIGH | Split `neo-rpc` into `neo-rpc-api` (traits) + `neo-rpc` (impl) + `neo-rpc-builder` (composition) |
| **Provider abstraction layer** | Partial | MEDIUM | `neo-storage` has DataCache/StoreCache; add a higher-level `BlockchainProvider` trait |
| **Stage-based sync pipeline** | Missing | MEDIUM | Create `neo-sync` crate with `SyncStage` trait and pipeline driver |
| **Engine tree abstraction** | Missing | MEDIUM | Create `neo-engine` crate for block processing pipeline |
| **RPC decoupled from node core** | Missing | HIGH | neo-rpc must depend on storage traits, not `neo_system::Node` |
| **Type-state builder pattern** | Partial | MEDIUM | Extend `NodeBuilder` with type-state transitions |
| **Dedicated execute/consensus/network crates** | Partial | LOW | Already have `neo-execution`, `neo-consensus`, `neo-network` — good |
| **Transaction pool as standalone trait** | Partial | LOW | `neo-mempool` exists but isn't behind a trait in neo-runtime |

### 2.2 reth's Crate Organization Pattern

```
reth/
├── bin/reth                    # Binary only
├── crates/
│   ├── primitives/             # Common types (depended on by everything)
│   ├── primitives-traits/      # Abstract type traits
│   ├── storage/
│   │   ├── db/                 # Database abstractions
│   │   ├── db-api/             # Database traits (low-level)
│   │   ├── db-models/          # Typed table models
│   │   ├── storage-api/        # Storage traits (high-level)
│   │   ├── provider/           # BlockchainProvider implementation
│   │   ├── errors/             # Storage-specific errors
│   │   └── codecs/             # Encoding/decoding
│   ├── net/
│   │   ├── network/            # P2P network
│   │   ├── network-api/        # Network traits (used by RPC, engine)
│   │   ├── peers/              # Peer management
│   │   └── downloaders/        # Block downloaders
│   ├── evm/
│   │   ├── evm/                # EVM configuration traits
│   │   ├── execution-types/    # Execution types
│   │   └── execution-errors/   # Execution errors
│   ├── rpc/
│   │   ├── rpc-api/            # RPC method traits
│   │   ├── rpc/                # RPC implementation
│   │   └── rpc-builder/        # RPC server composition
│   ├── engine/
│   │   └── tree/               # Engine API handler + pipeline
│   └── consensus/
│       └── consensus/          # Consensus engine
```

### 2.3 reth's Dependency Flow (ideal)

```
bin/reth
  └─ crates/ethereum/node       # NodeComponents impl for Ethereum
       ├─ crates/storage/provider  (trait)
       ├─ crates/net/network-api   (trait)
       ├─ crates/rpc/rpc-builder   (composition)
       ├─ crates/engine/tree       (execution pipeline)
       └─ crates/consensus/consensus
            └─ crates/primitives   (leaf types)
```

Key insight: The binary depends on the **composition layer** (node builder), which depends on **traits** (provider, network-api, rpc-api), which depend on **primitives**. No crate depends on the binary or the composition root.

---

## Part 3: Comparison with Polkadot SDK

### 3.1 What Polkadot SDK Does That We Should Adopt

| Polkadot Pattern | neo-rs Status | Priority | Action |
|---|---|---|---|
| **Host-Runtime (Wasm) boundary** | N/A | LOW | Not applicable (pure Rust execution) |
| **Pallet/plugin system** | Missing | MEDIUM | Native contracts are hardcoded; consider a `NativeContract` registry trait |
| **SCALE codec** | Partial | LOW | `neo-serialization` is custom; works fine but not standard |
| **Offchain workers** | Missing | LOW | Not a core neo-rs concern yet |
| **Benchmarking framework** | Partial | LOW | `benches-package/` has criterion benchmarks |
| **Amalgamation crate (frame::prelude)** | Missing | LOW | Could create `neo::prelude` for convenience imports |
| **Runtime versioning** | Missing | MEDIUM | No runtime version tracking for upgrade detection |
| **Executive orchestration** | Missing | MEDIUM | No dedicated block execution orchestrator |

### 3.2 Polkadot's Host-Runtime Separation

Polkadot SDK separates execution into two worlds:
- **Host (Native):** Network, consensus, storage (compiled to native binary)
- **Runtime (Wasm):** State transition function (compiled to Wasm, stored on-chain)

neo-rs doesn't need this exact pattern (pure Rust execution), but the **conceptual separation** is valuable:
- **Execution-sensitive code** should be in dedicated crates (`neo-execution`, `neo-native-contracts`)
- **Host code** in `neo-network`, `neo-storage` should never import execution internals
- Currently, `neo-wallets` (host) imports `neo-execution` (execution) — this boundary should be clearer

### 3.3 Polkadot's FRAME Pallet Pattern

```rust
// Polkadot: pallets are self-contained modules
#[frame::pallet]
pub mod pallet {
    #[pallet::config]
    pub trait Config: frame_system::Config { ... }
    
    #[pallet::call]
    impl<T: Config> Pallet<T> { ... }
}
```

neo-rs equivalent is the `NativeContract` trait:

```rust
// neo-rs: native contracts implement a trait
pub trait NativeContract {
    fn name(&self) -> &'static str;
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()>;
    // ...
}
```

**Gap:** Polkadot's pallets register dispatchable calls via `#[pallet::call]` with automatic encoding/decoding. neo-rs native contracts have manual call dispatch in `neo-native-contracts`. Consider adding a `#[native_contract::call]` macro for automatic method dispatch.

---

## Part 4: Recommendations (Priority-Ordered)

### HIGH Priority (Address Now)

#### 1. Decouple RPC from Node Composition

**Current:** `neo-rpc` directly imports `neo_system::Node`
**Target:** `neo-rpc` depends on storage traits (`neo-storage`), network traits (`neo-runtime::NetworkService`), and a shared types crate.

**Implementation:**
```
Step 1: Create neo-rpc-api crate (traits only)
  - Define RpcServer trait with method signatures
  - No dependencies on neo-system or neo-blockchain

Step 2: Move neo-rpc implementation to depend on traits
  - Replace `neo_system::Node` with `Arc<dyn BlockchainProvider>` 
  - Define BlockchainProvider trait in neo-storage or neo-runtime

Step 3: Create neo-rpc-builder crate (composition)
  - Accepts storage provider + network handle at construction
  - Called from neo-system or neo-node binary
  - This is where neo-system::Node is injected
```

#### 2. Define NodeComponents / NodeTypes Traits

**Current:** Services are `Option<Arc<dyn Trait>>` — compile-time unchecked
**Target:** Trait hierarchies that enforce valid configurations at compile time.

**Implementation in neo-runtime:**
```rust
/// Primitive types used throughout the node.
pub trait NodeTypes: Send + Sync + 'static {
    type Primitives: neo_primitives::NeoPrimitives;
    type Payload: neo_payloads::Payload;
    type Engine: NeoEngine;
}

/// Components a full node must provide.
pub trait NodeComponents<T: NodeTypes>: Send + Sync + 'static {
    type Pool: TransactionPool;
    type Network: NetworkService;
    type Consensus: ConsensusService;
    type Executor: BlockExecutor;
    type Provider: StateProvider;
}
```

#### 3. Remove neo-rpc from default-members

**Current:** `cargo build` compiles 22 crates
**Target:** `cargo build` compiles only essential crates (foundation + infrastructure + protocol + node binary)

**Implementation:**
```toml
default-members = [
    "neo-primitives",
    "neo-config",
    "neo-crypto",
    "neo-storage",
    "neo-io",
    "neo-consensus",
    "neo-node",        # Entry point
    # neo-rpc removed — builds on demand
    # neo-indexer removed — builds on demand
]
```

### MEDIUM Priority (Address in Next Iteration)

#### 4. Create neo-engine Pipeline Crate

A dedicated crate for the block processing pipeline:
- Block import queue management
- Execution → validation → persistence pipeline stages
- Engine API handler (if adopting Engine API pattern)
- In-memory state tracking (BlockBuffer, canonical chain tracker)

#### 5. Split Infrastructure Layer

```
Current L1 (9 crates, dense coupling):

  neo-error → neo-io → neo-crypto → neo-storage → neo-vm → neo-serialization → neo-manifest
  neo-config (standalone)
  neo-static-files (standalone)

Proposed split:
  Sub-L1a (Core Infra):  neo-primitives, neo-error, neo-io, neo-config
  Sub-L1b (State Infra): neo-crypto, neo-storage, neo-vm, neo-serialization, neo-manifest, neo-static-files
```

This makes the dependency direction explicit: Core Infra ← State Infra (not sideways within the same layer).

#### 6. Add Stage-Based Sync Pipeline

Create a `neo-sync` crate modeled on reth's `reth-stages`:
```rust
pub trait SyncStage: Send + Sync {
    fn id(&self) -> SyncStageId;
    async fn execute(&self, input: ExecInput, provider: &dyn StateProvider) 
        -> Result<ExecOutput>;
}

pub struct Pipeline {
    stages: Vec<Box<dyn SyncStage>>,
}
```

#### 7. Refactor neo-wallets to Remove Execution Dependency

Move witness/verification logic out of `neo-wallets` into `neo-execution`, where it belongs. `neo-wallets` should handle only:
- Key generation and storage
- NEP-2 encryption/decryption
- Wallet file I/O
- BIP-32/39/44 derivation

### LOW Priority (Long-Term Improvement)

#### 8. Runtime Versioning

Add a `neo-version` crate tracking runtime version, similar to `sp-version` in Polkadot SDK. This enables:
- Upgrade detection
- Feature-gated hardfork activation
- Version-aware serialization

#### 9. Native Contract Registry

Replace hardcoded native contract initialization with a registry pattern:
```rust
pub trait NativeContractFactory {
    fn create(&self, settings: &ProtocolSettings) -> Box<dyn NativeContract>;
}

pub struct NativeContractRegistry {
    factories: HashMap<ContractName, Box<dyn NativeContractFactory>>,
}
```

This allows third-party native contracts to be registered at node startup.

#### 10. Convenience Prelude Crate

Create `neo::prelude` re-exporting commonly used types, similar to `frame::prelude`:
```rust
pub mod prelude {
    pub use neo_primitives::*;
    pub use neo_crypto::*;
    pub use neo_error::*;
    pub use neo_storage::prelude::*;
}
```

---

## Part 5: Quantitative Metrics

| Metric | neo-rs | reth | Polkadot SDK |
|--------|--------|------|-------------|
| **Workspace crates** | 27 | 150+ | 300+ |
| **Circular deps** | 0 | 0 | 0 |
| **Layering violations** | 0 | 0 | 0 |
| **Trait abstractions** | 8 (neo-runtime) | 50+ | 100+ |
| **API/impl separation** | None | Complete | Complete |
| **Type-state builder** | Partial | Full | Full |
| **default-members compiles** | 22 crates | ~60 | ~80 |
| **RPC crate count** | 1 (monolithic) | 10+ | 15+ |
| **Storage crate count** | 1 (neo-storage) | 12 | 20+ |
| **Engine crate count** | 0 | 1 (engine/tree) | 3+ |

---

## Part 6: Proposed Target Architecture

```
neo-rs/
├── bin/neo-node                  # Binary only
├── crates/
│   ├── primitives/
│   │   ├── neo-primitives/       # L0: Core types (UInt160, UInt256, etc.)
│   │   ├── neo-error/            # L1a: CoreResult, CoreError
│   │   └── neo-io/               # L1a: Binary I/O helpers
│   ├── crypto/
│   │   ├── neo-crypto/           # Hashing, ECC, signatures, BLS
│   │   └── neo-hsm/              # Optional HSM support
│   ├── storage/
│   │   ├── neo-storage-api/      # L1b: Storage traits (ReadOnlyStore, DataCache)
│   │   ├── neo-storage/          # L1b: RocksDB/MDBX/in-memory backends
│   │   ├── neo-storage-models/   # L1b: Typed table models
│   │   └── neo-static-files/     # L1b: Append-only cold files
│   ├── vm/
│   │   └── neo-vm/               # L1b: NeoVM host
│   ├── codec/
│   │   ├── neo-serialization/    # L1b: Binary/JSON codecs
│   │   └── neo-manifest/         # L1b: Contract ABI/NEF
│   ├── protocol/
│   │   ├── neo-payloads/         # L2: Block, Transaction, etc.
│   │   └── neo-types/            # L2: Shared P2P types (optional)
│   ├── consensus/
│   │   └── neo-consensus/        # L2: dBFT consensus engine
│   ├── execution/
│   │   ├── neo-execution/        # L3: ApplicationEngine
│   │   └── neo-native-contracts/ # L3: All 11 native contracts
│   ├── state/
│   │   └── neo-state-service/    # L3: MPT, state root
│   ├── pool/
│   │   └── neo-mempool/          # L3: Transaction pool
│   ├── sync/
│   │   └── neo-sync/             # NEW: Staged sync pipeline
│   ├── engine/
│   │   └── neo-engine/           # NEW: Engine/pipeline orchestration
│   ├── blockchain/
│   │   └── neo-blockchain/       # L4: Validation, persistence
│   ├── network/
│   │   ├── neo-network-api/      # NEW: Network traits
│   │   └── neo-network/          # L4: P2P implementation
│   ├── wallets/
│   │   └── neo-wallets/          # L4: Key management only
│   ├── rpc/
│   │   ├── neo-rpc-api/          # NEW: RPC method traits
│   │   ├── neo-rpc/              # L6: RPC implementation
│   │   └── neo-rpc-builder/      # NEW: RPC composition
│   ├── node/
│   │   ├── neo-runtime/          # L3: Service traits (NodeTypes, NodeComponents)
│   │   ├── neo-config/           # L1a: Configuration
│   │   └── neo-system/           # L5: NodeBuilder, composition root
│   ├── plugins/
│   │   ├── neo-indexer/          # L4: Read-side indexing
│   │   ├── neo-oracle-service/   # L6: Oracle request fulfillment
│   │   └── neo-tee/              # L4: Optional TEE support
│   └── dev/
│       ├── neo-version/          # NEW: Runtime version tracking
│       ├── neo-prelude/          # NEW: Convenience re-exports
│       └── neo-benchmarks/       # Criterion benchmarks
```

---

## Conclusion

The neo-rs architecture is fundamentally sound — clean layering, no circular dependencies, working NodeBuilder, and trait-based service abstractions. The three highest-impact changes to match reth/Polkadot quality are:

1. **Decouple RPC from composition root** — `neo-rpc` must depend on traits, not `neo_system::Node`
2. **Add NodeTypes/NodeComponents traits** — make configuration errors compile-time errors
3. **Remove heavy crates from default-members** — reduce build times for developers

These changes can be made incrementally without breaking existing functionality. Each is a purely additive refactor.
