# neo-rs Architecture Design Document

**Version**: 5.1
**Date**: 2026-07-10
**Author**: Software Architect
**Status**: Active

---

## Executive Summary

This document captures the architecture decisions for the neo-rs Neo N3 Rust
node implementation. It covers the current state (26 crates, 8 ordered layers), the
identified issues, and the ADRs that resolve them. The goal is a codebase
that is professional, consistent, and ready for long-term evolution.

**Architecture health score**: 9.5/10 (up from 9.4 after ADR-027 dead code excision)

The ADR log now spans ADR-001 through ADR-036. Beyond the early trait-design,
duplication, and async/concurrency audits (ADR-016 through ADR-019), later ADRs
cover store-surface reduction and trait sealing (ADR-020, ADR-021), dead-code
excision (ADR-022, ADR-027, ADR-028, ADR-032), hex/KeyBuilder consolidation
(ADR-024, ADR-025), cross-crate helpers and test fixtures (ADR-029), the
neo-hsm default flip and ConsensusApi rename (ADR-030), and the async
ConsensusSigner deadlock fix (ADR-031), static composition (ADR-034), and the
staged core/application lifecycle with private service layouts (ADR-035), and
a production-consumed finalized Ledger archive (ADR-036).

---

## Current Architecture

### Layer Hierarchy (26 crates, 8 ordered layers)

| Layer | Crates | Role |
|-------|--------|------|
| L0 Foundation | neo-primitives | Type primitives (UInt160, UInt256, etc.) |
| L1a Core Infra | neo-io, neo-error | Binary I/O, unified CoreError type |
| L1b Stateful Infra | neo-crypto, neo-storage, neo-vm, neo-serialization, neo-manifest | Crypto, storage, VM, codecs, manifests |
| L1c Cross-cutting | neo-config | Configuration |
| L2 Protocol | neo-payloads, neo-consensus, neo-hsm | Block/tx types, dBFT, HSM signing |
| L3 Domain Services | neo-runtime, neo-execution, neo-native-contracts, neo-state-service, neo-mempool | Service traits, VM execution, native contracts, state, mempool |
| L4 Node Services | neo-blockchain, neo-network, neo-wallets, neo-indexer, neo-oracle-service | Block import, P2P, wallets, indexer, off-chain Oracle worker |
| L5 Composition | neo-system | Node composition root |
| L6 Plugin/RPC | neo-rpc | JSON-RPC server/client and adapter method groups |
| L7 Application | neo-node, neo-gui | Binary entry points |

### Key Patterns

1. **Provider traits** (`neo-runtime`): `ConfigProvider`, `StoreProvider`,
   `TxAdmission` — decouple service and adapter code from L5 (`BlockchainProvider` was removed in
   ADR-032)
2. **NodeTypes** (`neo-runtime`): the surviving sealed single-impl seam
   (ADR-021). The `NodeComponents` / `FullNode` type-state composition was
   removed in ADR-032; the builder validates concrete fields at `build()`
3. **Canonical block import** (`neo-runtime` + `neo-blockchain`): typed
   `BlockImport`, bounded import queue, validation pipeline, and ordered persist
4. **Unified error handling**: all library crates use `CoreError` with `From`
   impls; application crates use `anyhow::Result`

---

## Issues Resolved in This Session

### V1: neo-oracle-service → neo-system layer violation (FIXED)

**Problem**: `neo-oracle-service` (then classified as L6, now L4) held
`Arc<neo_system::Node>` (L5), creating an upward dependency.

**Fix**: Created the static `OracleRuntimeProvider` capability bound
(`ConfigProvider + StoreProvider + TxAdmission`) in `neo-oracle-service` and
added `TxAdmission` to `neo-runtime`. `OracleService<R, P>` remains generic over
its concrete runtime and native-contract providers; tests use `Node` via a dev
dependency.

### Error handling gaps (FIXED)

**Problem**: 7 crates had custom error types without `From<X> for CoreError`
impls, preventing seamless `?` propagation.

**Fix**: Added `From` impls for 11 error types across 7 crates. Added
`neo-error` dependency to 4 crates that lacked it.

### RpcError naming collision (FIXED)

**Problem**: Two types named `RpcError` — client enum in `errors/error.rs`
and server struct in `server/rpc_error/mod.rs`.

