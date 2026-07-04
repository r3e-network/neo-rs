# Architecture & Consistency Audit v2

**Date**: 2026-07-04  
**Scope**: All 29 workspace crates (27 neo-rs crates + tests + benches-package)  
**Method**: Automated dependency graph analysis + manual API surface review + functional boundary comparison

---

## Executive Summary

The neo-rs workspace is architecturally sound with a clear 7-layer hierarchy, no circular dependencies, and complementary crate responsibilities. However, **2 layer violations** and **several consistency gaps** were identified that should be addressed to achieve full architectural professionalism.

**Health Score**: 8.2/10  
- Layer structure: 9/10 (2 violations found)  
- Error handling: 7/10 (inconsistent CoreError adoption)  
- Dependency management: 8/10 (neo-rpc over-coupled, neo-gui non-compliant)  
- Functional boundaries: 9/10 (no duplication, neo-engine unused)  
- Naming/organization: 9/10 (consistent patterns, minor issues)  

---

## 1. Layer Hierarchy Verification

### Confirmed Layer Structure (28 crates + 2 dev-only)

```
L0  Foundation         neo-primitives
L1a Core Infra         neo-io, neo-error
L1b Stateful Infra     neo-crypto, neo-storage, neo-vm, neo-serialization, neo-manifest
L1c Cross-cutting      neo-config, neo-static-files
L2  Protocol           neo-payloads, neo-consensus, neo-hsm
L3  Domain Services    neo-runtime, neo-execution, neo-native-contracts, neo-state-service,
                        neo-mempool, neo-engine
L4  Node Services      neo-blockchain, neo-network, neo-wallets, neo-indexer, neo-tee
L5  Composition        neo-system
L6  Plugin/RPC         neo-rpc, neo-oracle-service
L7  Application        neo-node (binary), neo-gui (binary, pure HTTP client)
    Dev-only           tests, benches-package
```

### Layer Direction Check: PASS (no circular dependencies)

All dependency edges point downward (higher layer → lower layer), with **two exceptions**:

| # | Violation | Severity | Details |
|---|-----------|----------|---------|
| V1 | `neo-oracle-service` (L6) → `neo-system` (L5) | **HIGH** | Uses `neo_system::Node` directly in `service/mod.rs:33` and `lifecycle/state.rs:7` |
| V2 | `neo-rpc` (L6) → `neo-system` (L5) | **MEDIUM** | `neo-system` is a REQUIRED (non-optional) dependency. Task #143 created `NodeContext` to decouple, but `From<&neo_system::Node>` impl at `node_context.rs:188` and test code at `tests/server/support/test_support.rs:25` still depend on it. |

**Recommendation V1**: neo-oracle-service should depend on `neo-runtime` service traits (`BlockchainHandle`, `NetworkHandle`) instead of the concrete `Node`. The service receives handles, not the entire composition root.

**Recommendation V2**: Complete Task #143 by:
1. Making `neo-system` an optional dependency of `neo-rpc` (only needed for `From<Node>` conversion)
2. Moving the `From<&neo_system::Node> for NodeContext` impl to `neo-node` (the binary)
3. Updating neo-rpc tests to construct `NodeContext` directly

---

## 2. Functional Boundary Analysis

### 2.1 No Functional Duplication Found

| Pair | Status | Notes |
|------|--------|-------|
| neo-engine vs neo-blockchain | ✅ No overlap | neo-engine is a pure abstraction scaffold (traits + in-memory state). neo-blockchain has the real implementation. **However, neo-engine is currently unused** — no crate depends on it. |
| neo-storage vs neo-static-files | ✅ Complementary | neo-storage: hot stateful (RocksDB/MDBX). neo-static-files: cold append-only. Zero cross-dependencies. |
| neo-state-service vs neo-storage | ✅ Complementary | state-service: MPT state root tracking. storage: generic key-value. state-service depends on storage, not vice versa. |
| neo-runtime vs neo-system | ✅ Complementary | runtime: service traits & handles. system: composition root wiring. |
| neo-rpc vs neo-oracle-service | ✅ Complementary | RPC: JSON-RPC endpoint. oracle-service: oracle fulfillment logic. RPC calls into oracle-service for oracle RPC methods. |

