# neo-rs Project Memory

## Architecture
- Workspace uses 28 crates in layered order: L0 `neo-primitives`; L1 infra (`neo-io`, `neo-error`, `neo-crypto`, `neo-storage`, `neo-vm`, `neo-serialization`, `neo-manifest`, `neo-config`, `neo-static-files`); L2 protocol (`neo-payloads`, `neo-consensus`, `neo-hsm`); L3 domain services (`neo-runtime`, `neo-execution`, `neo-native-contracts`, `neo-state-service`, `neo-mempool`, `neo-engine`); L4 node services (`neo-blockchain`, `neo-network`, `neo-wallets`, `neo-indexer`, `neo-tee`); L5 `neo-system`; L6 `neo-rpc`, `neo-oracle-service`; L7 `neo-node`, `neo-gui`.
- Library crates use `neo_error::CoreError`/`CoreResult<T>` or crate-specific `thiserror`; app crates (`neo-node`, `neo-gui`) use `anyhow::Result`.
- RPC and oracle service are decoupled from `neo_system::Node` via runtime provider traits; `neo-system` is dev-only for those crates.
- `neo-engine` is integrated through `BlockchainEngineAdapter`; staged pipeline unification remains incremental.
- `neo-rpc` is outside default members; client/server internals are feature-gated.
- Composition root (`neo-node`) constructs `NodeContext` via `from_parts()` to avoid layer violations.

## ADR / Design State
- `design.md` is the source of truth: 26 ADRs, reth/Polkadot comparison, 4-phase roadmap, current health score 9.4/10.
- Audit reports: `.workbuddy/verification/architecture-audit.md`, `consistency-audit.md`, `infrastructure-layer-split.md`.
- ADR-010: pipeline unification is phased; ValidateStage extraction is next low-risk phase.
- ADR-011: domain-specific errors use crate errors; validation/codec use CoreError. Domain→CoreError mappings must preserve semantics.
- ADR-016: HSM redeem scripts delegate to `neo_vm::RedeemScript::signature_redeem_script()`.
- ADR-020: Store god trait split into `FastSyncStore` and `RawOverlayStore`; `Store` now has accessors and no capability boolean.
- ADR-021: `NodeTypes`, `NodeComponents`, `EngineApi` sealed; `Store`, `Service`, `PipelineStage`, `StoreProvider` remain open.
- ADR-022: dead `neo_execution::KeyBuilder` removed; `neo_storage::KeyBuilder` is the low-level builder.
- ADR-023: NodeComponents type-state is documented as scaffolded, not functional (0 concrete service impls).
- ADR-024: canonical `neo_primitives::hex_util` added (`encode/decode`, reversed variants, uppercase, prefix strip); removed hex dep from `neo-payloads` and `neo-manifest`; fixed lowercase-only `0x` bug.
- ADR-025: key-building systems documented: A `neo_storage::KeyBuilder`, C native-contract `support::keys`, D `StorageKey::create_with_*`; internal C helpers private; last production hand-rolled key path removed.
- ADR-026: `neo-blockchain::pipeline::validate_stage::NeoValidateStage` implements `neo_engine::ValidateStage`/`PipelineStage` with narrow `ValidateContext`; extracted and tested, not yet wired into live import loop.

## Pattern Adoption
- Adopted from reth: provider traits, sealed composition traits, EngineApi, scaffolded PipelineStage, service traits, anyhow-in-binary, NodeBuilder, feature-gated RPC.
- Adopted from Polkadot/Substrate: bounded context layers, service trait composition, per-domain errors.
- Deferred: RuntimeVersion, 3-crate RPC split, full staged sync, pallet/frame-style plugin system.

## Build & Test
- Main checks: `cargo check --workspace --tests`, `cargo test --workspace`.
- RPC checks: `cargo check -p neo-rpc`, `cargo check -p neo-rpc --features client`, `cargo check -p neo-rpc --features server`.
- Architecture boundary tests: `cargo test -p neo-tests --test layer_boundary_tests`.
