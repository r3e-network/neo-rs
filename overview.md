# Phase 1-3 Delivery Summary — Deep Architecture Refactor

**Commits**: `b8afcc0` (Phase 1) → `a6d7a7a` (Phase 2) → `c5f2c7e` (Phase 3)
**Date**: 2026-07-05
**ADRs**: ADR-027 through ADR-030

## TL;DR

3 of 4 phases complete. Deleted 2 dead crates, 6 dead traits, consolidated ~500 lines of duplicated boilerplate, flipped HSM FFI default off, resolved ConsensusService name collision, and decoupled neo-wallets from the execution engine. Workspace: 28 → 26 production crates + 1 test crate. 3342 tests pass (0 failures).

## Phase Summary

| Phase | Commit | ADR | Key Changes | Tests |
|-------|--------|-----|-------------|-------|
| 1 | b8afcc0 | 027, 028 | Delete neo-static-files + neo-engine; 6 dead traits; native contract support layer | 3346 ✅ |
| 2 | a6d7a7a | 029 | now_millis, elapsed_us, invocation_from_signature, impl_error_from_struct!, neo-test-fixtures | 3346 ✅ |
| 3 | c5f2c7e | 030 | neo-hsm default flip, ConsensusApi rename, Nep17MetadataReader extraction | 3342 ✅ |

## Cumulative Changes

### Crates Deleted (2)
- `neo-static-files` — 0 production consumers
- `neo-engine` — entire public state API had 0 production callers

### Crates Created (1)
- `neo-test-fixtures` — dev-only shared test builders

### Traits Deleted (6)
- `AsyncSystemContext`, `ApplicationEngineProvider`, `SignerProvider`, `AccountLike`, `MessageReceivedHandler`, `MessageLike`, `ConsensusMessage`, `Box<dyn ConsensusSigner>` impl, `OracleNodeProvider`

### Traits Renamed (1)
- `ConsensusService` → `ConsensusApi` (trait only; struct in neo-consensus unchanged)

### Traits Created (1)
- `Nep17MetadataReader` in neo-runtime (decouples neo-wallets from neo-execution)

### Support Modules Created (6)
- `neo-blockchain/src/pipeline/stage_traits.rs` — ValidateStage + PipelineStage (from neo-engine)
- `neo-native-contracts/src/support/codec.rs` — StackValue encode/decode helpers
- `neo-native-contracts/src/support/engine.rs` — require_persisting_block
- `neo-native-contracts/src/support/settings.rs` — hardfork-gated setting readers
- `neo-runtime/src/time.rs` — elapsed_us/elapsed_millis with saturating conversion
- `neo-runtime/src/service/nep17.rs` — Nep17MetadataReader trait

### Macros Extended (1)
- `impl_error_from_struct!` — new macro for struct-variant CoreError conversions (13 sites migrated)

### Dependency Changes
- `neo-blockchain` no longer depends on `neo-engine` (deleted)
- `neo-wallets` no longer depends on `neo-execution` (replaced with `neo-runtime`)
- `neo-execution` now depends on `neo-runtime` (one-way, no cycle)
- `neo-hsm` default features: `[]` (was `["pkcs11"]`)
- `neo-node` hsm feature: `["dep:neo-hsm", "neo-hsm/pkcs11"]`

## Remaining: Phase 4 (Async Signer Correctness)

**Status**: Approved, not yet executed

**Scope**:
- F1: `ConsensusSigner::sign` is sync but production signers (NitroEnclave, Pkcs11, AzureKeyVault, GcpKms) block on network/HSM round-trips. Change to `async fn` or split sync/async variants.
- F2: `await_wallet_future` in neo-rpc spawns a fresh CurrentThread runtime per call — restructure to `async fn` with `impl Future`.

**Risk**: HIGH — touches consensus signing hot path. But current behavior is a correctness/performance bug (sync blocking on network in consensus task).