### 2.2 neo-engine: Unused Scaffold (MEDIUM)

**Finding**: `neo-engine` was created in Task #140 as a block processing pipeline abstraction. It compiles, has tests, and has clean clippy. However:
- **No crate depends on it** (zero consumers in the workspace)
- `neo-blockchain::BlockchainService` does NOT use `neo-engine` traits
- The crate's own documentation states: "Future refactoring should extract the stage implementations..."

**Options**:
1. **Integrate**: Refactor `neo-blockchain::BlockchainService` to use `neo-engine` traits (significant effort)
2. **Remove**: Delete the crate and the workspace entry (clean up)
3. **Keep**: Document it as planned future work (current state — acceptable but adds build time)

**Recommendation**: Keep for now with a clear `# TODO: Integrate into neo-blockchain` marker. The abstraction is sound; the integration is a future task.

### 2.3 neo-wallets → neo-execution: Legitimate (LOW)

`neo-wallets` depends on `neo-execution` for `ApplicationEngine` to read NEP-17 asset metadata (name, symbol, decimals) via read-only contract calls. This is a legitimate dependency — wallets need to execute contract reads. Could be abstracted behind a `ContractReader` trait in `neo-runtime`, but low priority.

---

## 3. Error Handling Consistency

### 3.1 Current Pattern (Mixed Strategy)

The workspace uses a **dual error strategy**:

| Strategy | Crates | Pattern |
|----------|--------|---------|
| CoreError only | neo-serialization, neo-manifest, neo-payloads, neo-execution, neo-native-contracts, neo-mempool, neo-state-service, neo-blockchain, neo-rpc, neo-oracle-service | All functions return `CoreResult<T>` or `neo_error::Result<T>` |
| Own error only | neo-config, neo-static-files, neo-consensus, neo-runtime, neo-network | Functions return `CrateResult<T>` (e.g., `ConfigResult`, `ConsensusResult`) |
| Mixed | neo-crypto, neo-storage, neo-vm, neo-wallets, neo-indexer, neo-tee, neo-system, neo-engine | Some functions use `CoreResult`, some use `CrateResult` |

### 3.2 Missing `From<CrateError> for CoreError` Impl (MEDIUM)

Crates with their own error types that **lack** `From<CrateError> for CoreError` conversions, forcing consumers to manually convert:

| Crate | Error Type | Has `From` impl? | Impact |
|-------|------------|-----------------|--------|
| neo-crypto | CryptoError | ❌ No | Consumers must `.map_err()` manually |
| neo-config | ConfigError | ❌ No | Same |
| neo-consensus | ConsensusError | ❌ No | Same |
| neo-runtime | ServiceError | ❌ No | Same |
| neo-network | NetworkError, WireError, P2PError | ❌ No | Same |
| neo-static-files | StaticFileError | ❌ No | Same |
| neo-hsm | HsmError | ❌ No | Same |
| neo-storage | StorageError | ✅ Yes | Good |
| neo-vm | VmError | ✅ Yes | Good |

**Recommendation**: Add `From<CrateError> for CoreError` impls to all crates that define their own error types. The `impl_error_from!` macro from `neo-error` makes this trivial:

```rust
impl_error_from! {
    CryptoError => CoreError::Cryptographic
}
```

### 3.3 neo-rpc Duplicate RpcError Types (LOW)

`neo-rpc` defines **two different types named `RpcError`**:
1. `errors/error.rs:7` — `pub enum RpcError` (general RPC operations error)
2. `server/rpc_error/mod.rs:26` — `pub struct RpcError` (JSON-RPC server error response)

The server type is re-exported as `ServerRpcError` (`server/mod.rs:93`), but having two types with the same name in one crate is confusing.

**Recommendation**: Rename the server struct to `RpcServerError` or `JsonRpcError` for clarity.

