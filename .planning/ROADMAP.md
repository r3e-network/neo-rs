# Roadmap: neo-rs Production Node

## v1.0 Production-ready Neo N3 v3.10.1 Node (In Progress)

## Overview

This milestone turns the existing Neo N3 implementation into a node whose
correctness is demonstrated against Neo v3.10.1. The order is deliberate:
establish deterministic protocol semantics first, make persistence fail closed,
build differential evidence, prove live interoperability, then prove full
MainNet replay before optimizing a verified checkpoint-based fast-sync path.
The completed 2026-07 architecture refactor remains in Git history and
`.planning/codebase/deep-audit-2026-07-04.md`; it is not counted as unfinished
work in this milestone.

## Phases

- [x] **Phase 1: Reproducible Protocol Baseline** - Pin v3.10.1 consensus inputs and remove known nondeterministic execution paths. (completed 2026-07-13)
- [ ] **Phase 2: Fail-Closed Storage and Lifecycle** - Make persistence, startup, and shutdown errors explicit and non-corrupting.
- [ ] **Phase 3: Differential Protocol Parity** - Compare VM, native-contract, transaction, and state transitions with Neo v3.10.1.
- [ ] **Phase 4: P2P Interoperability and Canonical Sync** - Prove sustained communication and ordered block import against real Neo peers.
- [ ] **Phase 5: Full MainNet Replay and State Parity** - Replay MainNet from genesis and prove canonical state at protocol boundaries.
- [ ] **Phase 6: Verified Checkpoint Fast Sync** - Replace trusted archive replay with authenticated state bootstrap and canonical catch-up.
- [ ] **Phase 7: Production Hardening and Release** - Complete security, performance, operations, and release gates.

## Milestone-wide Exit Gates

Every phase must retain the relevant focused tests and pass locked workspace
format, check, test, doctest, and clippy gates before verification can pass.
Changes to fuzzed, dependency, container, or deployment surfaces must also pass
their corresponding locked fuzz, cargo-deny, and clean-context Docker gates.
Every architecture-level decision must update or add an ADR in `design.md`.

## Phase Details

### Phase 1: Reproducible Protocol Baseline

**Goal**: A clean checkout builds against one immutable VM revision and canonical execution uses only semantics proven for the selected Neo hardfork.
**Depends on**: Nothing
**Requirements**: [PROTO-01, BUILD-01, CONSENSUS-01]
**Success Criteria**:

  1. CI, fuzzing, and Docker resolve the same pinned `neo-vm-rs` revision without a sibling checkout.
  2. MainNet and TestNet hardfork schedules match the official Neo v3.10.1 configuration at every activation boundary.
  3. Canonical block execution cannot automatically dispatch to a non-hardfork-aware interpreter.
  4. Competing state-root votes cannot be aggregated across version, height, or root hash.
  5. Workspace format, check, test, doctest, and clippy gates pass.

**Plans**: 2/2 plans complete

Plans:

- [x] 01-01-PLAN.md
- [x] 01-02-PLAN.md

**Wave 1**

- [x] 01-01: Complete the pinned VM v0.2 migration and consensus-safety regressions.

**Wave 2** *(blocked on Wave 1 completion)*

- [ ] 01-02: Validate CI, fuzz, Docker, documentation, and dependency reproducibility from a clean build context.

### Phase 2: Fail-Closed Storage and Lifecycle

**Goal**: Storage and service failures cannot be mistaken for missing chain state or reported as successful node startup/shutdown.
**Depends on**: Phase 1
**Requirements**: [STORAGE-01, STORAGE-02, OPS-01]
**Success Criteria**:

  1. Backend read and snapshot-open errors propagate through result-bearing APIs and abort canonical mutation.
  2. Every database persists and validates network magic, genesis hash, schema version, and store role before use.
  3. RPC and P2P bind failures make startup fail, and shutdown joins critical tasks before the final durable flush.
  4. Crash, partial-commit, wrong-network, and incompatible-schema tests leave the last committed canonical state recoverable.

**Plans**: 3 plans

Plans:

- [ ] 02-01: Introduce fallible read/snapshot boundaries and migrate canonical consumers.
- [ ] 02-02: Add database identity/schema validation and coordinated recovery tests.
- [ ] 02-03: Make service startup and graceful shutdown transactional and observable.

### Phase 3: Differential Protocol Parity

**Goal**: Consensus-critical behavior is continuously compared with the official Neo v3.10.1 implementation across all scheduled hardforks.
**Depends on**: Phase 2
**Requirements**: [PROTO-02, VM-01, NATIVE-01]
**Success Criteria**:

  1. A versioned corpus covers script execution, faults, fees, notifications, storage writes, and native-contract side effects.
  2. Differential tests run immediately before and after every MainNet/TestNet hardfork height.
  3. Transaction results, application logs, fees, and post-state match Neo v3.10.1 byte-for-byte or by a documented canonical encoding.
  4. Any optimized VM path remains disabled until the same corpus proves equivalent behavior.

**Plans**: 3 plans

Plans:

- [ ] 03-01: Build the Neo v3.10.1 differential fixture and trace harness.
- [ ] 03-02: Close VM and serialization divergences across hardfork boundaries.
- [ ] 03-03: Close native-contract, fee, notification, and storage-transition divergences.

### Phase 4: P2P Interoperability and Canonical Sync

