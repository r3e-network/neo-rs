# Design — Comprehensive Refactoring & Protocol Completion

> Historical change design. Later architecture work removed `ServiceRegistry`
> and replaced it with explicit `NodeServiceHandles<S>` / `RpcServices<S>`
> composition (ADR-034).

## Overview

This change document captures the structural debt in neo-rs and proposes a phased approach to resolution. It is the **successor** to:

- `2026-06-12-style-consistency-and-crate-consolidation/` (mostly done; absorbed lint directives, macro dedup, dep style uniformity, error convention, merged `neo-script-builder`/`neo-application-logs`).
- `2026-06-12-deep-refactoring/` (in progress; refactored RPC models, error handling, etc.).

This change goes **deeper**: it deletes entirely dead crates (not just stale constants), replaces custom implementations with mature 3rd-party libraries (most importantly the abandoned `warp` framework with `jsonrpsee`), and consolidates duplicate logic that exists at the algorithm level (not just the import level).

## Architectural decisions

### 1. Why delete `neo-telemetry` entirely (not refactor)
`grep -r "use neo_telemetry\|use ::neo_telemetry\|neo_telemetry::" --include="*.rs" neo-*/src` returns only:
- `neo-telemetry/src/health.rs` (self-reference)
- `neo-telemetry/src/lib.rs` (self-reference)
- `tests/tests/layer_boundary_tests.rs:32,46` (cargo dependency classification, no actual import)

**Decision: delete the crate.** It would be wrong to "refactor" a 1,980-LoC crate with zero production consumers — that's not refactoring, that's preserving dead code. The `tracing` + `tracing-subscriber` ecosystem that `neo-tokens-tracker` already uses directly (`/Users/jinghuiliao/git/r3e/neo-rs/neo-tokens-tracker/Cargo.toml:25`) is the canonical observability stack in 2026; the prometheus + sysinfo + hyper stack in `neo-telemetry` is from a 2022-era design.