**Fix**: Renamed client enum to `RpcClientError`, result alias to
`RpcClientResult`. Server struct `RpcError` unchanged (matches C# class).

---

## Architecture Decision Records

### ADR-001: reth-style RPC crate split

**Status**: Partially Implemented (layer violation fixed; full 3-crate split deferred)

**Context**: `neo-rpc` (L6) had `neo-system` (L5) as a **required** dependency
because `node_context.rs` contained `impl From<&neo_system::Node> for
NodeContext`. This was a layer violation (L6 → L5 upward dependency). The
feature-flag system (`server` / `client`) already separated server and client
code paths, but the required `neo-system` dep meant even client-only builds
pulled in the entire composition root.

**Decision (Implemented)**: Remove the `From<&neo_system::Node> for
NodeContext` impl from `neo-rpc`. Add `NodeContext::from_parts()` constructor
so the composition root (`neo-node`) assembles a `NodeContext` from `Node`'s
public fields. Move `neo-system` from a required dependency to a
dev-dependency (test fixtures only). This eliminates the L6 → L5 layer
violation while keeping the single-crate structure.

**Decision (Deferred)**: The full 3-crate split (`neo-rpc-api` / `neo-rpc` /
`neo-rpc-client`) described below remains the long-term target for build-time
optimization. It is deferred because:
1. The critical layer violation is already fixed
2. Feature flags already separate client/server code paths
3. Solo developer — the 3-crate migration is high-effort, lower-urgency
4. Client-only builds are already possible via `--no-default-features --features client`

**Future 3-crate split target**:

```
neo-rpc-api    (L6) — trait definitions, request/response types, error codes
                     deps: neo-primitives, neo-serialization, serde
neo-rpc        (L6) — server implementation + JSON-RPC handler
                     deps: neo-rpc-api, neo-runtime, neo-storage, neo-execution, ...
neo-rpc-client (L6) — HTTP client implementation
                     deps: neo-rpc-api, reqwest, serde_json
```

**Trade-offs**:
- **Gained (now)**: No L6 → L5 layer violation. `neo-system` no longer in
  required deps. All 28 layer boundary tests and the full workspace suite pass.
- **Gaining (future)**: Client-only builds compile in seconds. Clean
  separation between API contracts and implementations.
- **Giving up**: 3 crates instead of 1 (future). Slightly more workspace
  complexity (future).
- **Reversibility**: High — the `from_parts()` constructor is mechanical to
  convert back to a `From` impl if needed.

**Consequences**:
- `neo-node` now calls `NodeContext::from_parts(...)` instead of
  `NodeContext::from(&node)` — 8-field construction at the composition root
- `neo-rpc` test files use a local `node_to_context()` helper (dev-dep
  `neo-system` still available for tests)
- Layer boundary test `test_no_upward_dependencies` passes

### ADR-002: neo-engine immediate integration

**Status**: Implemented (Phase 1 — adapter bridge)

**Context**: `neo-engine` was created as a scaffold with `EnginePipeline`
trait, stage traits, and in-memory state. It had zero consumers — no crate
depended on it. This was dead code that increased build time without value.

**Decision**: Integrate `neo-engine` into `neo-blockchain` via an adapter
pattern. `BlockchainEngineAdapter` wraps `BlockchainHandle` +
`LedgerContext` and implements `EnginePipeline`, delegating `process_block`
/ `process_batch` to the existing `BlockImport` trait, and
`current_height` / `current_hash` to `LedgerContext`'s atomic tip tracker.

**Trade-offs**:
- **Gaining**: The pipeline abstraction is now real, not theoretical. Stage
  composition enables future staged sync. Block processing has a clear
  trait boundary. `neo-engine` is no longer dead code.
- **Giving up**: One additional indirection layer (adapter wraps handle).
  Minimal runtime overhead — all delegation is direct method calls.
- **Reversibility**: High — the adapter can be removed without touching
  `BlockchainService` internals.

**Consequences**:
- `neo-blockchain` adds `neo-engine` as a dependency
- `BlockchainService` gains an `EnginePipeline` impl
- `neo-system` can accept `Arc<dyn EnginePipeline>` in the composition root
- Future staged sync can add stages to the pipeline without touching
  `BlockchainService` internals

### ADR-003: Error handling unification

**Status**: Accepted

**Context**: Library crates used a mix of custom error types and `CoreError`.
Some crates had `From<X> for CoreError` impls, others didn't. This made `?`
propagation inconsistent across crate boundaries.

**Decision**: Every library crate's error type must implement
`From<DomainError> for CoreError`. Application crates (`neo-node`, `neo-gui`)
continue using `anyhow::Result` (matching reth's `reth-node` pattern).

**Trade-offs**:
- **Gaining**: Consistent `?` propagation across all library crates. Single
  error type at the boundary. Clear conversion semantics.
- **Giving up**: Some type information is lost in the conversion (domain
  error variants become `CoreError::InvalidOperation { message }` or
  similar). This is acceptable because the message preserves the original
  error text.
- **Reversibility**: High — `From` impls are additive and can be removed
  without breaking existing code.

**Consequences**:
- All 14 domain error types now have `From` impls to `CoreError`
- `neo-error` is a dependency of every library crate
- The `impl_error_from!` macro is available but manual impls are preferred
  for clarity

### ADR-004: Oracle service decoupling via provider traits

**Status**: Accepted

**Context**: `neo-oracle-service` (L6) depended on `neo-system::Node` (L5)
to access settings, storage, and transaction routing. This was a layer
violation that made the oracle crate impossible to use without pulling in
the entire composition root.

**Decision**: Created `OracleNodeProvider` combined trait in
`neo-oracle-service` that requires `ConfigProvider + StoreProvider +
TxAdmission`. Added `TxAdmission` trait to `neo-runtime`. `Node` implements
all three traits.

**Trade-offs**:
- **Gaining**: `neo-oracle-service` production code no longer depends on
  `neo-system`. Layer boundary is clean. Oracle can be tested with mock
  providers. The `TxAdmission` trait is reusable by future L6 crates.
- **Giving up**: Tests still need `neo-system` as a dev-dep to construct
  real `Node` instances. This is acceptable — tests are not production code.
- **Reversibility**: High — the trait objects can be replaced with concrete
  types if performance becomes an issue.

**Consequences**:
- `neo-oracle-service/Cargo.toml`: `neo-system` moved to `[dev-dependencies]`
- `neo-runtime` gains `TxAdmission` trait
- `neo-system::Node` implements `TxAdmission`
- The pattern is now established for any future L6 crate that needs to
  submit transactions

### ADR-005: neo-gui as standalone excluded crate

**Status**: Accepted

**Context**: `neo-gui` uses direct version-pinned dependencies instead of
`workspace = true`. Initial audit flagged this as non-compliant.

**Decision**: Keep `neo-gui` excluded from the workspace. It is a standalone
eframe application that communicates with the node via JSON-RPC — it has
zero internal crate dependencies by design.

**Trade-offs**:
- **Gaining**: The GUI can be built independently without the workspace
  toolchain. No coupling between GUI dependencies (egui, eframe) and node
  dependencies.
- **Giving up**: GUI dependency versions are not centrally managed. This is
  acceptable because the GUI has no shared types with the node.
- **Reversibility**: High — adding `neo-gui` back to the workspace is a
  one-line change.

**Consequences**:
- `neo-gui` stays in the `exclude` list in root `Cargo.toml`
- Direct version pins are correct for an excluded crate
- No action needed

### ADR-006: Dead dependency cleanup

**Status**: Accepted

**Context**: Architecture audit v3 found three categories of dead or
misplaced dependencies:
1. `neo-oracle-service` declared `neo-blockchain` and `neo-network` as
   required dependencies but never imported either — pure dead weight
   inflating compile times and the dependency graph.
2. `neo-mempool` declared `neo-serialization` as a required dependency but
   only used it in `#[cfg(test)]` code (`src/tests/`).
3. `neo-rpc` had 12 unconditional internal dependencies that were only
   needed when `client` or `server` features were enabled. The default
   build (no features) compiled only the `error` and `error_code` modules
   but still pulled in all 12 crates.

**Decision**:
- Remove `neo-blockchain` and `neo-network` from `neo-oracle-service`
  `[dependencies]` entirely.
- Move `neo-serialization` from `[dependencies]` to `[dev-dependencies]`
  in `neo-mempool`.
- Make all 12 conditional deps in `neo-rpc` `optional = true` and gate
  them behind the `client` and/or `server` features as appropriate.

**Trade-offs**:
- **Gaining**: Default `cargo check -p neo-rpc` (no features) now compiles
  only `neo-primitives` + `neo-error` instead of 14 crates. Client-only
  builds skip 4 server-only crates (neo-network, neo-state-service,
  neo-blockchain, neo-mempool). Oracle service has 2 fewer dependencies.
- **Giving up**: Feature lists in Cargo.toml are longer and must be kept
  in sync when new deps are added. This is a documentation burden, not a
  technical one.
- **Reversibility**: High — adding a dep back to `[dependencies]` is
  trivial.

**Consequences**:
- `neo-oracle-service/Cargo.toml`: 2 deps removed
- `neo-mempool/Cargo.toml`: 1 dep moved to `[dev-dependencies]`
- `neo-rpc/Cargo.toml`: 12 deps made `optional = true`, feature lists updated
- `neo-rpc/src/client/mod.rs`: `parse_script_hash_or_address_inner` import
  gated behind `#[cfg(feature = "server")]` (only used by server module)
- `neo-rpc/src/client/utility.rs`: Same import gated in re-export

### ADR-007: Rename NeoEngine trait to EngineApi

**Status**: Accepted

**Context**: The `NeoEngine` trait in `neo-runtime` (L3) defined the
engine-API surface — the typed entry point used by the consensus driver
to ask the execution layer to apply a block. The name collided with the
`neo-engine` crate (also L3), which defines the `EnginePipeline` trait
for block processing pipeline orchestration. This created a naming
hazard: `NeoEngine::execute_block` (runtime) vs
`EnginePipeline::process_block` (engine) are conceptually adjacent but
live in different crates with confusingly similar names.

At audit time, `NeoEngine` had zero implementations — it was a scaffold
trait. `EnginePipeline` had two implementations (`Pipeline` scaffold +
`BlockchainEngineAdapter` production bridge).

**Decision**: Rename `NeoEngine` to `EngineApi` across all 9 affected
files. The new name matches the doc comment ("Engine-API surface") and
the reth `Engine` trait it was modeled after, while eliminating the
collision with the `neo-engine` crate.

**Trade-offs**:
- **Gaining**: Eliminates the naming collision. `EngineApi` clearly
  describes the consensus↔execution interface. `EnginePipeline` clearly
  describes the processing pipeline. No confusion.
- **Giving up**: The name `NeoEngine` was more "Neo-branded". This is
  cosmetic — the trait doc and reth lineage make the purpose clear.
- **Reversibility**: High — mechanical rename back if needed.

**Consequences**:
- `neo-runtime`: trait renamed in `services.rs`, re-exported from `mod.rs`
  and `lib.rs`, doc references updated in `outcome.rs`, `error.rs`,
  `types.rs` trait bound updated
- `neo-system`: `builder.rs` and `node.rs` updated to use `EngineApi`
- Zero implementations existed, so no impl blocks needed updating
- The `BlockExecutor` trait (also in `neo-runtime`) remains separate —
  it has a different shape (synchronous RPC use) and its name does not
  collide with any crate

### ADR-008: Oracle service layer ownership

**Status**: Superseded by the 2026-07-10 layer audit

**Context**: `neo-rpc` (L6) has a feature-gated dependency on
`neo-oracle-service` behind the `server` feature. The RPC server imports `OracleService`
and `OracleServiceError` to:
1. Access the explicitly configured `OracleService` handle
2. Map `OracleServiceError` variants to JSON-RPC error codes

The original type-map lookup described by this ADR was superseded by ADR-034.

**Decision**: Classify `neo-oracle-service` as a Node Service (L4). It is a
long-running off-chain worker with wallet, retry, signing, and request lifecycle
ownership. `neo-rpc` (L6) therefore consumes it through a normal downward
dependency while Oracle JSON-RPC methods remain in the RPC adapter crate.

**Trade-offs**:
- **Gaining**: RPC server code stays cohesive in `neo-rpc`; the oracle handle is
  a named typed field on `RpcServer`.
- **Giving up**: server builds pull in the Oracle node-service API even when the
  runtime service is disabled; client-only builds remain unaffected.
- **Reversibility**: Medium — moving oracle RPC handlers to `neo-node`
  is possible but would require significant refactoring.

**Consequences**:
- The former L6 lateral edge is removed from the architecture exception list
- `neo-rpc` client-only builds are unaffected (dependency is `optional`)
- Optional Oracle composition uses the named typed field on `RpcServer`; no
  service registry or type-map is reintroduced

### ADR-009: neo-engine pipeline overlap with neo-blockchain

**Status**: Accepted (deferred refactoring)

**Context**: `neo-engine` (L3) defines an abstract `PipelineStage`
trait and `EnginePipeline` trait for block processing orchestration.
`neo-blockchain` (L4) has its own `pipeline` module
(`block_validation`, `native_persist`, `block_processing`) that
implements the same Validate → Execute → Persist → Commit stages
without using `PipelineStage`.

The `BlockchainEngineAdapter` bridges `EnginePipeline` →
`BlockchainHandle`, but the stage-level abstraction (`PipelineStage`)
remains unused by the concrete pipeline implementation.

**Decision**: Accept the overlap as documented future work. The
`neo-engine` crate's docs explicitly acknowledge this:
> "The production implementation lives in `neo-blockchain` (via
> `BlockchainService`), which already orchestrates these stages
> internally. Future refactoring can extract the orchestration logic."

**Trade-offs**:
- **Gaining**: No risky refactoring of the block processing path until
  the staged sync feature is actually needed. The adapter bridge provides
  the trait boundary for external consumers.
- **Giving up**: Two parallel "pipeline stage" vocabularies exist. New
  developers may be confused about which to use.
- **Reversibility**: High — the refactoring can be done incrementally
  when staged sync is prioritized.

**Consequences**:
- `neo-engine::PipelineStage` remains a scaffold trait
- `neo-blockchain::pipeline` remains the production implementation
- `BlockchainEngineAdapter` provides the `EnginePipeline` bridge
- When staged sync is prioritized, extract `neo-blockchain`'s pipeline
  stages into `PipelineStage` implementations

---

## reth & Polkadot Pattern Comparison

neo-rs draws heavily from reth (Rust Ethereum node) and Polkadot/Substrate.
This section documents which patterns are adopted, adapted, or deferred.

### Patterns Adopted from reth

| Pattern | reth location | neo-rs location | Status |
|---------|---------------|-----------------|--------|
| Provider traits | `reth-provider` | `neo-runtime` (StoreProvider, ConfigProvider, TxAdmission; `BlockchainProvider` deleted in ADR-032) | Adopted (BlockchainProvider removed) |
| NodeTypes sealed seam | `reth-node-api` | `neo-runtime/src/node/types.rs` | Adopted (single-impl sealed seam) |
| NodeComponents / FullNode type-state | `reth-node-api` | (removed) | Removed (ADR-032) |
| Engine API trait | `reth-engine` `Engine` | (removed) `EngineApi` | Removed/Superseded (ADR-033) |
| Pipeline stage abstraction | `reth-stages` | `neo-engine` `PipelineStage` | Scaffolded (ADR-010) |
| Service trait vocabulary | `reth-interfaces` | `neo-runtime` `Service` (surviving); `BlockExecutor` etc. removed | Removed/Superseded (ADR-033) |
| `anyhow` in binary, typed errors in libs | `reth-node` | `neo-node` (anyhow), all libs (typed) | Adopted |
| Composition root builder | `reth-node-builder` | `neo-system` `NodeBuilder` | Adopted |
| Feature-gated RPC client/server | `reth-rpc` / `reth-rpc-client` | `neo-rpc` `client`/`server` features | Adopted (ADR-001) |

### Patterns Adopted from Polkadot/Substrate

| Pattern | Polkadot location | neo-rs location | Status |
|---------|--------------------|-----------------|--------|
| Bounded context separation | Parachain runtimes | Layer hierarchy (L0-L7) | Adopted |
| Service trait composition | `sc-service` | `neo-runtime` service traits + `neo-system` composition | Adopted |
| Error type per pallet | `pallet::*::Error` | Domain error per crate (ADR-011) | Adopted (formalized) |

### Patterns Deferred (Not Yet Needed)

| Pattern | Source | Reason for Deferral |
|---------|--------|---------------------|
| RuntimeVersion type | Polkadot `sp_version` | Neo N3 uses hardfork flags, not runtime versioning. ADR-014 proposes adding it for protocol upgrade coordination. |
| 3-crate RPC split | reth `neo-rpc-api`/`neo-rpc`/`neo-rpc-client` | Layer violation already fixed (ADR-001). Feature gating suffices until client-only consumers need lighter deps. |
| Full staged sync | reth `reth-stages` | `PipelineStage` traits defined but unused (ADR-009/010). Migration deferred until fast sync is prioritized. |
| Pallet/frame plugin system | Polkadot `frame` | Native contracts are registry-based (ADR-015 proposes extensibility). Not yet needed. |

### Key Divergence: Neo N3 vs Ethereum

neo-rs diverges from reth in domain-specific ways:
1. **NeoVM vs EVM**: neo-rs has two-crate VM split (`neo-vm-rs` semantics + `neo-vm` stateful host) vs reth's single `revm`. This is because Neo N3's VM is shared with RISC-V and zkVM execution profiles.
2. **dBFT consensus vs PoS**: `neo-consensus` implements dBFT (delegated Byzantine Fault Tolerance) rather than Ethereum's Casper FFG. The consensus crate owns its own message types and protocol state.
3. **Native contracts vs precompiles**: `neo-native-contracts` is a full crate with 11 native contracts (NEO, GAS, Policy, Oracle, etc.) vs reth's inline precompile handling. This reflects Neo's richer native contract system.
4. **MPT for state roots**: Neo uses MPT for state root consensus (not for primary storage). `neo-crypto::mpt_trie` (data structure) + `neo-state-service` (durable store) form a two-layer system (ADR-012).

---

## Architecture Decision Records (continued)

### ADR-010: Pipeline unification strategy

**Status**: Proposed (implementation deferred)

**Context**: ADR-009 documented that `neo-engine` defines `PipelineStage`
traits but `neo-blockchain` implements its pipeline as monolithic
`BlockchainService` methods. The `BlockchainEngineAdapter` bridges
`EnginePipeline` but not `PipelineStage`. This creates two parallel
"pipeline" vocabularies.

The investigation in V3 confirmed:
- `block_validation.rs` is already pure/stateless — closest to a clean stage
- `native_persist.rs` is service-coupled but conceptually maps to Execute/Persist
- `block_processing.rs` is woven into the mpsc command loop — highest effort
- Zero `impl PipelineStage` exist anywhere in the codebase

**Decision**: Adopt a 4-phase incremental migration strategy:

Phase 1 (low effort): Extract `ValidateStage` — wrap `block_validation.rs`
into a struct implementing `ValidateStage`. The validation logic is already
pure and stateless.

Phase 2 (medium effort): Extract `ExecuteStage` — decouple
`native_persist.rs`'s OnPersist/Application/PostPersist loop from
`&self` service state into an `ExecuteStage` struct.

Phase 3 (high effort): Extract `PersistStage` + `CommitStage` — unwind the
snapshot-commit + unverified-block-cache drain from `block_processing.rs`'s
mpsc command loop. This is the most invasive change.

Phase 4: Add `IndexStage` — discrete indexing step (currently inline).

**Trade-offs**:
- **Gaining**: Single pipeline vocabulary. Stage composition enables staged
  sync, parallel stage execution, and independent stage testing. New
  contributors learn one pattern.
- **Giving up**: Significant refactoring effort (Phase 3 is invasive).
  Risk of introducing regressions in the block processing path.
- **Reversibility**: High per-phase. Each stage extraction is independent.

**Consequences**:
- `neo-engine::PipelineStage` becomes the canonical pipeline abstraction
- `neo-blockchain::pipeline` modules become stage implementations
- The `Pipeline` driver struct in `neo-engine` becomes the orchestrator
- `BlockchainEngineAdapter` may become unnecessary once the service itself
  implements `EnginePipeline` via the stage driver

### ADR-011: Error type policy formalization

**Status**: Accepted

**Context**: The V3 audit found that 17 crates have their own domain error
type (`XxxError` + `XxxResult<T>`) while 9 crates use `CoreError`/
`CoreResult` directly. This split was organic, not policy-driven, leading
to uncertainty about which approach to use for new crates.

**Decision**: Formalize the existing split as policy:

**Rule 1 — Domain-specific error crates** (own error type):
A crate MUST define its own error type when it has domain-specific failure
modes that callers need to match on. This includes:
- Crypto operations (`CryptoError`: invalid signature, bad key)
- Storage operations (`StorageError`: key not found, backend failure)
- VM execution (`VmError`: stack overflow, invalid opcode)
- Consensus (`ConsensusError`: timeout, view change)
- Network (`NetworkError`: connection reset, peer disconnect)
- HSM (`HsmError`: device not found, signing failure)
- TEE (`TeeError`: attestation failure, enclave error)

**Rule 2 — Validation/codec crates** (use CoreError):
A crate MAY use `CoreError` directly when its failures are generic
validation or codec errors with no domain-specific variants callers need
to match on. This includes:
- `neo-payloads` (block/tx structural validation)
- `neo-native-contracts` (contract execution delegation)
- `neo-mempool` (admission policy — errors are config-driven)
- `neo-blockchain` (block import orchestration — delegates to sub-errors)
- `neo-state-service` (state root verification — delegates to crypto/storage)
- `neo-manifest` (ABI/NEF parsing — codec-style errors)
- `neo-execution` (ApplicationEngine — delegates to VM)
- `neo-serialization` (partial — has `JsonError` but uses CoreResult for codecs)

**Rule 3 — All domain error types MUST implement `From<DomainError> for CoreError`**
for seamless `?` propagation across crate boundaries (ADR-003).

**Trade-offs**:
- **Gaining**: Clear rule for new crates. Callers of domain-specific crates
  can match on specific variants. Generic crates avoid boilerplate error types.
- **Giving up**: Two patterns exist. This is accepted — forcing one pattern
  would either add boilerplate (own error for codec crates) or lose type
  information (CoreError for crypto/storage).
- **Reversibility**: High — the policy documents the status quo.

**Consequences**:
- The 17/9 split is now policy, not accident
- New crates follow the decision tree: "Does the caller need to match on
  domain-specific variants?" → yes = own error, no = CoreError
- `neo-serialization` is the only partial case (has `JsonError` for JSON,
  uses `CoreResult` for binary codecs) — accepted as-is

### ADR-012: MPT layering documentation

**Status**: Accepted

**Context**: The V3 audit investigated whether `neo-crypto::mpt_trie` and
`neo-state-service::storage::mpt_store` are duplicate MPT implementations.
Finding: they are **layered, not duplicated**.

- `neo-crypto::mpt_trie` — generic MPT data structure (Node, NodeType,
  Trie, MptCache, MptStoreSnapshot trait). No durable backend. Used by
  `neo-state-service` AND directly by `neo-rpc` (proof verification).
- `neo-state-service::storage::mpt_store` — durable MPT store built ON TOP
  of `neo-crypto::mpt_trie`. Adds `MptStore`, `MptChange`, `MptReadSnapshot`
  with snapshot/commit semantics over `neo-storage`.

**Decision**: Document the layering as intentional. No code changes needed.

**Trade-offs**:
- **Gaining**: Clear ownership: `neo-crypto` owns the data structure,
  `neo-state-service` owns the durable store. The data structure is
  reusable (already used by `neo-rpc` independently).
- **Giving up**: Two crates touch MPT. This is correct layering, not a smell.
- **Reversibility**: N/A — documentation only.

**Consequences**:
- The MPT layering is documented in `design.md` and crate boundary docs
- Future contributors should not "consolidate" the two — they serve
  different abstraction levels
- `neo-crypto::mpt_trie` must remain backend-agnostic (no `neo-storage` dep)

### ADR-013: doc(html_root_url) version management

**Status**: Accepted

**Context**: V3 audit found 11 crates with `#![doc(html_root_url = ".../0.9.0")]`
while the workspace version is 0.10.0. The version drift means rustdoc links
point to stale documentation.

**Decision**: Fix all 11 crates to 0.10.0 (completed in this session).
Going forward, the version in `doc(html_root_url)` MUST match
`workspace.package.version` in the root `Cargo.toml`.

If version drift recurs, consider one of:
1. A CI check that greps for mismatched versions
2. Removing the attribute entirely (rustdoc can auto-detect)
3. A `build.rs` script that injects the version

**Trade-offs**:
- **Gaining**: Correct documentation links. Consistent versioning.
- **Giving up**: Manual maintenance until a CI check or build script is added.
- **Reversibility**: High — attribute is cosmetic.

**Consequences**:
- All 11 `doc(html_root_url)` attributes now say 0.10.0
- Version sync is a documented release checklist item

### ADR-014: RuntimeVersion type

**Status**: Proposed (long-term)

**Context**: Neo N3 currently uses hardfork flags (e.g., `HF_Basilisk`,
`HF_Cockatrice`) for protocol upgrade gating. Polkadot uses a
`RuntimeVersion` struct for protocol-level version coordination. As neo-rs
matures and supports multiple network configurations (MainNet, TestNet,
private nets), a formal version type becomes valuable for:
1. Network compatibility checks at P2P handshake
2. Feature gating at runtime (vs compile-time hardfork flags)
3. Protocol negotiation for cross-version testing

**Decision**: Add `RuntimeVersion` to `neo-runtime` when the first of these
triggers occurs:
- Multiple protocol versions need to coexist on the same binary
- P2P layer needs version-based peer filtering
- Cross-network testing requires version-aware test fixtures

**Proposed structure** (when implemented):
```rust
pub struct RuntimeVersion {
    pub spec_version: u32,
    pub impl_version: u32,
    pub authoring_version: u32,
    pub active_hardforks: Vec<Hardfork>,
}
```

**Trade-offs**:
- **Gaining**: Protocol version coordination. Runtime feature gating.
  P2P compatibility checks.
- **Giving up**: One additional type to maintain. Version bumps must be
  coordinated with hardfork activation.
- **Reversibility**: High — additive type.

**Consequences**:
- Deferred until triggered by multi-version support needs
- `neo-config` already has `ProtocolConfig` which could host the version
- The hardfork flag system remains for compile-time gating

### ADR-015: Native contract registry extensibility

**Status**: Proposed (long-term)

**Context**: `neo-execution` defines `NativeContract` trait and
`NativeRegistry`. `neo-native-contracts` provides the 11 concrete
implementations (NEO, GAS, Policy, Oracle, Notary, StdLib, CryptoLib,
ContractManagement, LedgerContract, RoleManagement, Treasury).

The current registry is populated at startup with a fixed set of contracts.
Adding a new native contract requires:
1. Implementing `NativeContract` trait
2. Adding to the registry initialization in `neo-native-contracts`
3. Re-compiling the entire workspace

**Decision**: Defer a plugin-based registry until there is a concrete need
for runtime-loadable native contracts (e.g., sidechain-specific contracts,
governance-added contracts).

When triggered, the migration path is:
1. Extract `NativeRegistry` initialization into a builder pattern
2. Allow `neo-system::NodeBuilder` to accept additional native contracts
3. Native contracts remain compile-time by default; runtime loading is opt-in

**Trade-offs**:
- **Gaining** (future): Extensible native contract system. Sidechain support.
  Governance-added contracts.
- **Giving up** (future): Indirection in the registry. Potential for
  conflicting contract IDs.
- **Reversibility**: High — builder pattern is additive.

**Consequences**:
- Deferred — the fixed 11-contract set meets all current needs
- The `NativeContract` trait is already well-designed for extensibility
- No architectural changes needed until triggered

---

## Long-Term Evolution Roadmap

The roadmap is organized into 4 phases, ordered by dependency and urgency.
Each phase is independently shippable.

### Phase 1: Polish (Immediate — this session)

| Item | ADR | Effort | Status |
|------|-----|--------|--------|
| Fix doc(html_root_url) version drift | ADR-013 | Trivial | Done |
| Remove redundant inline lints | — | Trivial | Done |
| Document MPT layering | ADR-012 | Docs only | Done |
| Formalize error type policy | ADR-011 | Docs only | Done |
| reth/polkadot comparison analysis | — | Analysis | Done |

### Phase 2: Pipeline Foundation (Near-term — when staged sync is prioritized)

| Item | ADR | Effort | Dependency |
|------|-----|--------|------------|
| Extract `ValidateStage` from neo-blockchain | ADR-010 P1 | Low | None |
| Extract `ExecuteStage` from neo-blockchain | ADR-010 P2 | Medium | P1 |
| Make `neo-engine::Pipeline` the production driver | ADR-010 | Medium | P1+P2 |

This phase unifies the two pipeline vocabularies. After completion,
`neo-blockchain::pipeline` modules implement `neo-engine::PipelineStage`
traits, and the `Pipeline` driver orchestrates them.

### Phase 3: Infrastructure Hardening (Mid-term)

| Item | ADR | Effort | Dependency |
|------|-----|--------|------------|
| Extract `PersistStage` + `CommitStage` | ADR-010 P3 | High | Phase 2 |
| Add `IndexStage` | ADR-010 P4 | Low | P3 |
| Add `RuntimeVersion` type | ADR-014 | Low | Triggered by need |
| `neo-serialization` domain error (Priority 3) | — | Low | None |
| 3-crate RPC split | ADR-001 | Medium | Triggered by need |

### Phase 4: Extensibility (Long-term — when ecosystem demands)

| Item | ADR | Effort | Dependency |
|------|-----|--------|------------|
| Native contract registry builder | ADR-015 | Medium | Triggered by need |
| Full staged sync (parallel stages) | ADR-010 | High | Phase 3 |
| Plugin system for L6 services | — | High | Triggered by need |
| Stateless block validation | — | High | Phase 3 |

### Decision Framework for New Crates

When adding a new crate, apply this checklist:

1. **Layer assignment**: Which layer (L0-L7) does it belong to?
2. **Dependency direction**: Does it only depend on same-or-lower layers?
3. **Error type**: Does it need domain-specific error variants? (ADR-011)
4. **Boundary doc**: Does `lib.rs` have a `## Boundary` section?
5. **Module layout**: Does it follow `errors/` + domain subdirs convention?
6. **Lint policy**: Does it use `[lints] workspace = true`?
7. **Workspace dep**: Is it declared in root `Cargo.toml [workspace.dependencies]`?
8. **Feature gating**: If it has optional functionality, are features properly gated?
9. **Re-exports**: Does `lib.rs` have a clean public API surface?
10. **Doc URL**: Does `doc(html_root_url)` match workspace version? (ADR-013)

---

## Remaining Work (Legacy — superseded by Roadmap above)

> The priorities below are preserved for reference. The phased roadmap
> above is the authoritative plan.

### Priority 1: Full 3-crate RPC split (ADR-001)
Split `neo-rpc` into `neo-rpc-api` (traits) / `neo-rpc` (server) /
`neo-rpc-client` (HTTP client). The layer violation (L6→L5) is already
fixed and feature gating is clean, so this is now an optimization — not
a correctness issue. Defers until the workspace grows or client-only
consumers need a lighter dependency.

### Priority 2: Staged sync pipeline (ADR-009 → ADR-010)
Extract `neo-blockchain`'s pipeline stages into `neo-engine::PipelineStage`
implementations. The `BlockchainEngineAdapter` bridge is in place; the
refactoring can be done incrementally when fast sync is prioritized.
See ADR-010 for the 4-phase migration plan.

### Priority 3: neo-serialization domain error
Give `neo-serialization` its own `SerializationError` type (matching
`neo-crypto`/`neo-storage`/`neo-engine`), and stop leaking `CoreError`
at the public boundary. Currently `neo-serialization` is the only L1b
crate without a domain error type. (Note: ADR-011 accepts the partial
case as policy — `JsonError` for JSON, `CoreResult` for binary codecs.)

### Priority 4: Runtime versioning → ADR-014
Add `RuntimeVersion` type to `neo-runtime` for protocol upgrade coordination.
Deferred until multi-version support is needed.

### Priority 5: Native contract registry → ADR-015
Move native contract registration from `neo-execution` to a dedicated
registry pattern for extensibility. Deferred until runtime-loadable
contracts are needed.

---

## Architecture Audit v4 — Deep-Dive Findings & ADRs

A deep-dive audit was conducted across three dimensions: trait design patterns,
code duplication, and async/concurrency safety. Key findings and fixes:

### Audit Summary

| Dimension | Critical | Major | Minor | Info | Status |
|-----------|----------|-------|-------|------|--------|
| **Trait design** | 1 (Store god trait) | 10 | 6 | 7 | 3 fixed, rest documented |
| **Code duplication** | 1 (HSM redeem script) | 6 | 5 | 4 | 1 fixed, rest documented |
| **Async/concurrency** | 0 | 0 | 4 | 10 | All Info — excellent discipline |

### Fixes Applied in This Session

1. **CRITICAL: neo-hsm redeem script hardcoding** — Replaced hardcoded opcodes
   with delegation to `neo_vm::RedeemScript::signature_redeem_script` (ADR-016)
2. **MAJOR: StorageError→CoreError semantic loss** — Fixed blanket
   `InvalidOperation` mapping to preserve `KeyNotFound→NotFound` (ADR-017)
3. **MAJOR: NetworkError name collision** — Added `P2pError` alias in
   neo-primitives to disambiguate from `neo_network::NetworkError` (ADR-018)
4. **MAJOR: StoreProvider name collision** — Added `StoreFactory` alias in
   neo-storage to disambiguate from `neo_runtime::StoreProvider` (ADR-018)
5. **MAJOR: Inconsistent Debug bounds** — Added `Debug` to `Store`,
   `StoreSnapshot`, `PipelineStage`, `EnginePipeline`,
   `SyncStageCheckpointStore` (ADR-019)

### ADR-016: neo-hsm redeem script delegation

**Status**: Accepted (implemented)

**Context**: The `signature_redeem_script` function in
`neo-hsm/src/providers/pkcs11.rs` manually constructed the Neo single-sig
verification script with **hardcoded opcode bytes** (`0x0C`, `0x21`, `0x41`).
The canonical implementation in `neo-vm/src/script_builder/redeem_script.rs`
uses symbolic `OpCode::PUSHDATA1.byte()` and `OpCode::SYSCALL.byte()`. If
opcodes are ever renumbered, the HSM path would silently diverge — producing
wrong validator addresses and wrong `NextConsensus` values.

**Decision**: Added `neo-vm` as a dependency of `neo-hsm` and replaced the
hardcoded implementation with a delegation to
`RedeemScript::signature_redeem_script`.

**Trade-offs**:
- **Gaining**: Single source of truth for redeem script construction. No
  consensus divergence risk from opcode drift.
- **Giving up**: neo-hsm now depends on neo-vm (one additional dependency).
  This is acceptable — neo-hsm already depends on neo-crypto which depends
  on neo-vm's sibling.
- **Reversibility**: High — the function signature is unchanged.

**Consequences**:
- `neo-hsm/Cargo.toml` adds `neo-vm = { workspace = true }`
- `signature_redeem_script` in pkcs11.rs is now a one-line delegation
- The `Crypto::sha256` import for `checksig_hash` is no longer needed in
  the redeem script path (but `Crypto` is still used elsewhere in the file)

### ADR-017: Domain error semantic preservation

**Status**: Accepted (implemented for StorageError)

**Context**: The `From<StorageError> for CoreError` impl collapsed all
StorageError variants to `CoreError::InvalidOperation`, losing semantic
information. Specifically, `StorageError::KeyNotFound` became
`CoreError::InvalidOperation` instead of `CoreError::NotFound`, making it
impossible for callers to distinguish "key not found" from "invalid operation"
after the conversion.

**Decision**: Implement variant-preserving `From` impls for domain errors.
Map each domain variant to the semantically closest CoreError variant:

| StorageError variant | CoreError variant | Rationale |
|---------------------|-------------------|-----------|
| KeyNotFound | NotFound | Same semantic meaning |
| ReadOnly | InvalidOperation | Operation not permitted |
| Serialization | Codec | Codec error category |
| Backend | Io | Backend = I/O layer |
| InvalidOperation | InvalidOperation | Same |
| CommitFailed | InvalidOperation | Operation failure |
| Io | Io | Same |

**Trade-offs**:
- **Gaining**: Callers can now match on `CoreError::NotFound` to handle
  "key not found" uniformly, regardless of which domain error produced it.
- **Giving up**: The `From` impl is more verbose (match instead of blanket).
  This is acceptable — correctness > brevity for error handling.
- **Reversibility**: High — the match can be collapsed back if needed.

**Consequences**:
- `StorageError::KeyNotFound` now converts to `CoreError::NotFound`
- Other domain errors should follow the same pattern when their `From` impls
  are next touched
- The policy is: domain `From` impls MUST map to the semantically closest
  CoreError variant, not blanket `InvalidOperation`

### ADR-018: Name collision resolution via aliases

**Status**: Accepted (implemented)

**Context**: Two name collisions caused `use` ambiguity:
1. `NetworkError` — exists in both `neo-primitives` (P2P protocol errors) and
   `neo-network` (network service errors). Different semantics, same name.
2. `StoreProvider` — exists in both `neo-storage` (store factory) and
   `neo-runtime` (store accessor). Different semantics, same name.

**Decision**: Add type aliases to disambiguate without breaking existing code:
- `neo-primitives`: `pub type P2pError = NetworkError` + `pub type P2pResult<T> = NetworkResult<T>`
- `neo-storage`: `pub trait StoreFactory: StoreProvider {}` (marker alias)

New code should use `P2pError` and `StoreFactory`; existing names remain for
backward compatibility.

**Trade-offs**:
- **Gaining**: New code has unambiguous names. No breaking changes.
- **Giving up**: Two names for the same type. This is a transitional cost
  until all consumers migrate to the preferred names.
- **Reversibility**: High — aliases are additive.

**Consequences**:
- `P2pError` / `P2pResult` re-exported from neo-primitives
- `StoreFactory` exported from neo-storage persistence traits
- Existing `NetworkError` and `StoreProvider` names unchanged

### ADR-019: Consistent Debug bounds on traits

**Status**: Accepted (implemented)

**Context**: The trait audit found inconsistent `Debug` bound requirements:
runtime-layer traits (`Service`, `BlockchainProvider`, `StoreProvider`,
`ConfigProvider`, `TxAdmission`) require `Debug`, but storage and engine
traits (`Store`, `StoreSnapshot`, `PipelineStage`, `EnginePipeline`,
`SyncStageCheckpointStore`) did not. This meant `Arc<dyn Store>` could not be
formatted with `{:#?}` in logs.

**Decision**: Add `std::fmt::Debug` as a supertrait bound to:
- `Store` (neo-storage)
- `StoreSnapshot` (neo-storage)
- `PipelineStage` (neo-engine)
- `EnginePipeline` (neo-engine)
- `SyncStageCheckpointStore` (neo-runtime)

Added manual `Debug` impls to all concrete implementations: `MemoryStore`,
`MemorySnapshot`, `MdbxStore`, `MdbxSnapshot`, `RocksDbStore`,
`RocksDbSnapshot`, `Pipeline`, and test `OverlayContractStore`.

**Trade-offs**:
- **Gaining**: All `Arc<dyn Trait>` service objects can now be logged with
  `{:#?}`. Consistent trait bounds across all layers. Better observability.
- **Giving up**: Every concrete implementation must implement `Debug`. For
  types wrapping non-Debug FFI types (MDBX `Database`, RocksDB `DB`), manual
  `finish_non_exhaustive()` impls are needed. This is a small one-time cost.
- **Reversibility**: High — removing a supertrait bound is additive.

**Consequences**:
- All 5 traits now require `Debug`
- All 8 concrete implementations now have `Debug` impls
- `Arc<dyn Store>`, `Arc<dyn EnginePipeline>`, etc. can be logged uniformly

---

### ADR-020: Store capabilities without dynamic extension traits

**Status**: Accepted (implemented)

**Context**: The earlier ADR-020 split fast-sync and raw-overlay behavior into
`FastSyncStore` / `RawOverlayStore` extension traits exposed through
`as_*() -> Option<&dyn ...>` accessors. That reduced the original store surface,
but it also reintroduced dynamic dispatch and downcast-like capability probing
on a hot storage boundary. The storage backends are a fixed production set
(`memory`, `mdbx`, `rocksdb`) behind `StoreFactory`, so dynamic extension
objects were unnecessary.

**Decision**: Keep one generic `Store` boundary and expose optional backend
capabilities as direct default methods:
- `backend_kind()`, `mdbx_environment_info()`, and `rocksdb_batch_metrics()`
  replace concrete-store downcasts for diagnostics.
- `supports_fast_sync_mode()`, `enable_fast_sync_mode()`,
  `disable_fast_sync_mode()`, `discard_pending_fast_sync_writes()`, and
  `has_pending_fast_sync_writes()` replace `FastSyncStore`.
- `try_commit_raw_overlay()` and `try_commit_borrowed_raw_overlay()` replace
  `RawOverlayStore`.
- Unsupported backends keep no-op/`Ok(false)` defaults; concrete stores
  override the methods they actually support.
- The `Store::as_any`, `StoreProvider::as_any`, `FastSyncStore`, and
  `RawOverlayStore` APIs are deleted.

**Trade-offs**:
- **Gaining**: Callers remain generic over `S: Store` and no longer ask for
  `&dyn` extension handles. Backend metrics and fast-sync/overlay behavior use
  one consistent provider/capability pattern.
- **Giving up**: The `Store` trait is larger than the temporary extension-trait
  split, but the methods are all no-op defaults and remove runtime capability
  object plumbing.
- **Reversibility**: Medium — reintroducing extension traits would be
  mechanical, but it would deliberately re-add dynamic dispatch at the storage
  boundary.

**Consequences**:
- Storage callers use direct generic methods instead of `as_*()` accessors.
- MDBX and RocksDB telemetry no longer downcast concrete stores.
- `RuntimeStore` forwards capabilities through enum dispatch.
- Test and benchmark stores implement the same `Store` methods as production
  stores.

### ADR-021: Trait sealing (NodeTypes, NodeComponents, EngineApi)

**Status**: Accepted (implemented)

**Context**: reth seals its composition/protocol traits (`NodeTypes`,
`NodeComponents`, `Engine`) using a private `Sealed` supertrait to prevent
external implementations. neo-rs had 0 of 11 key traits sealed, meaning
external crates could implement `NodeTypes` or `EngineApi` — a contract
violation since these are internal composition surfaces.

**Decision**: Seal three traits using the private `Sealed` supertrait pattern:
- `NodeTypes` — only `NeoNodeTypes` (in neo-runtime) can implement it
- `NodeComponents<N>` — only internal types can implement it
- `EngineApi` — only internal types can implement it

Leave unsealed (documented extension points):
- `Store` — "Developers should implement this interface to provide new storage
  engines" (docstring)
- `Service` — marker trait, sealing defeats purpose
- `PipelineStage` — extension point for stage plugins
- `StoreProvider` (both) — extension points for storage backends
- `BlockchainProvider`, `ConfigProvider` — RPC decoupling surface
- `EnginePipeline` — has cross-crate impl (`BlockchainEngineAdapter` in
  neo-blockchain), cannot seal with private module

**Trade-offs**:
- **Gaining**: Locks the composition surface. External crates cannot implement
  `NodeTypes` or `EngineApi`, preventing accidental contract violations.
  Matches reth convention.
- **Giving up**: `EnginePipeline` cannot be sealed due to cross-crate impl
  (neo-blockchain's `BlockchainEngineAdapter`). This is a Rust limitation —
  sealing requires the `Sealed` trait to be in the same crate as the impl.
- **Reversibility**: Medium — unsealing is additive but would require removing
  the `Sealed` supertrait.

**Consequences**:
- 3 traits sealed, 8 left open (documented extension points)
- `NeoNodeTypes` gets `impl sealed::Sealed for NeoNodeTypes {}`
- No external API breakage (no external impls existed)

### ADR-022: Dead KeyBuilder wrapper removal

**Status**: Accepted (implemented)

**Context**: The `neo_execution::KeyBuilder` was a newtype wrapper around
`neo_storage::KeyBuilder` that added `add_ecpoint` (a one-liner calling
`self.add(key.as_bytes())`). It had zero production callers — only its own
unit test used it. This was one of four parallel key-builder implementations
(A: `neo_storage::KeyBuilder`, B: `neo_execution::KeyBuilder` [dead],
C: `support::keys` free functions, D: `StorageKey::create_with_*`).

**Decision**: Delete the `neo_execution::KeyBuilder` wrapper entirely. The
`add_ecpoint` convenience method is not moved — callers can use
`builder.add(ecpoint.as_bytes())` directly (one-liner).

**Trade-offs**:
- **Gaining**: Removes dead code. Eliminates one of four parallel key-builder
  systems. Reduces `neo-execution` public API surface.
- **Giving up**: The `add_ecpoint` convenience method is gone. Any future
  caller that needs it must use `builder.add(ecpoint.as_bytes())` or add the
  method to `neo_storage::KeyBuilder` (which would require adding
  `neo-crypto` as a dependency to `neo-storage`).
- **Reversibility**: High — the wrapper can be re-added if needed.

**Consequences**:
- `neo-execution/src/storage/key_builder.rs` deleted
- `neo-execution/src/tests/storage/key_builder.rs` deleted
- `neo-execution` re-exports updated (removed `KeyBuilder` and `key_builder`)
- Zero production callers affected (there were none)

### ADR-023: NodeComponents type-state migration status

**Status**: Accepted (documented)

**Context**: The v5 audit found that the NodeComponents type-state migration
is non-functional:
- 3 `Option<Arc<dyn BlockExecutor/ConsensusService/EngineApi>>` fields on
  `Node` are always `None` in production
- 3 builder methods (`with_block_executor`, `with_consensus`, `with_engine`)
  have zero call sites in any `.rs` file (only in docs)
- Zero production implementations of `BlockExecutor`, `ConsensusService`, or
  `EngineApi` exist
- Real consensus wiring uses a free-standing `consensus_driver_task()` function
  in `neo-node`, completely bypassing the trait seam
- The `FullNodeComponentsExt` accessor trait is reserved but not exported

The traits (`NodeTypes`, `NodeComponents`, `FullNode`) are defined and exported
but never used as bounds anywhere in the workspace.

**Decision**: Document the current state as "scaffolded, not functional" and
fix the documentation to describe reality:
- Keep `NodeTypes` / `NeoNodeTypes` as a future-proofing seam (cheap to keep,
  matches reth shape)
- Keep `NodeComponents` / `FullNode` / `FullNodeComponentsExt` behind
  `#[allow(dead_code)]` until concrete service impls exist
- Fix docs to describe the runtime `Option<Arc<dyn>>` approach as the actual
  design, not the type-state design as adopted
- The 3 dead `Option<Arc<dyn>>` fields on `Node` and their builder methods are
  kept as future wiring points, not removed

**Trade-offs**:
- **Gaining**: Honest documentation. Future contributors won't be misled into
  thinking type-state composition is "almost done". The dead fields serve as
  visible wiring points for future service implementations.
- **Giving up**: The dead fields and builder methods remain in the codebase,
  adding a small amount of dead code. The type-state traits remain exported
  but unused.
- **Reversibility**: High — implementing concrete service impls and wiring them
  through the trait seam would activate the design without structural changes.

**Consequences**:
- Documentation updated to describe runtime composition as the actual design
- Type-state traits kept as scaffolded future work
- If type-state is eventually desired, start by implementing concrete service
  traits (`impl ConsensusService for DbftConsensusController`, etc.) — that
  work is required either way

### ADR-024: Hex encoding consolidation

**Status**: Accepted (implemented)

**Context**: The v5 audit found hex encoding scattered across 18 crates with
~150+ call sites and no shared utility. The root cause was that
`parse_reversed_hex` and `format_reversed_hex` in `neo-primitives` were
`pub(crate)` — other crates couldn't use them, so they called `hex::encode`/
`hex::decode` directly. This led to:
- Inconsistent prefix stripping (one call site only handled lowercase `0x`)
- Duplicated error mapping at ~10 sites
- 3 competing hex-display mechanisms (`hex::encode`, `impl_display_hex!` macro,
  manual `{:02x}` loops)
- Confusion between Neo reversed-hex and straight hex

**Decision**: Create a public `hex_util` module in `neo-primitives` with:
- `encode_hex(bytes: &[u8]) -> String` — straight hex, lowercase, no prefix
- `decode_hex(s: &str) -> PrimitiveResult<Vec<u8>>` — with prefix stripping + error mapping
- `encode_reversed_hex(bytes: &[u8]) -> String` — Neo hash format (reversed, `0x` prefix)
- `decode_reversed_hex(s: &str) -> PrimitiveResult<Vec<u8>>` — Neo hash format parse
- `encode_hex_upper(bytes: &[u8]) -> String` — uppercase hex (for TLS fingerprints)
- `strip_hex_prefix(s: &str) -> &str` — canonical prefix stripper (already existed, now in hex_util)

Consolidated call sites in:
- `neo-payloads` (signer.rs, witness.rs, witness_rule/helpers.rs) — removed `hex` dep entirely
- `neo-manifest` (contract_group.rs, contract_permission_descriptor.rs) — removed `hex` dep entirely
- `neo-rpc` (signers.rs parameter converter) — fixed prefix-stripping bug

**Trade-offs**:
- **Gaining**: Single source of truth for hex encoding/decoding. Consistent
  prefix stripping. Centralized error mapping. Clear distinction between
  reversed-hex (Neo hash format) and straight hex. 2 crates (`neo-payloads`,
  `neo-manifest`) no longer depend on the `hex` crate directly.
- **Giving up**: Call sites that still use `hex::encode` directly (in neo-crypto,
  neo-wallets, neo-execution, neo-tee, neo-rpc server, neo-oracle-service,
  neo-node, neo-hsm) require gradual migration. The `hex` crate remains a
  workspace dependency for these crates.
- **Reversibility**: High — the `hex_util` functions are thin wrappers over the
  `hex` crate.

**Consequences**:
- `neo-primitives::hex_util` is the canonical hex utility module
- `neo-payloads` and `neo-manifest` removed `hex` from Cargo.toml
- Fixed a latent bug: `signers.rs` only stripped lowercase `0x`, missing `0X`
- Remaining ~130 call sites can be migrated incrementally

### ADR-025: KeyBuilder system documentation and cleanup

**Status**: Accepted (implemented)

**Context**: After ADR-022 removed the dead `neo_execution::KeyBuilder` (system B),
three key-builder systems remained:
- **A**: `neo_storage::KeyBuilder` — low-level byte builder with length enforcement.
  Had zero production callers but was kept as reference implementation.
- **C**: `neo-native-contracts/support/keys` — ergonomic typed free functions.
  The production standard, enforced by a style test. 35+ call sites.
- **D**: `StorageKey::create_with_*` constructors — retained for test fixtures
  and `create_search_prefix` (range scans). Forbidden in production by style test.

A 4th ad-hoc path existed in `neo-rpc/src/server/rpc_server_state/mod.rs` that
hand-rolled key bytes with `Vec::with_capacity` + `extend_from_slice`.

**Decision**:
1. **Document the three-system coexistence** in `support/keys.rs` module docs.
   Each system has a distinct role:
   - A: low-level reference implementation (kept, zero callers)
   - C: production standard for native contracts (ergonomic, enforced)
   - D: test fixtures + range-scan helper (retained, forbidden in prod)
2. **Make C's internal helpers private**: The 8 `Vec<u8>`-producing functions
   (`prefixed`, `prefixed_with_hash160`, `prefixed_with_hash256`, etc.) were
   `pub` but only called within `keys.rs` itself. Made them private (`fn` not
   `pub fn`) and inlined the logic into the `*_key` variants.
3. **Fix the 4th ad-hoc path**: Replaced the hand-rolled `Vec::with_capacity`
   in `rpc_server_state/mod.rs` with `StorageKey::create_with_uint160(...).to_array()`.
4. **Updated tests**: Removed tests for the now-private `Vec<u8>` helpers;
   kept the oracle test that proves C and D produce byte-identical keys.

**Trade-offs**:
- **Gaining**: Clearer API surface (8 fewer public functions in keys.rs). No
  hand-rolled key construction anywhere in the workspace. Documented
  three-system architecture.
- **Giving up**: The 8 `Vec<u8>` helpers are no longer available for external
  callers. No external callers existed, so no breakage.
- **Reversibility**: High — making functions public again is additive.

**Consequences**:
- `support/keys.rs` public API reduced from 16 to 8 functions
- Zero hand-rolled key construction in production code
- Three-system coexistence documented and justified

### ADR-026: Concrete ValidateStage extraction

**Status**: Accepted (implemented, not wired into live command loop)

**Context**: ADR-010 defined a phased pipeline-unification roadmap, but
`neo-engine::ValidateStage` still had no concrete implementation in
`neo-blockchain`. Existing validation was usable but remained split between
pure `BlockValidator` helpers and service-level logic, so the stage abstraction
was scaffolded rather than testable.

**Decision**: Add `neo_blockchain::pipeline::validate_stage::NeoValidateStage`,
a concrete implementation of both `neo_engine::ValidateStage` and
`neo_engine::PipelineStage`.

The stage has a narrow `ValidateContext` dependency surface:
- protocol settings
- previous block hash lookup
- previous block timestamp lookup
- validator count

Validation is split into:
1. **Stateless checks**: version, serialized block size, protocol transaction
   count, merkle root, duplicate transaction hashes, header witness scripts,
   transaction witness scripts.
2. **Stateful checks**: primary index, timestamp bounds/progression, previous
   hash chaining, next-height sequencing.

**Trade-offs**:
- **Gaining**: First concrete pipeline stage in `neo-blockchain`; validation can
  now be tested independently of the service command loop. The context trait is
  intentionally narrow, so wiring later should not require exposing full service
  internals.
- **Giving up**: The stage is extracted but not yet wired into live block import,
  avoiding a risky behavior change in the same iteration. Existing service paths
  continue to use their current validation flow.
- **Reversibility**: High — the new stage is additive and isolated.

**Consequences**:
- `NeoValidateStage` is available from `neo_blockchain::pipeline::validate_stage`
  and re-exported from the crate pipeline surface.
- 6 focused unit tests cover stage identity, valid child validation, height
  mismatch, previous-hash mismatch, protocol transaction limit, and invalid
  header witness rejection.
- ADR-010 Phase 1 is complete at extraction level; next phase is service-loop
  wiring behind the existing import/verify policy.

---

## Verification

| Check | Status |
|-------|--------|
| `cargo check --workspace` | Clean (0 errors) |
| `cargo check -p neo-rpc` (no features) | Clean (0 errors, only 2 deps) |
| `cargo check -p neo-rpc --features client` | Clean (0 errors, 0 warnings) |
| `cargo check -p neo-rpc --features server` | Clean (0 errors, 0 warnings) |
| `cargo test --workspace` | 3,356 tests, 0 failures |
| `cargo test -p neo-rpc --features server` | 654 tests, 0 failures |
| Layer boundary tests | 20/20 pass |
| Error handling consistency | All 14 domain errors have From impls |
| Error type policy | Formalized (ADR-011): 17 domain, 9 CoreError |
| Error semantic preservation | StorageError→CoreError preserves variants (ADR-017) |
| Layer violations | All resolved (V1 oracle, V2 rpc) |
| Dead dependencies | All removed (ADR-006) |
| Feature gating | All conditional deps properly gated |
| Circular dependencies | None — graph is a valid DAG (29 nodes) |
| Lateral violations | None inappropriate (L6 lateral documented in ADR-008) |
| Crate responsibility overlap | 3 documented (ADR-007, ADR-008, ADR-009) |
| Naming collisions | Resolved (NeoEngine→EngineApi ADR-007, P2pError/StoreFactory ADR-018) |
| MPT layering | Documented (ADR-012) — not duplication |
| doc(html_root_url) versions | All 11 crates at 0.10.0 (ADR-013) |
| Redundant inline lints | Removed (tokens_tracker module) |
| reth/polkadot comparison | Documented (8 patterns adopted, 4 deferred) |
| Evolution roadmap | 4 phases, full ADR log (ADR-001 through ADR-036) |
| Debug trait bounds | Consistent across all service traits (ADR-019) |
| HSM redeem script | Delegates to canonical neo-vm impl (ADR-016) |
| Async/concurrency safety | Excellent — 0 Critical/Major issues (ADR audit) |
| Backpressure handling | Bounded channels + try_send slow-peer isolation |
| Mempool TOCTOU safety | Atomic check-verify-act under write lock |
| Store trait surface | Direct generic capability methods; no Store downcast or dynamic extension traits (ADR-020) |
| Trait sealing | NodeTypes, NodeComponents, EngineApi sealed (ADR-021) |
| Dead KeyBuilder wrapper | Removed (ADR-022) |
| NodeComponents type-state | Documented as scaffolded, not functional (ADR-023) |
| Hex encoding | Canonical hex_util module in neo-primitives (ADR-024) |
| KeyBuilder systems | 3-system coexistence documented, ad-hoc path fixed (ADR-025) |
| ValidateStage extraction | Concrete `NeoValidateStage` added and tested (ADR-026) |
| Static files | Old orphan deleted in ADR-027; production-consumed replacement accepted in ADR-036 |
| Dead crate `neo-engine` | Deleted — traits moved to `neo-blockchain::pipeline::stage_traits` (ADR-027) |
| Dead trait excision | 6 dead traits + 1 marker trait deleted (ADR-027) |
| Native contract codec layer | `support/codec.rs` + `support/engine.rs` + `support/settings.rs` consolidate ~265 lines of duplicated boilerplate (ADR-028) |
| Cross-crate helpers + test fixtures | Shared `now_millis`, `elapsed_us`, `invocation_from_signature`, `impl_error_from_struct!` macro, `neo-test-fixtures` crate (ADR-029) |

---

### ADR-029: Cross-crate helpers + test fixtures

**Status**: Accepted (implemented)

**Context**: The deep audit (`.planning/codebase/deep-audit-2026-07-04.md` Theme D
+ E) found ~250 lines of duplicated utility code scattered across 4-5 crates:

1. **Epoch-millisecond clock** — 3 copies of `now_millis()` / `current_timestamp()`
   in `neo-consensus`, `neo-node` (×2), each with `SystemTime::now().duration_since(
   UNIX_EPOCH).unwrap_or_default().as_millis() as u64`.

2. **Elapsed-microsecond timing** — 6 sites in `neo-blockchain::pipeline` used
   `as_micros() as u64` which silently truncates u128→u64 on overflow (only
   relevant for durations >584M years, but still a code smell).

3. **Signature invocation script** — 3 copies of `invocation_script_from_signature`
   (2 hand-rolling `PUSHDATA1` bytes, 1 using `ScriptBuilder::emit_push`) and 2
   copies of the inverse `signature_from_invocation_script`, across
   `neo-consensus` and `neo-node` (×2).

4. **Domain→CoreError From impls** — 13 mechanical `impl From<DomainError> for
   CoreError { fn from(err) -> Self { CoreError::Variant { message:
   err.to_string() } } }` blocks across 7 crates. The existing
   `impl_error_from!` macro only supported tuple-variant constructors.

5. **Test fixtures** — `make_transaction`, `try_make_ledger_block`, `try_store_block`
   duplicated between two `neo-rpc` test files (~120 lines each, with slightly
   different defaults).

**Decision**:

1. **`now_millis()`** — added to `neo-primitives/src/utils/time.rs` (an existing
   `time` module already re-exported at the crate root as `neo_primitives::time`).
   3 call sites migrated. `neo-consensus::current_timestamp()` kept as a thin
   delegate to preserve its private API.

2. **`elapsed_us()` / `elapsed_millis()`** — new `neo-runtime/src/time.rs` module
   with saturating u128→u64 conversion (`.min(u64::MAX as u128) as u64`).
   6 call sites in `neo-blockchain::pipeline` migrated.

3. **`invocation_from_signature()` / `signature_from_invocation()`** — added to
   `neo-vm/src/script_builder/mod.rs`. The forward direction is a method on
   `ScriptBuilder` (delegates to `emit_push`); the inverse is a standalone
   function checking `PUSHDATA1 0x40 <64 bytes>`. Verified byte-identical to
   both the hand-rolled and ScriptBuilder-based originals. 5 call sites migrated
   (3 forward + 2 inverse). `neo-consensus::InvocationScript` struct kept as a
   thin delegate.

4. **`impl_error_from_struct!` macro** — new macro in `neo-io/src/core/macros.rs`
   alongside the existing `impl_error_from!`, re-exported from `neo-error`.
   Supports struct-variant constructors: `CoreError::Variant { message:
   err.to_string() }`. 13 domain error impls migrated across 7 crates.
   `neo-storage::StorageError` skipped (has custom `match` logic, not pure
   message conversion). **Bug fix**: also fixed `impl_error_from!` to use
   `Self::` instead of `<$error_type>::` which triggered E0658 (experimental
   qualified path syntax) for path-typed `$error_type`.

5. **`neo-test-fixtures` crate** — dev-only workspace member providing
   `TestTransactionBuilder` (fluent builder with sensible defaults), plus
   fallible `try_make_ledger_block()` and `try_store_block()` /
   `try_store_block_with_vmstate()` helpers. Test files keep any intentional
   fixture assertions at their call sites, while the shared crate propagates
   storage and serialization failures. The crate is classified as Layer 7
   (Application) in layer boundary tests since it's test infrastructure, not
   production code.

**Trade-offs**:
- **Gaining**: ~250 lines of duplication eliminated. Saturating conversion is
  strictly safer than truncating. New contracts/tests follow a clear pattern via
  `TestTransactionBuilder` and `impl_error_from_struct!`. Future contributors
  have one canonical location for each utility.
- **Giving up**: One indirection layer per utility call. The `neo-test-fixtures`
  crate adds a 27th workspace member (test-only, no production weight).
- **Reversibility**: High — each call site can be inlined back.

**Consequences**:
- `neo_primitives::time::now_millis()` is the canonical epoch-millisecond clock
- `neo_runtime::time::elapsed_us/elapsed_millis` is the canonical elapsed-timing helper
- `neo_vm::ScriptBuilder::invocation_from_signature` + `neo_vm::signature_from_invocation` are the canonical signature script helpers
- `neo_error::impl_error_from_struct!` is the canonical macro for struct-variant CoreError conversions
- `neo-test-fixtures` is the canonical location for shared test builders
- Workspace: 26 production crates + 1 test crate = 27 workspace members
- 3346 tests pass (matches baseline — zero regressions)
- Architecture health score: 9.5 → 9.5 (consolidation, no structural change)

---

### ADR-030: neo-hsm default features, ConsensusApi rename, Nep17MetadataReader extraction

**Status**: Accepted (implemented)

**Context**: Three independent architecture-honesty fixes from the deep audit
(Theme B4 + G1 + G3):

1. **B4 — `neo-hsm` default FFI** (Finding 12): `neo-hsm` had
   `default = ["pkcs11"]`, pulling PKCS#11 FFI (cryptoki crate) into any
   consumer that didn't specify `default-features = false`. A signing-backend
   crate should not have FFI on by default (compare `neo-tee` which defaults
   to simulation).

2. **G1 — `ConsensusService` name collision** (Finding 3):
   `neo-runtime/src/service/services.rs:139` defined
   `pub trait ConsensusService: Service` while
   `neo-consensus/src/service/core.rs:12` defined
   `pub struct ConsensusService` (the real dBFT state machine). The trait had
   0 implementations. Exact analogue of ADR-007's `NeoEngine → EngineApi` fix.

3. **G3 — `neo-wallets` → `neo-execution` coupling** ( Finding 4):
   `neo-wallets` (L4) depended on `neo-execution` (L3) solely for
   `AssetDescriptor` which queries NEP-17 `symbol`/`decimals` by running a
   contract call through `ApplicationEngine`. This transitively forced every
   `neo-wallets` consumer to compile the full execution engine.

**Decision**:

**B4**: Flip `neo-hsm` default to `[]`. Update `neo-node`'s `hsm` feature to
`["dep:neo-hsm", "neo-hsm/pkcs11"]` so enabling `neo-node`'s `hsm` feature
still activates PKCS#11. The 4 PKCS#11 tests in `neo-hsm` now require
`cargo test -p neo-hsm --features pkcs11` to run (expected — they test FFI
code that shouldn't compile by default).

**G1**: Rename the trait `ConsensusService` → `ConsensusApi` in `neo-runtime`.
Update all 8 referencing files (trait def, re-exports, `NodeComponents`
associated type, builder, node, test). Do NOT rename the `ConsensusService`
struct in `neo-consensus` — that's the real implementation.

**G3**: Extract `Nep17MetadataReader` trait in `neo-runtime/src/service/nep17.rs`
with a single `read_metadata(contract_hash) -> Result<Nep17Metadata, ServiceError>`
method (returns both symbol + decimals in one call to preserve the single-VM-
execution behavior from C#). The concrete impl `Nep17MetadataReaderImpl` lives
in `neo-execution/src/nep17_reader.rs` (natural home for `ApplicationEngine`).
`neo-wallets` now depends on `neo-runtime` (light, trait-only) instead of
`neo-execution` (heavy, full VM engine). `neo-execution` gained `neo-runtime`
as a dependency (one-way, no cycle).

**G4 (skipped)**: `StoreProvider`/`ConfigProvider` impls on `Node` vs
`NodeContext` have trivial 1-line forwarding bodies — not worth abstracting.

**Trade-offs**:
- **Gaining**: `neo-hsm` no longer footguns consumers with FFI by default.
  `ConsensusService` collision resolved (matches ADR-007 precedent). `neo-wallets`
  compile time drops (no longer pulls execution engine transitively). The
  trait seam enables future mock implementations for wallet testing.
- **Giving up**: The 4 PKCS#11 tests don't run in default `cargo test --workspace`
  (they require `--features pkcs11`). `neo-execution` gained `neo-runtime` as
  a dependency (acceptable — one-way, and `neo-runtime` is lightweight).
- **Reversibility**: High for all three changes.

**Consequences**:
- `neo-hsm` default features: `[]` (was `["pkcs11"]`)
- `neo-node` hsm feature: `["dep:neo-hsm", "neo-hsm/pkcs11"]`
- `neo_runtime::ConsensusApi` is the canonical trait name (was `ConsensusService`)
- `neo_runtime::Nep17MetadataReader` is the canonical NEP-17 metadata trait
- `neo_execution::Nep17MetadataReaderImpl` is the canonical concrete impl
- `neo-wallets` no longer directly depends on `neo-execution` (moved to dev-deps)
- Workspace default test count: 3342 (was 3346 — 4 PKCS#11 tests now behind
  `--features pkcs11`, all pass when feature is enabled)
- Architecture health score: 9.5 → 9.5

---

### ADR-031: Async ConsensusSigner + await_wallet_future deadlock fix

**Status**: Accepted (implemented)

**Context**: The deep audit (`.planning/codebase/deep-audit-2026-07-04.md` Theme F)
found two async/blocking correctness issues:

1. **F1 — Sync `ConsensusSigner::sign` blocking the tokio worker** (Finding F1):
   `ConsensusSigner::sign` was `fn sign(&self, ...) -> ConsensusResult<Vec<u8>>`
   (sync), but the production signers make blocking network/HSM round-trips:
   - `AzureKeyVaultSigner` used `reqwest::blocking::Client` with a 10s timeout
   - `NitroEnclaveSigner` does a VSOCK transport request (blocks)
   - `Pkcs11Signer` does C FFI calls to HSM hardware (blocks)
   - `GcpKmsSigner` is a stub (no blocking)

   The `ConsensusService` state machine is sync and called directly from the
   neo-node async `tokio::select!` loop. When `sign()` was called, it blocked
   the tokio worker thread for up to 10s (Azure timeout), starving all other
   async tasks on that worker.

2. **F2 — `await_wallet_future` deadlock risk** (Finding F2):
   `neo-rpc/src/server/rpc_server_wallet/mod.rs:264` took a
   `Pin<Box<dyn Future>>` and, when the host was a current-thread runtime,
   spawned a fresh `CurrentThread` runtime per call. If the wallet future
   depended on the parent runtime's reactor (e.g. a `tokio::time::Sleep` or
   an mpsc receiver), the parent thread would block waiting for the spawned
   runtime, which in turn could never drive the parent's resources — a silent
   deadlock.

**Decision**:

**F1**: Make `ConsensusSigner::sign` async via `#[async_trait]`. Cascade the
async signature through the entire ConsensusService call chain:

- `ConsensusSigner::sign` → `async fn sign` (trait + `Arc<dyn>` blanket impl)
- `ConsensusService::sign` / `sign_block_hash` / `create_payload` → `async fn`
- All handlers that sign: `on_prepare_request`, `on_prepare_response`,
  `send_prepare_response`, `check_prepare_responses`, `on_commit`,
  `on_change_view`, `request_change_view`, `request_recovery`, `change_view`,
  `broadcast_change_agreement`, `on_recovery_request`, `on_recovery_message`,
  `reprocess_recovery_payload`, `maybe_send_recovery_response`,
  `resend_recovery_message`, `on_transactions_received` → all `async fn`
- `process_message`, `on_timer_tick`, `resume`, `resume_with_next_consensus`
  → `async fn`
- neo-node driver: `.await` added to all async ConsensusService calls

Signer implementation strategy:
- **Software signer** (local ECDSA): stays sync inside async fn (< 1ms CPU,
  not wrapped in `spawn_blocking`)
- **NitroEnclaveSigner**: `spawn_blocking` wrapping the blocking VSOCK transport
- **Pkcs11Signer**: `spawn_blocking` wrapping the mpsc channel to the HSM worker
- **AzureKeyVaultSigner**: switched from `reqwest::blocking::Client` to async
  `reqwest::Client` (native async I/O, no `spawn_blocking` needed)
- **GcpKmsSigner**: trivially async (stub)

**F2**: Remove the `RuntimeFlavor::CurrentThread` spawn path from
`await_wallet_future`. Now always uses `block_in_place` when a runtime handle
exists. `block_in_place` panics on a current-thread runtime — this is a loud,
immediate failure that tells the operator to use a multi-thread runtime,
rather than a silent hang. When no runtime is present, creates a temporary
current-thread runtime (safe — no parent reactor to deadlock against).

**Trade-offs**:
- **Gaining**: HSM/network signers no longer block the tokio worker thread.
  Azure signer uses native async I/O. The `await_wallet_future` deadlock path
  is eliminated. The async trait seam enables future async signer backends
  (e.g. a native GCP KMS async client) without another trait change.
- **Giving up**: `async_trait` adds a `Pin<Box<dyn Future>>` allocation per
  `sign()` call (acceptable — signing is called ~once per block, not in a
  hot loop). The entire ConsensusService method chain is now async, which
  means all test functions touching it are `#[tokio::test] async fn`. The
  `block_in_place` panic on current-thread runtime is a deliberate trade:
  loud failure > silent deadlock.
- **Reversibility**: Medium — the async cascade touches ~20 files. Reverting
  would require re-adding `spawn_blocking` at each call site or going back to
  sync blocking.

**Consequences**:
- `ConsensusSigner::sign` is `async fn` (was `fn`)
- All ConsensusService methods that transitively sign are `async fn`
- `neo-consensus` gained `async-trait` dependency
- `neo-hsm` reqwest switched from `blocking` to async; gained `tokio` (rt) dep
- `neo-tee` tokio gained `macros` + `rt` features
- `AzureKeyVaultSigner::new` uses async reqwest builder
- `await_wallet_future` no longer spawns a CurrentThread runtime (deadlock fix)
- 12 test files converted from `#[test] fn` to `#[tokio::test] async fn`
- Workspace test count: 3343 (was 3342 — 1 new test for multi-thread runtime)
- All test suites pass: workspace 3343, neo-consensus 137, neo-rpc server 655,
  neo-hsm pkcs11 4, neo-tee nitro 93, layer_boundary 20
- Architecture health score: 9.5 → 9.6 (correctness fix on hot path)
### ADR-028: Native contract support layer — codec, engine, and settings helpers

**Status**: Accepted (implemented)

**Context**: The `neo-native-contracts` crate (L3, 106 files, 28K LOC) had ~265 lines
of copy-pasted boilerplate across its 11 contracts (NEO, GAS, Policy, Oracle, Notary,
StdLib, CryptoLib, ContractManagement, LedgerContract, RoleManagement, Treasury):

1. **StackValue encode/decode** — 14 sites called
   `BinarySerializer::deserialize_stack_value_with_limits` with identical
   `ExecutionEngineLimits::default()` + error wrapping. 12 sites called
   `to_stack_value()` + `serialize_stack_value_default()`. 8 `from_stack_value`
   impls repeated the `StackValue::Struct(_, items)` destructure +
   `items.get(i)` + per-field error wrapping pattern.

2. **`persisting_block()` prelude** — 4 sites repeated
   `engine.persisting_block().ok_or_else(|| ...)` with different label strings.

3. **Hardfork-gated i64 setting readers** — 3 `get_max_*_snapshot` functions
   in PolicyContract had identical hardfork-gate + storage-read + BigInt→u32
   structure. 12 sites used BigInt→i64 conversion with bespoke error messages.

The `support/` module already existed (ADR-025 consolidated key-building there).
This ADR extends it with codec, engine, and settings helpers.

**Decision**:

Create three new modules in `neo-native-contracts/src/support/`:

1. **`codec.rs`** — `decode_stack_value(bytes, label)`,
   `encode_storage_struct<T: Interoperable>(value, label)`, and a
   `StructDecoder` helper with position-based field accessors
   (`bigint`, `u32`, `i64`, `i32`, `bool_value`, `byte_array`, `string`,
   `ec_point`, `hash160`, `hash256`, `is_null`, `len`). Each accessor
   wraps errors with `{label} {field}` for diagnostics.

2. **`engine.rs`** — `require_persisting_block(engine, contract)` returning
   `CoreResult<&PersistingBlock>` with a labelled error.

3. **`settings.rs`** —
   `read_hardfork_gated_u32_setting(snapshot, settings, default, hardfork, key, label)`
   consolidating the 3 PolicyContract snapshot readers. Promoted
   `read_optional_i64_setting_key`, `read_required_i64_setting_key`,
   `put_required_i64_setting_key` from PolicyContract so Notary and Oracle
   reuse them.

Migrate 19 production files + 8 test files to use the new helpers.

**Trade-offs**:
- **Gaining**: All copy-paste duplication eliminated. The encode/decode/setting
  pattern is defined once in 3 files instead of scattered across 11 contracts.
  New contracts follow a clear pattern via `StructDecoder`. Error messages are
  consistent (`{label} {field} ...` format). Data encoding remains
  byte-identical — verified by 377 native-contract tests passing.
- **Giving up**: ~351 lines of new helper code offset by ~273 lines of removed
  boilerplate, so net line count is approximately neutral. The abstraction
  adds one indirection layer for each encode/decode/setting call.
- **Reversibility**: High — each call site can be inlined back if needed.

**Consequences**:
- `neo-native-contracts::support::codec` is the canonical location for
  StackValue encode/decode helpers
- `neo-native-contracts::support::engine` is the canonical location for
  ApplicationEngine preludes
- `neo-native-contracts::support::settings` is the canonical location for
  hardfork-gated setting readers
- PolicyContract's i64 helpers remain as thin delegation wrappers (avoids
  touching all internal call sites)
- Error message wording changed slightly (e.g. "Notary deposit" →
  "Notary deposit is not a struct") but data encoding is byte-identical
- 377 native-contract tests pass; 3346 workspace tests pass (no regressions)
- Architecture health score: 9.5 → 9.5 (consolidation, no structural change)

### ADR-027: Dead code excision — neo-static-files, neo-engine, and dead traits

**Status**: Accepted (implemented)

**Context**: The v6 deep audit (`.planning/codebase/deep-audit-2026-07-04.md`) found
that the 26 prior ADRs codified a number of aspirational abstractions that never ran
in production. Specifically:

1. **`neo-static-files` crate (L1c)** — 613 LOC, 1 file. Workspace-wide grep for
   `use neo_static_files` returned exactly 1 hit: its own test file. The real
   cold-archive code (`StaticLedgerArchive`, `HotColdLedgerProvider`) lives in
   `neo-blockchain/src/ledger/static_archive.rs`. The crate was an orphaned
   parallel implementation with zero production callers.

2. **`neo-engine` crate (L3)** — 6 files, 839 LOC. The entire public state API
   (`Pipeline`, `CanonicalChain`, `ChainTip`, `BlockBuffer`) had zero production
   callers — only neo-engine's own tests used them. The `BlockchainEngineAdapter`
   (ADR-002/009/010 described it as "the EnginePipeline bridge") was never
   instantiated: `BlockchainEngineAdapter::new` had 0 call sites. The only real
   consumer was `NeoValidateStage` (ADR-026), which used only the `ValidateStage`
   + `PipelineStage` traits.

3. **Six dead traits** across the workspace:
   - `AsyncSystemContext` (neo-blockchain) — 0 impls, 0 callers
   - `ApplicationEngineProvider` (neo-execution) — 0 impls, also leaked concrete
     `ApplicationEngine` return type (fake seam)
   - `SignerProvider`, `AccountLike`, `MessageReceivedHandler`, `MessageLike`
     (neo-payloads) — 0 production impls, only test mock impls
   - `ConsensusMessage` (neo-consensus) — 0 impls; real wire type is
     `ConsensusPayload` struct which does not implement the trait
   - `Box<dyn ConsensusSigner>` blanket impl (neo-consensus) — 0 callers;
     everyone uses `Arc<dyn ConsensusSigner>`
   - `OracleNodeProvider` (neo-oracle-service) — 0-method marker trait with
     blanket impl; `OracleService::system: Arc<dyn OracleNodeProvider>` replaced
     with 3 separate fields (`config`, `store`, `tx`)

4. **Two traits investigated but correctly KEPT** (audit was wrong):
   - `WalletChangedHandler` — has a production implementation in
     `neo-oracle-service/src/service/handlers.rs:56` (`impl WalletChangedHandler
     for OracleService`). NOT dead.
   - `BlockLike` — has a production implementation in
     `neo-payloads/src/ledger/block.rs:180` (`impl BlockLike for Block`) and is
     used by `NeoValidateStage` via `BlockValidator::validate_block_size<B:
     BlockLike>`. NOT dead.

**Decision**:

- **Delete `neo-static-files`** entirely. Remove from workspace members,
  `[workspace.dependencies]`, Dockerfile, and layer boundary tests.
- **Delete `neo-engine`** entirely. Move the two traits actually used by
  `NeoValidateStage` (`ValidateStage` + `PipelineStage`) and their supporting
  types (`EngineError`, `EngineResult`, `StageId`, `StageContext`, `StageOutput`)
  into a new `neo-blockchain/src/pipeline/stage_traits.rs` module, next to the
  one concrete implementation that uses them. Delete the dead
  `BlockchainEngineAdapter`, the dead `Pipeline`/`CanonicalChain`/`ChainTip`/
  `BlockBuffer` state API, and the dead `ExecuteStage`/`PersistStage`/
  `CommitStage`/`EnginePipeline` traits.
- **Delete the 6 dead traits** listed above. For `OracleNodeProvider`,
  restructure `OracleService` to hold 3 separate `Arc<dyn ConfigProvider>`,
  `Arc<dyn StoreProvider>`, `Arc<dyn TxAdmission>` fields instead of the marker
  trait object.
- **Mark `BlockchainProvider` stub methods** with explicit TODOs deferring the
  finish-vs-delete decision. The trait has zero `dyn` consumers but is an
  associated type on the sealed `NodeComponents` scaffold (ADR-023).
  *(Resolved in ADR-032: deleted — zero consumers, and the two stubs silently
  returned `Ok(None)`.)*
- **Keep `WalletChangedHandler` and `BlockLike`** — the audit's "zero impls"
  claim was incorrect for both.

**Trade-offs**:
- **Gaining**: 2 crates deleted (28 → 26 workspace members). ~400 LOC of dead
  code removed. `neo-blockchain → neo-engine` dependency removed. The
  `EngineApi` (runtime) vs `EnginePipeline` (engine) two-vocabulary split that
  ADR-007 only renamed is now structurally collapsed — pipeline stage traits
  live where their implementation lives. Future contributors are no longer
  misled by ADR-002/009/010 claiming the adapter is "the bridge" when it never
  ran. `OracleService` dependency surface is now explicit (3 named fields vs 1
  opaque marker).
- **Giving up**: `neo-engine` as a separate L3 trait crate. This is correct —
  the crate shipped nothing used in production. If staged sync is ever
  prioritized (ADR-010), the pipeline driver can be added to
  `neo-blockchain::pipeline` or a new crate at that time, informed by concrete
  requirements rather than speculation.
- **Reversibility**: High — the traits are mechanical to re-extract if a
  cross-crate consumer ever appears.

**Consequences**:
- Workspace: 26 crates (down from 28)
- `neo-blockchain::pipeline::stage_traits` is the canonical location for
  `ValidateStage` + `PipelineStage` + `EngineError` + supporting types
- `NeoValidateStage` implements `crate::pipeline::stage_traits::ValidateStage`
  instead of `neo_engine::ValidateStage`
- `OracleService` constructor takes 3 provider arguments instead of 1
- Layer boundary tests updated: `neo-static-files` and `neo-engine` removed
  from L1/L3 match arms
- Dockerfile updated: `COPY neo-static-files/` line removed
- 3346 tests pass (down from 3356 — 10 dead tests removed with their dead traits)
- Architecture health score: 9.4 → 9.5 (dead code excision improves honesty)

### ADR-032: Dead type-state scaffolding excision — BlockchainProvider, NodeComponents, FullNode

**Status**: Accepted (implemented)

**Context**: ADR-021 sealed and ADR-023 documented a reth-inspired compile-time
type-state composition seam in `neo-runtime/src/node/types.rs`: the
`NodeComponents<N>` trait (associated component types), `FullNode`,
`FullNodeTypes`, the `#[allow(dead_code)]` `FullNodeComponentsExt`, and a
read-side `BlockchainProvider` trait. ADR-023 already recorded this as
"scaffolded but not functional." The post-ADR-027..031 audit confirmed it never
became functional and never will in its current shape:

- `NodeComponents` / `FullNode` / `FullNodeComponentsExt` — **zero
  implementations, zero consumers** (only re-exports and doc comments).
- `FullNodeTypes` — used only as a bound by the above; no other consumer.
- `BlockchainProvider` — **zero `dyn` consumers**; its only implementation
  (`impl BlockchainProvider for Node` in neo-system) returned `Ok(None)` from
  `get_transaction_by_hash` and `get_state_root`. That is a *silent wrong
  answer* landmine: any future `dyn BlockchainProvider` consumer would have
  read "no such transaction / no state root" instead of an error or the real
  value. The stubs carried a mis-tagged `TODO(ADR-031, Phase 3 G2)`.

This is roadmap item G2 ("decide the runtime service-trait seam: delete the dead
traits OR move to an L2 `neo-runtime-api`") and the tail of A5.

**Decision**: **Delete** the entire dead type-state seam rather than finish or
relocate it. Removed from `neo-runtime`: `BlockchainProvider`, `NodeComponents`,
`FullNode`, `FullNodeTypes`, `FullNodeComponentsExt`, plus their re-exports
(`node/mod.rs`, `lib.rs`) and now-unused imports (`async_trait`, `BlockExecutor`,
`ConsensusApi`, `EngineApi`, `NetworkService`, `ImportQueue`). Removed from
`neo-system`: the `impl BlockchainProvider for Node` block and its unused
imports. **Kept**: `NodeTypes` + `NeoNodeTypes` (live protocol-primitive types)
and the three active provider traits `StoreProvider`, `ConfigProvider`,
`TxAdmission` — these are load-bearing (the RPC session consumes
`Arc<dyn StoreProvider>`; the oracle service consumes `TxAdmission`).

**G4 (StoreConfigBundle) deliberately NOT done** — reaffirming the Phase-3 skip.
`StoreProvider`/`ConfigProvider` are implemented once each on `Node` and once on
`neo-rpc`'s `NodeContext` as trivial 1-line forwarding, and both structs must
remain the `dyn` target. A shared `StoreConfigBundle` would still require both to
`impl` the traits (Rust has no impl delegation), so it would *add* an indirection
layer without removing any forwarding. Not worth the churn.

**Trade-offs**:
- **Gaining**: ~140 LOC of never-functional scaffolding removed; the silent
  `Ok(None)` correctness trap eliminated; the architecture doc now matches
  reality — node composition is runtime `Option<Arc<dyn Trait>>` wiring, not a
  type-state graph.
- **Giving up**: the reth-style compile-time component seam. Correct call: it
  shipped nothing, and a real typed-node design should be driven by concrete
  requirements, not a speculative scaffold. ADR-021 (sealing) and ADR-023
  (type-state status) are superseded for these types.
- **Reversibility**: High — mechanical to reintroduce if a concrete `TypedNode`
  ever needs it.

**Consequences**:
- `neo-runtime::node` exports only `NodeTypes`, `NeoNodeTypes`, `StoreProvider`,
  `ConfigProvider`, `TxAdmission`.
- Resolves roadmap G2 and the A5 `BlockchainProvider` tail; the Phase-1 TODOs in
  `neo-system/src/composition/node.rs` are gone.
- No test changes — nothing consumed the deleted types.
- Architecture health score: 9.6 → 9.6 (removes a latent correctness trap; net
  honesty gain, no structural regression).

### ADR-033: Dead service-trait scaffolding excision — BlockExecutor, ConsensusApi, EngineApi

**Status**: Accepted (implemented)

**Context**: The comprehensive architecture-consistency audit found that three of
the reth-style runtime service traits in `neo-runtime/src/service/services.rs`
were the same never-wired scaffolding ADR-032 already removed once — a trait +
`Option<Arc<dyn ..>>` field + no-impl profile:

- `BlockExecutor` — zero production implementations (only a test `DummyExecutor`).
- `ConsensusApi` — zero implementations at all (the name renamed from
  `ConsensusService` in ADR-030 to break a collision; the trait itself was dead).
- `EngineApi` — zero implementations, plus a `sealed` module gating zero
  implementors.

`Node` and `NodeBuilder` carried `block_executor` / `consensus` / `engine`
`Option<Arc<dyn ..>>` fields with `with_*` setters. Verified: the setters have
**zero call sites** and the fields are **never read** — they are always `None`
in production.

**Decision**: **Delete** `BlockExecutor`, `ConsensusApi`, `EngineApi`, the
`sealed` module, the three `Node`/`NodeBuilder` fields + their `with_*` setters +
build-assignments, and the `DummyExecutor` test — applying the ADR-032 treatment.

**KEEP** (verified genuinely used, NOT dead — the audit's "half-live" grouping
was too broad):
- `Service` — the marker supertrait, still implemented by the live traits below.
- `NetworkService` — has a real production impl (`LocalNodeService`).
- `BlockImport` + `ImportQueue` — load-bearing generic bounds for
  `BlockImportQueue<I: BlockImport>` (shipped in v0.9.0 for bounded concurrent
  block preverification) and `SyncPipelineDriver<Q: ImportQueue>`. These are the
  staged-sync pipeline infrastructure; deleting them would destroy real WIP.

So the excision is a clean 3-trait removal, not 6 — the genuinely-dead seams
only.

**Trade-offs**:
- **Gaining**: ~110 LOC of never-instantiated scaffolding removed; `Node` /
  `NodeBuilder` no longer carry three always-`None` fields and three dead
  setters; the runtime service vocabulary now reflects what is actually wired
  (network + import/sync).
- **Giving up**: the reth-style executor/consensus/engine service seam. Correct:
  the node wires block execution, consensus, and the engine surface through
  concrete types; the traits shipped nothing. ADR-021's sealing of `EngineApi`
  is superseded (the trait is gone).
- **Reversibility**: High — mechanical to reintroduce behind a concrete impl if
  a real polymorphic consumer ever appears.

**Consequences**:
- `neo_runtime::service::services` exports `Service`, `NetworkService`, `TxHash`.
- The surviving `ConsensusService` name in the tree is the `neo-consensus`
  STRUCT (the real dBFT machine), not a trait — the ADR-030 collision stays
  resolved.
- `cargo check --workspace --all-targets`, the full workspace suite, and
  workspace Clippy are clean.
- Architecture health score: 9.6 → 9.6 (honesty; removes dead scaffolding).

### ADR-034: Static runtime composition and allocation-free async trait calls

**Status**: Accepted (implemented)

**Context**: The remaining runtime service locator stored optional services in
`HashMap<TypeId, Arc<dyn Any + Send + Sync>>`. Besides dynamic dispatch and
locking on every request, the erased key made storage backing mismatches look
like a disabled service: registering `StateStore<RuntimeStore>` and requesting
the default `StateStore<MemoryStore>` compiled and returned `None`. The
registry also duplicated `SyncImportPipeline`, which was already an explicit
`Node` field. Separately, the surviving generic network, import, signer, wallet,
and block-validation traits used `async_trait`; that macro returned a boxed
trait-object future for every statically dispatched call.

**Decision**:

- Delete `neo_runtime::ServiceRegistry`, all `register_service` /
  `get_service<T>` APIs, and the duplicate sync-pipeline registration.
- Keep `neo_system::Node<P, S>` limited to explicit core handles, including its
  concrete `Arc<SyncImportPipeline<...>>` field.
- Let `neo-node::NodeServiceHandles<S>` own daemon-only state, state-commit,
  indexer, application-log, token-tracker, and remote-ledger handles.
- Pass an immutable `neo_rpc::RpcServices<S>` projection into `NodeContext`.
  Every supported RPC service has a named typed field; the backing `S` is part
  of the compile-time contract.
- Keep `OracleService<NodeContext>` beside the context in `RpcServer`, not
  inside `NodeContext`, because the oracle itself owns an `Arc<NodeContext>` and
  nesting it there would create a strong-reference cycle.
- Replace `async_trait` with return-position `impl Future + Send` for genuinely
  asynchronous static traits. Make validation and consensus-witness pipeline
  stages synchronous because they perform no awaited I/O.
- Remove `?Sized` from the core import queue, sync driver, consensus signer,
  and native-contract ABI where all supported implementations are concrete.

**Trade-offs**:

- **Gaining**: storage-backing mismatches are compile errors; RPC optional
  service lookup is a branch plus `Arc` clone with no map lock or downcast;
  import/network/signer/wallet calls no longer allocate boxed futures; node
  ownership is visible from fields and function signatures.
- **Giving up**: arbitrary third-party service registration by `TypeId` and
  unsized core service implementations. The project has no production consumer
  requiring either capability. A future open-ended plugin API must define an
  explicit capability boundary instead of reintroducing an `Any` map.
- **Reversibility**: Typed fields are straightforward to extend. Reintroducing
  erased lookup is intentionally not considered a compatible extension.

**Consequences**:

- Production Rust contains no structural `dyn` service, VM, storage, native,
  signer, wallet, or pipeline boundary. Remaining `dyn Any` occurrences are
  only Rust's standard panic payload ABI at `catch_unwind` boundaries.
- Runtime-selected built-in implementations use closed enums such as
  `RuntimeStore` rather than trait objects.
- `async-trait` is absent from first-party manifests and production service
  traits. Some third-party crates still pull it transitively.
- ADR-032 statements describing runtime `Option<Arc<dyn Trait>>` wiring are
  historical and superseded by this decision.

### ADR-035: Staged core composition and private service layouts

**Status**: Accepted (implemented)

**Context**: The manifest graph enforced downward dependencies, but ownership
still leaked upward. `neo-node::composition` directly created the canonical
store cache, snapshot, mempool, header cache, ledger context,
`NodeSystemContext`, and `BlockchainService`, then manually rebuilt the same
component graph through `NodeBuilder`. The application runtime destructured a
`RunningNode` field bag to assemble startup-import, live-service, and shutdown
contexts. `neo-blockchain` and `neo-network` also publicly exposed command-loop,
pending-block, wire, and protocol source module trees even though ordinary
callers used typed handles.

**Decision**:

- Add generic `neo_system::NodeCoreBuilder<P, S, H>`, where `P` is the native
  provider, `S` is the concrete store, and `H` is the static block-commit hook.
- Construct the canonical `StoreCache`/snapshot, mempool, header cache, ledger
  context, `NodeSystemContext`, and `BlockchainService` in `neo-system`.
- Return `NodeCoreLaunch`, which separates an owned `BlockchainTask` from the
  shareable `NodeCore`. Consume `NodeCore` through `into_node(network)` so final
  composition cannot substitute a different store, provider, mempool, cache,
  or blockchain handle.
- Keep config parsing, optional daemon services, network policy, consensus/HSM
  setup, observability, and task supervision in `neo-node`.
- Make `RunningNode` private state with one `run_requested_mode` operation. It
  orders existing startup-import, live-service, and shutdown stages while
  preserving their named contexts, outcomes, and independent tests.
- Keep process cancellation and graceful shutdown in `RunningNode`; the
  reusable `neo_system::Node` does not manufacture a second cancellation token
  or expose a competing process lifecycle.
- Make blockchain service internals and network wire/protocol module layouts
  private. Re-export stable handles, services, outcomes, protocol values, and
  codecs at crate roots. Keep command enums public only where public typed
  channel constructors expose them.
- Delete the unused network capability-constructor wrapper and timeout-counter
  modules after closing their compatibility module paths.

**Trade-offs**:

- **Gaining**: one reusable owner for provider-neutral core construction;
  compile-time identity of core collaborators; a short application lifecycle;
  stable capability APIs independent of source-file layout; less dead code.
- **Giving up**: external imports through implementation paths such as
  `neo_blockchain::service::*`, `neo_network::wire::*`, and
  `neo_network::proto::*`. The project does not preserve intermediate APIs.
- **Reversibility**: New root-level capabilities can be added deliberately.
  Reopening entire module trees is intentionally not a compatibility goal.

**Consequences**:

- The active daemon path is
  `NodeCommand -> OpenNodeRuntime -> NodeRuntime -> RunningNode`.
- Architecture tests reject direct provider-neutral constructors in
  `neo-node::composition`, application-level `RunningNode` destructuring, new
  unapproved same-layer dependencies, and public node-service implementation
  module layouts.
- The manifest architecture guard covers production and build dependencies;
  test-only `dev-dependencies` are intentionally outside the runtime graph.
- All 28 layer-boundary tests, workspace unit/integration/doctests,
  `cargo check --workspace --all-targets`, and workspace Clippy pass.
- Neo N3 v3.10.1 wire bytes, execution, native contracts, state roots,
  persistence ordering, and task-failure policy are unchanged.

### ADR-036: Production-consumed finalized Ledger static archive

**Status**: Accepted (implemented)

**Context**: ADR-027 correctly deleted an earlier `neo-static-files` crate: it
had no production caller, duplicated an aspirational archive implementation,
and made architecture claims unsupported by node composition. The live system
later gained concrete prerequisites that did not exist then:

- byte-exact Ledger writers and provider traits for block, transaction, and
  conflict-stub reads;
- a generic hot/cold provider factory with clean-miss routing;
- an explicit canonical durability callback and deferred sync-batch boundary;
- startup recovery policy that distinguishes pre-canonical observer hazards
  from recoverable post-canonical services.

The remaining storage roadmap required an immutable archive, but teaching a
generic KV backend about Neo blocks would invert ownership, while putting file
framing in `neo-blockchain` would mix protocol semantics with infrastructure.

**Decision**:

- Reintroduce the name `neo-static-files` for a new protocol-blind
  infrastructure implementation with real production consumers. It stores
  opaque key/value rows in contiguous versioned height frames, compresses each
  frame with zstd, protects indexes and payloads with xxh3 checksums, reuses the
  workspace `lru` cache, and repairs incomplete tails on open. Archives begin
  at genesis and hold a standard-library kernel writer lease for the lifetime
  of the shared provider, so a second process cannot race startup repair or
  append. The lease is released automatically if the process exits.
- Keep Neo semantics in `neo-blockchain::ledger::static_archive`. It captures
  the exact persisted `Prefix_BlockHash`, `Prefix_Block`, final
  `Prefix_Transaction`, and signer-specific conflict rows from the
  post-execution snapshot. `StaticLedgerProvider` decodes those same bytes
  through the canonical Ledger codecs and implements the existing provider
  capabilities.
- Publish static rows **after** canonical MDBX/RocksDB success. The pre-commit
  hook only buffers immutable rows; one canonical-success callback appends the
  accepted batch with one sync. Canonical failure discards the buffer.
- Treat archive publication failure as recoverable canonical lag, not as a
  cross-store atomicity failure. Request a clean restart without writing the
  pre-commit poison marker. Startup truncates impossible ahead data, validates
  every retained block hash against the overlapping canonical hot prefix, and
  replays any missing durable hot suffix before local read services start.
- Enable the archive only when `[storage].static_files_dir` is configured.
  Keep hot Ledger rows authoritative and do not prune in this phase.

**Trade-offs**:

- **Gaining**: a tested immutable storage domain, exact Ledger read parity,
  batch-level file durability, clean hot/cold composition, and crash recovery
  without changing consensus bytes or state roots.
- **Giving up**: immediate disk savings. Until persistent MDBX archive-offset
  indexes and prune/recovery parity are implemented, enabling static files
  stores a compressed mirror in addition to hot Ledger rows.
- **Constraint**: The current file uses an in-memory latest-key index rebuilt
  from frame indexes at open, and reconciliation verifies the retained prefix.
  Startup work and index memory therefore scale with archive size. Segment
  rotation and persistent MDBX offset/checkpoint indexes remain required before
  enabling aggressive historical hot-row pruning at very large chain sizes.
- **Constraint**: Archive-enabled P2P sync caps deferred batches at 64 blocks;
  oversized batches from other sources use per-block durability. This bounds
  staged Ledger-row memory while preserving batched commits for normal sync.
- **Reversibility**: High. The feature is config-gated, hot data remains
  complete, and removing the archive does not alter canonical storage.

**Consequences**:

- ADR-027 remains historically correct: its unused implementation is not
  restored. ADR-036 introduces a different implementation whose consumers are
  `neo-blockchain` and `neo-node`.
- `neo-static-files` is an infrastructure workspace member;
  `neo-blockchain` is the only Neo Ledger semantic adapter; `neo-node` owns
  configuration, startup reconciliation, commit-hook buffering, and shutdown
  policy.
- Trusted empty-block bulk fast-forward cannot skip commit hooks while the
  archive is enabled; the committing fast path remains available and preserves
  per-height Ledger history.