**Goal**: The node interoperates with Neo N3 peers and imports one ordered canonical chain under realistic network conditions.
**Depends on**: Phase 3
**Requirements**: [P2P-01, SYNC-01, REORG-01, VALIDATION-01, VALIDATION-02]
**Success Criteria**:

  1. Handshake, address exchange, headers, inventories, payload requests, blocks, transactions, and extensible payloads interoperate with Neo v3.10.1 nodes.
  2. Peer scoring, timeouts, backpressure, duplicate suppression, and bounded queues withstand malformed or slow peers.
  3. Policy-aware stateful validation runs in the ordered canonical command loop before execution or persistence, preserving Neo v3.10.1 witness and peer-import semantics.
  4. Header and block sync resume after disconnects and process forks/reorgs without violating ordered state mutation.
  5. Multi-day TestNet/MainNet soak reports show advancing height, stable memory, bounded disk growth, and no silent service failure.

**Plans**: 4 plans

Plans:

- [ ] 04-01: Add cross-client wire and live-peer interoperability tests.
- [ ] 04-02: Wire policy-aware validation into the ordered canonical import loop with Neo v3.10.1 validity semantics.
- [ ] 04-03: Harden peer lifecycle, request scheduling, backpressure, and reorg handling.
- [ ] 04-04: Run and retain network soak evidence.

### Phase 5: Full MainNet Replay and State Parity

**Goal**: A genesis-to-tip MainNet replay produces the same canonical blocks, transaction outcomes, and state roots as Neo v3.10.1.
**Depends on**: Phase 4
**Requirements**: [REPLAY-01, STATE-01, RECOVERY-01]
**Success Criteria**:

  1. A transaction-bearing replay starts from an empty database and reaches the selected MainNet tip without bypassing canonical execution.
  2. Block hashes, transaction VM states, fees, notifications, and state roots match trusted Neo v3.10.1 references at every retained checkpoint.
  3. Hardfork boundary windows and deterministic random samples have retained machine-readable parity reports.
  4. Interrupted replay resumes from the last atomic commit and reproduces the uninterrupted final state root.

**Plans**: 3 plans

Plans:

- [ ] 05-01: Make the replay runner self-contained and reference-data aware.
- [ ] 05-02: Run bounded transaction-bearing milestones and close the first divergence.
- [ ] 05-03: Complete and retain a full MainNet replay proof.

### Phase 6: Verified Checkpoint Fast Sync

**Goal**: A new node authenticates a checkpoint state, installs it atomically, and catches up through normal canonical validation substantially faster than full replay.
**Depends on**: Phase 5
**Requirements**: [FASTSYNC-01, FASTSYNC-02, SECURITY-01]
**Success Criteria**:

  1. Fast sync has an explicit trust model using authenticated manifests/checkpoints and strong content hashes; MD5 is never treated as authenticity.
  2. State installation is staged, schema/network checked, root verified, and atomically promoted or discarded after a crash.
  3. Headers establish the canonical checkpoint and post-checkpoint blocks execute through the normal ordered import pipeline.
  4. Corrupt, equivocated, stale, wrong-network, and unavailable checkpoint sources fail closed with actionable diagnostics.
  5. Reproducible benchmarks show wall-clock, CPU, memory, bandwidth, and disk improvements over full archive replay.

**Plans**: 3 plans

Plans:

- [ ] 06-01: Specify checkpoint format, trust policy, and atomic installation lifecycle using reth/Substrate patterns.
- [ ] 06-02: Implement authenticated state download, verification, and promotion.
- [ ] 06-03: Integrate canonical catch-up, adversarial tests, and benchmarks.

### Phase 7: Production Hardening and Release

**Goal**: The node has the evidence, controls, and operational surface required for a supported production release.
**Depends on**: Phase 6
**Requirements**: [SECURITY-02, PERF-01, RELEASE-01]
**Success Criteria**:

  1. Fuzz, property, concurrency, crash-recovery, and resource-exhaustion suites pass with retained reports.
  2. Metrics and health endpoints distinguish peer, sync, execution, persistence, state-root, and degraded-service failures.
  3. Operator documentation covers secure configuration, backup/restore, upgrades, rollback, pruning, and incident diagnosis.
  4. Reproducible release artifacts, SBOM, dependency audit, threat model, and independent review are complete.
  5. Production claims cite full replay, live interoperability, soak, and fast-sync evidence rather than test counts alone.
  6. The repository file-size debt baseline is empty: oversized Rust, operational Python, and Python test modules are split without weakening the 900-line review budget.

**Plans**: 3 plans

Plans:

- [ ] 07-01: Complete security and resilience gates.
- [ ] 07-02: Tune performance from measured profiles and finish observability.
- [ ] 07-03: Produce release artifacts, operations documentation, and acceptance evidence.

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Reproducible Protocol Baseline | 2/2 | Complete   | 2026-07-13 |
| 2. Fail-Closed Storage and Lifecycle | 0/3 | Not started | - |
| 3. Differential Protocol Parity | 0/3 | Not started | - |
| 4. P2P Interoperability and Canonical Sync | 0/4 | Not started | - |
| 5. Full MainNet Replay and State Parity | 0/3 | Not started | - |
| 6. Verified Checkpoint Fast Sync | 0/3 | Not started | - |
| 7. Production Hardening and Release | 0/3 | Not started | - |