### 2. Why keep `neo-tee` and `neo-hsm` (not delete)
`neo-tee` and `neo-hsm` are **future-feature crates** awaiting wallet/transaction-signing integration. They have:
- Production-quality code (SGX sealing, PKCS#11, BIP-32 derivation).
- Simulation modes that work standalone.
- Test coverage (54 lib tests in `neo-tee`).
- No production consumers.

**Decision: keep, mark explicitly as not production-ready.** The `node-readiness-audit-2026-06-11.md` and `neo-core-dissolution-validated-dag.md` both plan to wire them up in a future "secure-wallet" feature. Deleting them now would discard working code that's slated for integration.

### 3. Why migrate `warp` → `jsonrpsee` (not keep both)
The `neo-rpc` crate declares **both** `warp = "0.3"` (line 53) and `jsonrpsee = "0.24.10"` (line 120 of `Cargo.toml`):
- `warp` was last released in **2020**; it has open security advisories and is effectively abandoned.
- `jsonrpsee` is the modern, mature JSON-RPC framework used by Polkadot, Ankr, and other production blockchain nodes.
- The `neo-rpc/src/server/jsonrpsee_adapter.rs` (116 LoC) already provides a working jsonrpsee adapter wrapping the existing handlers.
- The 611-LoC `neo-rpc/src/server/rpc_server.rs` still uses `warp::Filter` + manual hyper plumbing.

**Decision: remove warp entirely, promote jsonrpsee.** Two parallel JSON-RPC frameworks is exactly the kind of "different projects stitched together" inconsistency the user asked us to eliminate.

### 4. Why NOT merge `neo-error` into `neo-primitives` (despite fan-in)
The user asked: "in the meanwhile check if we can refactor this project, reduce crates numbers, merge similar functional crates yet both are small and can be merged."

`neo-error` (623 LoC) has **fan-in 20** (15 direct dependents + transitive). Merging it into `neo-primitives` would:
- Invert the layer ordering: `neo-primitives` is the L0 leaf with **zero** `neo-*` deps; adding `neo-error` as a sibling would make the two crates co-equal at L0, but their current relationship is `neo-error → neo-primitives`.
- Force every L0 consumer (currently `neo-primitives`'s 15+ direct users) to take a transitive `neo-primitives → neo-error → neo-primitives` cycle.

**Decision: keep `neo-error` as a separate crate.** This matches the `polkadot-sdk` and `reth` precedents (both have dedicated `errors` crates). The `crate-boundary-audit-2026-06-08.md:109-119` already plans to drop the 5 cross-crate `From` impls to trim its deps to `neo-primitives + neo-io` only.

### 5. Why NOT merge `neo-runtime` into `neo-system`
`neo-runtime` and `neo-system` have a clean 1-way edge: `neo-system → neo-runtime`. They serve different purposes:
- `neo-runtime` owns shared static service contracts (`Service`, `NetworkService`, `BlockImport`, `ImportQueue`, and runtime events).
- `neo-system` is the core composition root (`Node` + `NodeBuilder` + `WalletProvider` + explicit typed handles). Optional daemon/RPC services are composed above it through `NodeServiceHandles<S>` and `RpcServices<S>`.

This is the **Reth-style split** (`reth-engine` traits vs. `reth-node-builder`). Merging them would re-introduce the layering inversion the recent `2026-06-08-reth-style-service-architecture` change spent 5 stages eliminating.

**Decision: keep separate.** Apply the planned deletion of duplicate `BlockchainHandle`/`BlockchainCommand` (86 LoC) from `neo-runtime/src/blockchain.rs`.

### 6. Why merge `neo-tokens-tracker` → `neo-rpc::plugins::tokens_tracker`
- `neo-tokens-tracker` (2,243 LoC, 15 internal deps) is only consumed by `neo-rpc` (and feature-gated by `neo-node`).
- `neo-rpc` already has `src/server/rpc_server_tokens_tracker/` (660+ LoC) that imports from `neo-tokens-tracker`. This is the C# `RpcServer.TokensTracker` integration.
- The 15 internal deps in `neo-tokens-tracker/Cargo.toml` are mostly redundant once inside `neo-rpc` (which already depends on most of them).

**Decision: merge.** Updates required: 6 import sites in `neo-rpc`, 1 test file. Net: -1 workspace member, ~13 redundant internal deps removed.

### 7. Why extract `neo-rpc-types` (not split `neo-rpc` outright)
The `neo-rpc` crate (40,170 LoC, 133 files) is monolithic. The 5,921-LoC `client/models/` directory contains 37 pure DTO structs that are used by both `client/` and `server/`. The dissolution plan calls for a separate `neo-rpc-types` leaf crate.

**Decision: extract `neo-rpc-types` first** (~7,000 LoC). This is a pre-requisite for any future `neo-rpc-server`/`neo-rpc-client` split. Doing both at once would explode the scope.

## Testing strategy

For each phase, the verification command is `cargo test --workspace --lib --no-fail-fast`. The current passing counts (per `PROTOCOL_VERIFICATION_REPORT.md` 2026-06-12):

- `neo-vm`: 85
- `neo-consensus`: 116
- `neo-p2p`: 27
- `neo-payloads`: 61
- `neo-blockchain`: 62
- `neo-mempool`: 24
- `neo-state-service`: 31
- `neo-rpc`: 6 lib
- `neo-network`: 34
- `neo-native-contracts`: 221 lib + 16 integration
- `neo-wallets`: 9
- `neo-crypto`: 141
- `neo-primitives`: 222
- `neo-storage`: 124
- `neo-system`: 16
- `neo-oracle-service`: 7
- `neo-tokens-tracker`: 4
- `neo-tee`: 54

**Target after this change:** same or higher counts (no test removal; only new tests for new shared helpers).

## Risk assessment

| Phase | Risk | Mitigation |
|---|---|---|
| A (dead code) | Very low (zero consumers) | Pure deletion; cargo check after each step |
| B (native contract dedup) | Low (mechanical refactor) | One contract at a time; preserve 221 passing tests |
| C1 (warp → jsonrpsee) | Medium (touches RPC server) | Keep `jsonrpsee_adapter.rs` as the migration target; smoke test each endpoint |
| C3 (BigDecimal) | Medium (subtle numeric semantics) | Keep public API unchanged; comprehensive numeric tests in `neo-primitives` |
| C4 (Result<_, String> → thiserror) | Low (compile-time) | One module at a time |
| D (rpc_method! macro) | Medium (handler signature changes) | Macro provides identical semantics; smoke tests |
| E1 (key_builder!) | Low | Mechanical replacement |
| E3 (neo-tokens-tracker merge) | Low (only 6 import sites) | After E4 (rpc-types extraction) |
| E4 (neo-rpc-types) | Medium (5,921 LoC moved) | Verify each model after move |
| E5 (feature-gating) | Low | Tests run with `--all-features` |

## Sequencing rationale

The phases must be applied roughly in order because:
- **A** must come first (removes dead code so B/C/D/E don't import dead crates).
- **B** is independent but reduces the surface for **C** (less `Result<_, String>` in `neo-native-contracts`).
- **C** is the highest-impact refactor; do it early to lock in the new third-party dependencies.
- **D** depends on **C1** (jsonrpsee migration) being done; otherwise the macro-generated handlers run through the wrong transport.
- **E** is mostly independent; can run in parallel with **D**.

Within each phase, the tasks are ordered to minimize disruption:
- Helper extractions before call-site replacements (B1 hoists, B1.3 replaces).
- Macro introduction before macro usage (D1 implements, D1.3 uses).
- Crate extraction before crate deletion (E4 extracts `neo-rpc-types`, E3 merges `neo-tokens-tracker`).

## Out of scope (deferred to follow-up)

These items are documented but explicitly NOT included in this change:

- **Phase F** (protocol completeness): 6–10 weeks of work to reach 100% C# wire/protocol parity. Warrants a dedicated proposal.
- **Splitting `neo-rpc` into `neo-rpc-server`/`neo-rpc-client`**: dependent on E4 (`neo-rpc-types` extraction).
- **Splitting `neo-native-contracts` per-contract** (`neo-native-oracle`, `neo-native-notary`): feature-gating in E5 is the lighter-touch alternative.
- **Wiring `neo-tee`/`neo-hsm` to production code paths**: requires a wallet/transaction-signing integration PR.
- **Removing `neo-vm-rs` sibling dependency**: out of scope (sibling-crate migration is a multi-week effort).
- **Adding missing documentation**: separate effort.

## References

- `/Users/jinghuiliao/git/r3e/neo-rs/PROTOCOL_VERIFICATION_REPORT.md`
- `/Users/jinghuiliao/git/r3e/neo-rs/ARCHITECTURE.md`
- `/Users/jinghuiliao/git/r3e/neo-rs/CONVENTIONS.md`
- `/Users/jinghuiliao/git/r3e/neo-rs/claudedocs/crate-boundary-audit-2026-06-08.md`
- `/Users/jinghuiliao/git/r3e/neo-rs/claudedocs/node-readiness-audit-2026-06-11.md`
- `/Users/jinghuiliao/git/r3e/neo-rs/claudedocs/neo-core-dissolution-validated-dag.md`
- `/Users/jinghuiliao/git/r3e/neo-rs/openspec/changes/2026-06-12-style-consistency-and-crate-consolidation/`
- `/Users/jinghuiliao/git/r3e/neo-rs/openspec/changes/2026-06-12-deep-refactoring/`
- https://github.com/neo-project/neo-vm/blob/master/src/Neo.VM/OpCode.cs — C# OpCode reference
- https://docs.neo.org/docs/n3/develop/rpc/api.html — Neo N3 RPC API reference (browser-only SPA)