### 3.4 neo-serialization Mixed Error Strategy (LOW)

`neo-serialization` uses `CoreError` in its codec module (`codec/serialization.rs`) but defines its own `JsonError` in `json/error.rs`. This is inconsistent within a single crate.

**Recommendation**: Either adopt `CoreError` throughout or provide `From<JsonError> for CoreError`.

---

## 4. Dependency Management

### 4.1 neo-rpc Over-Coupling (HIGH)

`neo-rpc` has **14 required (non-optional) internal dependencies** including `neo-system`, `neo-blockchain`, `neo-network`, `neo-wallets`, `neo-mempool`, `neo-state-service`, `neo-native-contracts`. This means even building the `client` feature alone pulls in the entire node stack.

**Impact**: Build time tax for any consumer that only needs the RPC client. The `client` feature should only need: `neo-primitives`, `neo-io`, `neo-crypto`, `neo-error`, and optionally `neo-payloads` for type definitions.

**Recommendation**: Make all server-specific dependencies optional. The `client` feature should be lightweight:
```toml
[features]
client = ["dep:neo-primitives", "dep:neo-io", "dep:neo-crypto", "dep:neo-error", "dep:neo-payloads"]
server = ["client", "dep:neo-system", "dep:neo-blockchain", ...]
```

### 4.2 neo-gui Non-Compliant Workspace Deps (LOW)

`neo-gui` uses **zero** `workspace = true` references. All dependencies are direct version pins (`eframe = "0.29"`, `anyhow = "1"`, `serde = "1"`, etc.). This is inconsistent with the rest of the workspace.

**Recommendation**: Add shared deps (serde, serde_json, anyhow, tracing, tracing-subscriber, reqwest) to workspace.dependencies and use `workspace = true` in neo-gui.

### 4.3 External Path Dependency: neo-vm-rs (INFO)

`neo-vm-rs = { path = "../neo-vm-rs" }` points outside the workspace to a sibling repository. This is a deliberate design decision (separate VM semantics crate), but it means:
- The workspace doesn't fully own its VM layer
- CI must clone both repos
- Version sync requires manual coordination

**Status**: Acceptable as-is. Document in README that `neo-vm-rs` is a sibling repo.

### 4.4 Workspace Dependency Coverage: GOOD

All other crates use `workspace = true` consistently for shared dependencies. The previous session's consolidation (Task #146) successfully eliminated all non-workspace deps except neo-gui.

---

## 5. Module Organization & Naming

### 5.1 Module Style: CONSISTENT

All crates use `mod.rs` style consistently. No crate mixes `mod.rs` and file-style modules.

### 5.2 Module-Level Documentation: CONSISTENT

All 27 neo-* crates have `//!` doc comments in their `lib.rs`. Verified in Task #145.

### 5.3 Error Result Type Naming: CONSISTENT

All crates that define their own error types follow the pattern:
```rust
pub type CrateResult<T> = Result<T, CrateError>;
```

Examples: `CryptoResult`, `ConfigResult`, `StorageResult`, `VmResult`, `ConsensusResult`, `ServiceResult`, `EngineResult`, `NetworkResult`, `WalletResult`, `IndexerResult`, `TeeResult`, `NodeResult`, `RpcResult`.

### 5.4 Empty Feature Sections (LOW)

Three crates have `[features]` sections with only `default = []` and no actual features:
- `neo-payloads`
- `neo-blockchain`
- `neo-consensus`

These are unnecessary boilerplate. Cargo treats missing `[features]` as `default = []`.

**Recommendation**: Remove empty `[features]` sections, or add a comment explaining they're placeholders for future features.

### 5.5 Feature Flag Strategy: GOOD

