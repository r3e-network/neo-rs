# Phase 1 Delivery Summary — Deep Architecture Refactor

**Commit**: `b8afcc0` — Phase 1: Dead code excision + native contract support layer
**Date**: 2026-07-04
**ADRs**: ADR-027 (dead code excision), ADR-028 (native contract support layer)

## TL;DR

Deleted 2 dead crates, 6 dead traits, and consolidated ~265 lines of duplicated
boilerplate into 3 support modules. Workspace went from 28 → 26 crates.
Architecture health score 9.4 → 9.5. All 3346 tests pass.

## What Was Done

### Phase 1.1 — Dead Code Excision (ADR-027)

| Action | Detail |
|--------|--------|
| Deleted `neo-static-files` crate | L1c orphan — 0 production consumers. Real cold-archive code lives in `neo-blockchain/src/ledger/static_archive.rs` |
| Deleted `neo-engine` crate | L3 — entire public state API had 0 production callers. `BlockchainEngineAdapter` was never instantiated. `ValidateStage` + `PipelineStage` traits moved to `neo-blockchain/src/pipeline/stage_traits.rs` |
| Deleted 6 dead traits | `AsyncSystemContext`, `ApplicationEngineProvider`, `SignerProvider`, `AccountLike`, `MessageReceivedHandler`, `MessageLike`, `ConsensusMessage`, `Box<dyn ConsensusSigner>` impl |
| Restructured `OracleNodeProvider` | 0-method marker trait → 3 explicit fields (`config`, `store`, `tx`) on `OracleService` |
| Correctly kept 2 traits | `WalletChangedHandler` (has prod impl in OracleService) and `BlockLike` (has prod impl on Block) — audit was wrong about these |

### Phase 1.2 — Native Contract Support Layer (ADR-028)

| File | Purpose | Sites Consolidated |
|------|---------|-------------------|
| `support/codec.rs` | `decode_stack_value`, `encode_storage_struct`, `StructDecoder` | 14 decode + 12 encode + 8 from_stack_value |
| `support/engine.rs` | `require_persisting_block` | 4 sites |
| `support/settings.rs` | `read_hardfork_gated_u32_setting` + i64 helpers | 3 snapshot readers + 12 BigInt→i64 sites |

19 production files + 8 test files migrated. Data encoding is byte-identical.

## Verification

| Check | Result |
|-------|--------|
| `cargo check --workspace --tests` | ✅ 0 errors, 0 warnings |
| `cargo test --workspace` | ✅ 3346 passed, 0 failed |
| `cargo test -p neo-native-contracts` | ✅ 377 passed, 0 failed |
| `cargo test -p neo-tests --test layer_boundary_tests` | ✅ 20/20 pass |

## File Changes

- **Deleted**: `neo-static-files/` (entire directory), `neo-engine/` (entire directory),
  `neo-blockchain/src/service/engine_adapter.rs`, `neo-execution/src/runtime/engine_provider.rs`
- **Created**: `neo-blockchain/src/pipeline/stage_traits.rs`,
  `neo-native-contracts/src/support/codec.rs`, `support/engine.rs`, `support/settings.rs`
- **Modified**: 19 production files in neo-native-contracts, 8 test files,
  `Cargo.toml`, `Dockerfile`, `layer_boundary_tests.rs`, `design.md`
- **Total diff**: 265 files changed, +2263 / -3272 lines (net -1009 lines)

## Next Steps (Approved, Not Yet Executed)

- **Phase 2**: Cross-crate helpers (`now_millis`, `elapsed_us`, `invocation_script`,
  `impl_error_from` macro) + `neo-test-fixtures` dev crate
- **Phase 3**: Split `neo-rpc` (275 files) into api/client/server + architecture
  honesty renames
- **Phase 4**: Async signer correctness (`ConsensusSigner::sign` → async)
