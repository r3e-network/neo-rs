# neo-rs Deep Refactor Roadmap

**Initialized**: 2026-07-04
**Source audit**: `.planning/codebase/deep-audit-2026-07-04.md`
**Method**: spec-driven, phase-by-phase, each phase = independent commit + `cargo test --workspace` green + ADR update

---

## Phase 1 — Dead Code Excision + Native Contract Support Layer (A + C)

**Goal**: Remove the aspirational abstractions that never ran in production, and consolidate the real native-contract duplication. This is the prerequisite for all later phases — later structural changes are unsafe while dead scaffolding misrepresents the architecture.

**Themes**: A (8 findings) + C (3 findings)
**Estimated impact**: ~575 LOC removed, 1 crate deleted, 9 dead traits deleted, `neo-blockchain → neo-engine` dep removed
**Risk**: Low — all deletions are verified zero-caller; support-layer additions are pure refactor

### Plan 1.1 — Dead code excision (Theme A)
- A1: Delete `neo-static-files` crate (orphaned, 0 prod consumers)
- A2: Delete `BlockchainEngineAdapter` (never instantiated) + drop `neo-blockchain → neo-engine` dep
- A3: Delete dead `neo-engine` public state API (`Pipeline`, `CanonicalChain`, `ChainTip`, `BlockBuffer`); keep only `ValidateStage`/`PipelineStage` traits + move `NeoValidateStage` ownership decision
- A4: Delete 9 dead traits (`AsyncSystemContext`, `ApplicationEngineProvider`, 5 plugin-handler traits in neo-payloads, `ConsensusMessage`, `BlockLike`)
- A5: `BlockchainProvider` — delete the 2 stub methods; keep trait but mark `#[allow(dead_code)]` until Phase 3 decides finish-vs-delete
- A6: Delete `OracleNodeProvider` marker trait; split into 3 fields
- A7: Delete `MempoolLike`; hold `Arc<MemoryPool>` directly
- A8: Delete dead `Box<dyn ConsensusSigner>` blanket impl

### Plan 1.2 — Native contract support layer (Theme C)
- C1: Add `neo-native-contracts/src/support/codec.rs` — `decode_stack_value`, `encode_storage_struct`, `StructDecoder`
- C2: Add `neo-native-contracts/src/support/engine.rs` — `require_persisting_block`
- C3: Add `neo-native-contracts/src/support/settings.rs` — `read_hardfork_gated_u32_setting`, promote Policy i64 helpers
- Migrate the ~26 duplicated call sites to the new helpers

**Verification**: `cargo check --workspace --tests` clean, `cargo test --workspace` 3356+ tests pass, layer boundary tests 20/20
**ADR**: ADR-027 (dead-trait excision) + ADR-028 (native contract support layer)

---

## Phase 2 — Cross-Crate Helpers + Test Fixtures (D + E)

**Goal**: Consolidate the cross-crate duplication that doesn't fit in a single crate's support module.

**Themes**: D (4 findings) + E (1 finding)
**Estimated impact**: ~250 LOC removed, 1 new dev-only crate
**Risk**: Low — mechanical consolidation

### Plan 2.1 — Cross-crate helpers (Theme D)
- D1: Promote `invocation_script_from_signature` / `signature_from_invocation` to `neo-vm::ScriptBuilder`
- D2: Add `now_millis()` to `neo-primitives::time`
- D3: Add `elapsed_us` / `elapsed_millis` to `neo-runtime::time` (or `neo-primitives::time`)
- D4: Extend `impl_error_from!` macro for struct-variant `CoreError`; migrate 14 sites

### Plan 2.2 — Test fixtures (Theme E)
- E1: Create `neo-test-fixtures` dev-only workspace member
- Consolidate `make_transaction` / `make_ledger_block` / `store_block` from 4 crates

**Verification**: workspace tests pass, no test regressions
**ADR**: ADR-029 (cross-crate helpers consolidation)

---

## Phase 3 — Crate Consolidation + Architecture Honesty (B + G)

**Goal**: Structural changes — merge the micro-crate, split the monolith, rename the colliding trait.

**Themes**: B (4 findings) + G (4 findings)
**Estimated impact**: 1 crate merged, 1 crate split into 3-4, 1 trait rename, 1 trait extraction
**Risk**: Medium — structural changes, import path updates across workspace

### Plan 3.1 — Crate consolidation (Theme B)
- B1: Merge `neo-engine` into `neo-runtime` (after Phase 1 removed its dead parts)
- B2: Split `neo-rpc` into `neo-rpc-api` / `neo-rpc-client` / `neo-rpc` (+ optional plugin crates)
- B3: Feature-gate `neo-native-contracts` per contract (or split api/impl)
- B4: Flip `neo-hsm` default features to `[]`

### Plan 3.2 — Architecture honesty (Theme G)
- G1: Rename `ConsensusService` trait → `ConsensusApi` (mirror ADR-007)
- G2: Decide runtime service-trait seam: delete dead traits OR move to L2 `neo-runtime-api`
- G3: Extract `Nep17MetadataReader` trait; remove `neo-wallets → neo-execution` dep
- G4: Extract `StoreConfigBundle`; deduplicate `StoreProvider`/`ConfigProvider` impls

**Verification**: workspace tests pass, `cargo check -p neo-rpc --features client` light, layer boundary tests pass
**ADR**: ADR-030 (crate consolidation) + ADR-031 (architecture honesty)

---

## Phase 4 — Async/Blocking Correctness (F)

**Goal**: Fix the two async/blocking correctness issues that force HSM/Nitro signers to spawn their own runtimes.

**Themes**: F (2 findings)
**Risk**: HIGH — touches consensus signing hot path and RPC wallet handlers

### Plan 4.1 — Async signer
- F1: Change `ConsensusSigner::sign` to `async fn` (or split sync/async traits)
- Update all signers: software (sync), Nitro/HSM/Azure/GCP (async)
- Update consensus driver to await

### Plan 4.2 — Wallet future
- F2: Rewrite `await_wallet_future` as `async fn` with `impl Future`
- Push `block_in_place`/`spawn` decision up to RPC handler layer

**Verification**: consensus test suite passes, RPC wallet tests pass, no runtime deadlocks
**ADR**: ADR-032 (async signer correctness)

---

## Phase Dependencies

```
Phase 1 (A+C) ──┬──→ Phase 2 (D+E) ──→ Phase 3 (B+G) ──→ Phase 4 (F)
                │
                └──→ Phase 3 depends on Phase 1's neo-engine cleanup
```

Phase 1 must complete first (dead code removal makes Phase 3's merge safe).
Phase 2 can run after Phase 1 (helpers don't depend on structure).
Phase 3 must follow Phase 1 (crate merge needs dead code gone first).
Phase 4 must follow Phase 3 (async trait changes are cleaner after crate restructure).

## Success Criteria (per phase)

- [ ] `cargo check --workspace --tests` exits 0
- [ ] `cargo test --workspace` — no regressions vs baseline (3356+ tests)
- [ ] `cargo test -p neo-tests --test layer_boundary_tests` — 20/20 pass
- [ ] ADR(s) written and design.md updated
- [ ] Commit is atomic and describes the phase's changes