Meaningful feature flags are well-designed:
- `neo-crypto`: `bls-experimental` (gated BLS12-381)
- `neo-network`: `upnp` (optional UPnP)
- `neo-rpc`: `server` / `client` (modular RPC)
- `neo-oracle-service`: `oracle` / `neofs-grpc` (transport options)
- `neo-hsm`: `pkcs11` / `azure` / `gcp` (HSM backends)
- `neo-tee`: `simulation` / `sgx-hw` / `attestation` / `nitro` (TEE backends)
- `neo-node`: `tee` / `tee-sgx` / `hsm` (binary toggles)

---

## 6. Build & Test Status

| Check | Status |
|-------|--------|
| `cargo check --workspace` | ✅ Clean (0.92s) |
| `cargo clippy -p neo-engine` | ✅ Zero warnings |
| `cargo test --workspace` | ✅ All pass (0 failures) |
| Architecture boundary tests (20 tests) | ✅ All pass |
| Compiler warnings | ✅ None |

---

## 7. Prioritized Action Items

| # | Issue | Severity | Effort | Recommendation |
|---|-------|----------|--------|----------------|
| A1 | neo-oracle-service → neo-system layer violation | HIGH | Medium | Replace `neo_system::Node` with `neo-runtime` trait handles |
| A2 | neo-rpc → neo-system required dependency | HIGH | Medium | Make `neo-system` optional, move `From<Node>` to neo-node |
| A3 | neo-rpc over-coupled required deps (14 internal) | MEDIUM | Large | Make server-specific deps optional; `client` should be lightweight |
| A4 | Missing `From<CrateError> for CoreError` impls (7 crates) | MEDIUM | Small | Add `impl_error_from!` to crypto, config, consensus, runtime, network, static-files, hsm |
| A5 | neo-engine unused scaffold | MEDIUM | N/A | Keep with TODO marker; integrate in future refactoring |
| A6 | neo-rpc duplicate RpcError types | LOW | Small | Rename server struct to `JsonRpcError` |
| A7 | neo-gui non-compliant workspace deps | LOW | Small | Add shared deps to workspace.dependencies |
| A8 | neo-serialization mixed error strategy | LOW | Small | Adopt CoreError or add From<JsonError> |
| A9 | Empty [features] sections (3 crates) | LOW | Trivial | Remove or document as placeholders |
| A10 | Document error handling convention | LOW | Trivial | Add to CONTRIBUTING.md: "Library crates use thiserror for domain errors; provide From<CrateError> for CoreError; use CoreResult for cross-crate APIs" |

---

## 8. Positive Findings

- **No circular dependencies** — dependency graph is a clean DAG
- **No functional duplication** — every crate has a distinct, complementary role
- **Layer hierarchy is sound** — only 2 violations out of 100+ dependency edges
- **Module organization is consistent** — uniform mod.rs style, all crates documented
- **Error result type naming is uniform** — `{Crate}Result<T> = Result<T, {Crate}Error>`
- **Feature flags are well-designed** — meaningful optional features, not gratuitous
- **Build is clean** — zero warnings, all tests pass
- **neo-gui is properly decoupled** — pure HTTP client, no internal crate linking
- **neo-storage vs neo-static-files split is correct** — hot vs cold, no overlap
- **Workspace dependency consolidation is nearly complete** — only neo-gui remains

---

## 9. Comparison with Reference Architectures

| Aspect | neo-rs | reth | Polkadot SDK | Assessment |
|--------|--------|------|-------------|------------|
| Layer hierarchy | 7 layers | 6 layers | 5 layers | ✅ Comparable depth |
| Error strategy | Dual (CoreError + thiserror) | Single (eyre/anyhow) | Dual (sp-runtime::Error + thiserror) | ✅ Matches Polkadot pattern |
| Pipeline abstraction | neo-engine (unused) | reth-stages (integrated) | cumulus/polkadot (integrated) | ⚠️ Abstraction exists but unused |
| RPC decoupling | Partial (NodeContext created) | Full (traits only) | Full (trait-based) | ⚠️ Needs completion |
| Feature flags | Meaningful per-crate | Minimal | Extensive per-crate | ✅ Good balance |
| Workspace deps | Nearly complete | Full | Full | ⚠<arg_value> neo-gui exception |
