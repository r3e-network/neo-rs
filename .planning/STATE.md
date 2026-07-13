---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: Production-ready Neo N3 v3.10.1 Node
current_phase: 01
current_phase_name: Reproducible Protocol Baseline
status: executing
stopped_at: Completed 01-01-PLAN.md
last_updated: "2026-07-13T16:02:37.087Z"
last_activity: 2026-07-13
last_activity_desc: Phase 01 execution started
progress:
  total_phases: 7
  completed_phases: 0
  total_plans: 2
  completed_plans: 1
  percent: 0
---

# Project State

## Project Reference

See: `.planning/PROJECT.md` (updated 2026-07-13)

**Core value:** Import, validate, execute, persist, and expose canonical Neo N3 state exactly as Neo v3.10.1 does.
**Current focus:** Phase 01 — Reproducible Protocol Baseline

## Current Position

Phase: 01 (Reproducible Protocol Baseline) — EXECUTING
Plan: 2 of 2
Status: Ready to execute
Last activity: 2026-07-13 — Phase 01 execution started

Progress: [----------] 0%

## Completed Previous Milestone

The July 2026 deep architecture refactor and cleanup landed before this
production-readiness milestone. Its ADR-027 through ADR-043 decisions remain
part of the architecture baseline; deferred production work is represented in
the new roadmap rather than being treated as complete.

## Accumulated Context

### Decisions

- Neo v3.10.1 and its official network configuration are protocol authorities; reth and Substrate are architecture references only.
- Canonical execution uses the local hardfork-aware VM until differential evidence proves any optimized interpreter equivalent.
- Full MainNet replay/state-root parity is a release gate, not an aspirational follow-up.
- Fast sync must evolve from accelerated full-history archive replay to an authenticated checkpoint/state bootstrap with canonical catch-up.

### Evidence Established This Session

- `neo-vm-rs` is pinned to revision `3081e83db3716fd51dc58c0afc039290d2d07253` in root and fuzz manifests.
- Official Neo v3.10.1 Gorgon schedules are MainNet `12,020,000` and TestNet `17,960,000`; Huyao is unscheduled.
- State-root vote pools are isolated by `(version, index, root_hash)`.
- Canonical application execution no longer automatically dispatches to the external interpreter.
- No retained local full-MainNet replay or per-height parity database exists yet.

### Blockers/Concerns

- MDBX read and snapshot-open errors are still represented through infallible `Option`-based traits and can be mistaken for absent state.
- Databases do not yet enforce persisted network/genesis/schema/store-role identity before use.
- Fast sync currently relies on an HTTPS manifest plus MD5 integrity, imports archives with reduced verification, and makes independent reference proof optional.
- P2P startup failure and task-join/flush shutdown ordering need end-to-end failure tests; RPC bind/startup propagation is covered.
- Full transaction-bearing MainNet replay and state-root parity remain unproven.

## Session Continuity

Last session: 2026-07-13T16:02:37.083Z
Stopped at: Completed 01-01-PLAN.md
Resume file: None

## Performance Metrics

| Phase | Plan | Duration | Notes |
|-------|------|----------|-------|
| Phase 01 P01 | 13 min | 2 tasks | 18 files |
