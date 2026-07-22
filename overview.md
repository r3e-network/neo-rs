# Historical Architecture Refactor Snapshot (2026-07-05)

> This file records the four-phase refactor as it stood on 2026-07-05. It is
> not the current crate inventory: later work reintroduced `neo-static-files`
> as the production append-only Ledger archive and continued the ChainSpec,
> provider, storage, and execution architecture. Use `README.md`,
> `docs/architecture.md`, and the latest ADRs in `design.md` for current state.

**Commits**: b8afcc0 → a6d7a7a → 504e8ed → f71b431
**ADRs**: ADR-027 through ADR-031 (5 new ADRs)
**Date**: 2026-07-05

## TL;DR

All 4 phases complete. Deleted 2 dead crates, 9 dead trait/impl items (6 dead traits + 1 marker trait + 1 blanket impl + ConsensusMessage), consolidated ~750 lines of duplicated code, decoupled neo-wallets from the execution engine, flipped HSM FFI default off, resolved ConsensusService name collision, and fixed the async signer correctness bug on the consensus hot path. Workspace: 28 → 26 production crates + 1 test crate. 3343 tests pass (0 failures). Architecture health score: 9.4 → 9.6.

## Phase Summary

| Phase | Commit | ADR | Key Changes | Tests |
|-------|--------|-----|-------------|-------|
| 1 | b8afcc0 | 027, 028 | Delete neo-static-files + neo-engine; 6 dead traits; native contract support layer | 3346 ✅ |
| 2 | a6d7a7a | 029 | now_millis, elapsed_us, invocation_from_signature, impl_error_from_struct!, neo-test-fixtures | 3346 ✅ |
| 3 | 504e8ed | 030 | neo-hsm default flip, ConsensusApi rename, Nep17MetadataReader extraction | 3342 ✅ |
| 4 | f71b431 | 031 | Async ConsensusSigner::sign + cascade; await_wallet_future deadlock fix | 3343 ✅ |

## Cumulative Changes

### Crates Deleted (2)
- `neo-static-files` — 0 production consumers (orphaned)
- `neo-engine` — entire public state API had 0 production callers

### Crates Created (1)
- `neo-test-fixtures` — dev-only shared test builders

### ADRs Written (5)
- **ADR-027**: Dead code excision (neo-static-files, neo-engine, 6 dead traits)
- **ADR-028**: Native contract support layer (codec, engine, settings helpers)
- **ADR-029**: Cross-crate helpers + test fixtures
- **ADR-030**: neo-hsm default features, ConsensusApi rename, Nep17MetadataReader
- **ADR-031**: Async ConsensusSigner + await_wallet_future deadlock fix

### Traits
- **Deleted** (9): AsyncSystemContext, ApplicationEngineProvider, SignerProvider, AccountLike, MessageReceivedHandler, MessageLike, ConsensusMessage, Box<dyn ConsensusSigner> impl, OracleNodeProvider
- **Renamed** (1): ConsensusService → ConsensusApi (trait only)
- **Created** (1): Nep17MetadataReader
- **Made async** (1): ConsensusSigner::sign

### Support Modules Created (6)
- `neo-blockchain/src/pipeline/stage_traits.rs`
- `neo-native-contracts/src/support/{codec,engine,settings}.rs`
- `neo-runtime/src/support/time.rs`
- `neo-runtime/src/service/nep17.rs`

### Dependency Changes
- `neo-blockchain` no longer depends on `neo-engine` (deleted)
- `neo-wallets` no longer depends on `neo-execution` (replaced with `neo-runtime`)
- `neo-execution` now depends on `neo-runtime` (one-way, no cycle)
- `neo-hsm` default features: `[]` (was `["pkcs11"]`)
- `neo-hsm` reqwest: blocking → async
- `neo-consensus` gained `async-trait` dependency

## Verification

| Check | Result |
|-------|--------|
| `cargo check --workspace --tests` | ✅ 0 errors |
| `cargo test --workspace` | ✅ 3343 passed, 0 failed |
| `cargo test -p neo-tests --test layer_boundary_tests` | ✅ 20/20 |
| `cargo test -p neo-consensus` | ✅ 137 passed |
| `cargo test -p neo-rpc --features server` | ✅ 655 passed |
| `cargo test -p neo-hsm --features pkcs11` | ✅ 4 passed |
| `cargo test -p neo-tee --features nitro` | ✅ 93 passed |

## Architecture Health Score

9.4 → 9.6

- Phase 1: +0.1 (dead code excision improves honesty)
- Phase 2: ±0 (consolidation, no structural change)
- Phase 3: ±0 (architecture honesty, no structural change)
- Phase 4: +0.1 (correctness fix on consensus hot path)

## Phase 5 — Honesty cleanup

**Commits**: 5b829e6 (ADR-032), 41134a9 (hygiene)

- **ADR-032**: deleted the dead type-state scaffolding (`BlockchainProvider`,
  `NodeComponents`, `FullNode`, `FullNodeTypes`, `FullNodeComponentsExt`) from
  neo-runtime, including the silent `Ok(None)` landmine.
- The current provider surface is deliberately smaller: `neo-config` owns the
  immutable `ChainSpecProvider`, while `neo-runtime` exposes only the narrow
  `StoreProvider` and `TxAdmission` capability traits.
- Restored neo-hsm safety lints and removed dead `do_sign`.
- De-flaked the observability metrics test.
- **Deliberately skipped**: A7 (MempoolLike — documented test seam) and G4
  (StoreConfigBundle — trivial forwarding), both net-negative. B2 (neo-rpc
  split) and B3 (native-contracts split) remain deferred.
