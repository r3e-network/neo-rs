# neo-rs Deep Refactor — Project State

**Last updated**: 2026-07-05
**Current phase**: Complete (Phase 5 cleanup landed)

---

## Position

- **Phase**: All 4 refactor phases complete, plus a Phase-5 cleanup pass.
- **Refactor phases (ADR-027..031)**: b8afcc0 (Phase 1), a6d7a7a (Phase 2),
  504e8ed (Phase 3), f71b431 (Phase 4) — all landed.
- **Phase-5 cleanup**: 5b829e6 (ADR-032 dead-scaffolding excision), 41134a9
  (hygiene) — landed.

## Key Decisions (this project)

| # | Decision | Rationale | Date |
|---|----------|-----------|------|
| 1 | Execute all 4 phases | User confirmed full scope | 2026-07-04 |
| 2 | Each phase = independent commit + verify | Safety; can stop at any phase boundary | 2026-07-04 |
| 3 | Sync update design.md with ADRs per phase | Keep design.md as source of truth | 2026-07-04 |
| 4 | spec-skill methodology as framework | User invoked @skill:spec-skill explicitly | 2026-07-04 |

## Baseline Metrics (pre-refactor)

- `cargo test --workspace`: 3356 tests, 0 failures
- Layer boundary tests: 20/20
- Crates: 28 (27 workspace members + 1 excluded neo-gui)
- ADRs: 26
- design.md health score: 9.4/10

## Blockers

None currently.

## Notes

- Phase 1 Plan 1.1 A3 (neo-engine deletion) is the highest-judgment call: the
  audit found the entire public state API dead, but `NeoValidateStage`
  (ADR-026) implements `neo_engine::ValidateStage`. Decision: move the
  `ValidateStage` + `PipelineStage` traits into `neo-blockchain::pipeline`
  (where the one impl already lives), then delete `neo-engine` entirely.
  This collapses the L3 trait-crate split that ADR-007 only renamed.
- Phase 3 B2 (neo-rpc split) is the largest single structural change. If
  context budget gets tight, defer B2 to a follow-up session.
- A7 (MempoolLike) and G4 (StoreConfigBundle) were deliberately SKIPPED as
  net-negative (documented test-injection seam / trivial forwarding); B2
  (neo-rpc split) and B3 (native-contracts split) remain DEFERRED.
