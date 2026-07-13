---
phase: 01-reproducible-protocol-baseline
plan: 01
subsystem: protocol
tags: [neo-vm, hardforks, notifications, state-root, consensus]
requires: []
provides:
  - Alias-preserving StackValue graph conversion with conflict rejection
  - Neo v3.10.1 notification identity and immutability behavior
  - Official MainNet and TestNet hardfork boundaries
  - Canonical local VM execution and full state-root vote identity
affects: [differential-protocol-parity, canonical-sync, mainnet-replay]
tech-stack:
  added: []
  patterns:
    - Memoized compound graph reconstruction
    - Hardfork behavior selected by the local stateful VM
    - Consensus aggregation keyed by complete payload identity
key-files:
  created: []
  modified:
    - neo-vm/src/stack_item/stack_item.rs
    - neo-execution/src/interop/application_engine_helper.rs
    - neo-config/src/settings/hardfork.rs
    - neo-blockchain/src/state_root/consensus.rs
key-decisions:
  - "Canonical execution remains on the local hardfork-aware VM until differential parity is proven."
  - "Compound IDs and state-root votes are identity-bearing protocol data and fail closed on conflicts."
patterns-established:
  - "Bridge identity: memoize compound IDs before recursive descent and compare repeated definitions."
  - "Quorum identity: aggregate only votes with matching version, block index, and root hash."
requirements-completed: [PROTO-01, CONSENSUS-01]
coverage:
  - id: D1
    description: "Repeated compound IDs preserve aliases and conflicting definitions are rejected."
    requirement: PROTO-01
    verification:
      - kind: unit
        ref: "cargo +1.89.0 test --locked -p neo-vm stack_value"
        status: pass
    human_judgment: false
  - id: D2
    description: "Notifications match pre- and post-Domovoi identity and immutability semantics."
    requirement: PROTO-01
    verification:
      - kind: unit
        ref: "cargo +1.89.0 test --locked -p neo-execution get_notifications"
        status: pass
    human_judgment: false
  - id: D3
    description: "Built-in network hardfork schedules match Neo v3.10.1 at every boundary."
    requirement: PROTO-01
    verification:
      - kind: unit
        ref: "cargo +1.89.0 test --locked -p neo-config hardfork"
        status: pass
    human_judgment: false
  - id: D4
    description: "Canonical application execution cannot automatically dispatch to the external interpreter."
    requirement: PROTO-01
    verification:
      - kind: unit
        ref: "cargo +1.89.0 test --locked -p neo-execution canonical_execution"
        status: pass
      - kind: unit
        ref: "cargo +1.89.0 test --locked -p neo-execution zero_shift"
        status: pass
    human_judgment: false
  - id: D5
    description: "State-root quorums cannot combine votes across version, height, or root hash."
    requirement: CONSENSUS-01
    verification:
      - kind: unit
        ref: "cargo +1.89.0 test --locked -p neo-blockchain state_root::consensus::tests"
        status: pass
    human_judgment: false
duration: 13 min
completed: 2026-07-13
status: complete
---

# Phase 1 Plan 1: Protocol Semantics and Consensus Safety Summary

**Alias-correct VM graph conversion, Neo v3.10.1 hardfork behavior, canonical local execution, and isolated state-root quorums**

## Performance

- **Duration:** 13 min
- **Started:** 2026-07-13T15:49:00Z
- **Completed:** 2026-07-13T16:01:38Z
- **Tasks:** 2
- **Files modified:** 18

## Accomplishments

- Preserved shared compound object identity and rejected conflicting compound definitions.
- Matched Neo's notification identity and immutable-copy behavior across Domovoi.
- Locked canonical execution and state-root aggregation to complete protocol identities.
- Passed the full workspace test-aware compiler check with Rust 1.89.0 and `--locked`.

## Task Commits

1. **Task 1: Reconcile compound graph and notification semantics** - `f9b08689`
2. **Task 2: Lock hardfork execution and state-root quorum identities** - `f658d421`

## Files Created/Modified

- `neo-vm/src/stack_item/stack_item.rs` - Memoized compound graph conversion and conflict checks.
- `neo-payloads/src/execution/notify_event_args.rs` - Stored immutable notification state.
- `neo-execution/src/interop/application_engine_helper.rs` - Domovoi-aware notification projection.
- `neo-config/src/settings/hardfork.rs` - Official built-in network schedules.
- `neo-execution/src/application_engine/storage_ops/load_execute_storage.rs` - Sole canonical local execution route.
- `neo-blockchain/src/state_root/consensus.rs` - Full state-root quorum identity.

## Decisions Made

- Official Neo v3.10.1 configuration remains the hardfork authority.
- The external interpreter is retained only for differential experiments until Phase 3 proves equivalence.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Included the retained external interpreter boundary files**
- **Found during:** Task 2
- **Issue:** The plan named the canonical execution file but omitted the adjacent v0.2 constructor update and explicit non-canonical module boundary.
- **Fix:** Included `external_vm.rs` and `application_engine/mod.rs` in the task commit.
- **Verification:** Canonical execution sentinels and `cargo +1.89.0 check --workspace --tests --locked` pass.
- **Committed in:** `f658d421`

**Total deviations:** 1 auto-fixed (1 missing critical).
**Impact on plan:** Required to keep the retained differential component buildable without widening the canonical execution surface.

## Issues Encountered

- Rust 1.89.0 was partially installed. Rustup completed the existing installation before verification; the final `rustc`, `cargo`, `rustfmt`, and Clippy components all report 1.89.0-compatible versions.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Semantic and consensus gates are ready for the reproducible CI, documentation, and container proof in Plan 01-02.
- Full differential parity, live peer interoperability, and MainNet replay remain intentionally unclaimed.

## Self-Check: PASSED

---
*Phase: 01-reproducible-protocol-baseline*
*Completed: 2026-07-13*
