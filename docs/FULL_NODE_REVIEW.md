# neo-rs Full Node Review (Module-by-Module)

**Date:** 2025-12-17  
**Workspace root:** `/home/neo/git/neo-rs`  
**Note:** This file is a living review; keep it in sync with `neo-node` wiring.

This document is a *technical review* of the current codebase layout, crate responsibilities, and
end-to-end “full node” completeness from the perspective of running `neo-node` + querying it with `neo-cli`.

## What Was Verified

- `cargo test --workspace`: **PASS**
- `cargo fmt --all --check`: **PASS**
- `cargo clippy --workspace --all-targets`: **PASS with warnings**
  - `neo-core`: `clippy::type_complexity` in monitoring callbacks
  - `neo-mempool`: `clippy::too_many_arguments` in `TransactionEntry::new`
  - `neo-node`: numerous `dead_code` warnings (unused runtime fields / WIP paths)
  - `neo-vm`: `clippy::module_inception` in test module layout

## Executive Assessment

### Strengths

- The workspace is cleanly split into crates that map to Neo’s domains (primitives/crypto/vm/core/etc.).
- Foundation crates (`neo-primitives`, `neo-io`, `neo-json`, `neo-storage`, `neo-crypto`) compile cleanly and are reusable.
- `neo-consensus` now contains a real dBFT implementation (not just enums/types).

### Current Status (Full Node Wiring)

`neo-node` now wires the daemon to the parity runtime and full RPC server:

- **Runtime:** `neo_core::neo_system::NeoSystem` (actor-based, feature `neo-core/runtime`)
- **P2P:** started via `NeoSystem::start_node(...)`
- **RPC:** `neo_rpc::server::RpcServer` (feature `neo-rpc/server`) with the standard Neo handler set

The earlier “refactor stub” node runtime (`neo-node/src/runtime/*`, `neo-node/src/p2p_service.rs`,
`neo-node/src/rpc_service.rs`, etc.) has been removed to avoid maintaining two competing stacks.

These gaps do not necessarily mean the *protocol code* is incorrect; they mean the “node daemon”
composition is still mid-refactor and does not yet present a full-node contract end-to-end.

## Crate-by-Crate Review

### Foundation Layer

#### `neo-primitives`
- **Role:** Fundamental types (e.g., `UInt160`, `UInt256`, verification/result enums).
- **Notes:** Looks cohesive and dependency-light; this is the right “bottom” of the graph.

#### `neo-io`
- **Role:** Serialization (`BinaryReader`/`BinaryWriter`), helpers, cache utilities.
- **Notes:** Good separation; keep it free of protocol/business logic.

#### `neo-json`
- **Role:** C#-style JSON token model (`JToken`, `JObject`, etc.).
- **Notes:** Self-contained and useful for RPC models and tooling.

#### `neo-storage`
- **Role:** Storage traits + minimal types for backends.
- **Notes:** Correctly avoids depending on `neo-core`. The project still has a parallel persistence API in `neo-core::persistence`; that split is acceptable as long as conversions are explicit and intentional.

#### `neo-crypto`
- **Role:** Hashing, ECC, MPT trie support, Bloom filters.
- **Notes:** `NeoHash` is a convenience wrapper over `Crypto`; the layering is fine and avoids duplicated implementations.

### Core Layer

#### `neo-vm`
- **Role:** NeoVM implementation (execution engine, stack items, opcodes/jump table).
- **Notes:** Large but well-contained. Clippy warning is test-only organization.

#### `neo-core`
- **Role:** C# parity “core” (ledger types, ApplicationEngine, native contracts, persistence implementations, NeoSystem, internal telemetry, P2P protocol types/framing).
- **Architecture note:** This crate remains a large aggregation point. That can be OK for parity, but it makes “new runtime” integration harder unless stable trait boundaries are defined and used by `neo-node`.

#### `neo-p2p`
- **Role:** Lightweight P2P protocol enums/types + service traits for external consumers.
- **Notes:** Good “dependency-light” crate; actual network service orchestration lives in `neo-node` right now.

#### `neo-consensus`
- **Role:** dBFT service + message types/state context.
- **Notes:** Solid direction. The missing piece is runtime integration (wallet, P2P bridging, transaction sourcing).

### Service Layer

#### `neo-config`
- **Role:** Parse/validate node and protocol configuration (including genesis data).
- **Notes:** Important for correctness; ensure changes here are covered by config validation tests and strict parsing (unknown keys should fail).

#### `neo-telemetry`
- **Role:** Production observability primitives (logging/metrics/health helpers).
- **Notes:** `neo-node` currently has its own `health.rs` + `metrics.rs` and also uses `neo-core::telemetry`; long-term pick a single “observability stack” boundary to reduce duplication.

#### `neo-state`
- **Role:** World state abstraction + MPT-based state trie manager.
- **Notes:** Currently not fully wired into `neo-node` block execution (state is allocated but unused). This is one of the largest “completeness blockers”.

#### `neo-mempool`
- **Role:** Lightweight transaction pool.
- **Notes:** API is clean; consider a builder/struct for `TransactionEntry::new` to avoid the “too many args” smell.

#### `neo-chain`
- **Role:** Standalone chain state machine (block index, fork choice, validation hooks).
- **Notes:** Right now it is in-memory state/index management. A full node needs persistence + a validation pipeline backed by real state.

### Application Layer

#### `neo-node`
- **Role:** Node daemon that hosts P2P + RPC on top of `NeoSystem`.
- **Status:** Uses `neo-rpc` server handlers; health/metrics are pumped from the live runtime.

#### `neo-cli`
- **Role:** JSON-RPC client CLI for a Neo node.
- **Notes:** Correctly delegates to `neo-rpc` client; will be useful once `neo-node` hosts the full RPC surface.

#### `neo-rpc`
- **Role:** Full JSON-RPC server + client (feature-gated).
- **Notes:** This is the likely long-term RPC implementation, but it is not currently used by `neo-node`.

#### `neo-tee`
- **Role:** Optional TEE primitives and SGX feature gating.
- **Notes:** Workspace member; integrated into `neo-node` behind `--features tee` / `tee-sgx`.

#### `neo-tests` (`tests/`)
- **Role:** Cross-crate integration tests (currently focused on layer boundaries).
- **Notes:** Useful pattern for preventing layering regressions.

## Top Recommendations (Architecture & Completeness)

1. **Choose the “node composition” path**
   - Either (A) wire `neo-node` to use `neo-core`/`neo-rpc`/`NeoSystemContext` as the authoritative runtime, or
   - (B) finish the new modular runtime (`neo-chain` + `neo-state` + `neo-mempool` + `neo-consensus`) and adapt `neo-rpc` server to it.
   - Today the repo contains significant pieces of both approaches, and the integration seams are the main source of incompleteness.

2. **Keep the wiring single-path**
   - Avoid reintroducing the removed stub services; keep `neo-node` as composition + configuration around `NeoSystem` and `neo-rpc`.

3. **Back block execution with real state**
   - Execute genesis (or otherwise initialize native contract storage) and ensure every block executes against the previous block’s committed snapshot.
   - Persist state changes (storage) and expose correct block/transaction queries for RPC.

4. **Consensus scope**
   - Consensus (dBFT validator mode) can remain a separate “service layer” concern; keep it optional and well-isolated from the sync/validation path.

## Documentation Notes

- `README.md` references `docs/METRICS.md`; that file now exists and documents the health/metrics endpoints.
- Several older docs refer to a removed `neo-plugins` crate; treat them as historical or update them as the refactor stabilizes.
