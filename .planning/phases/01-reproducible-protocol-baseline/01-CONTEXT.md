# Phase 1: Reproducible Protocol Baseline - Context

**Gathered:** 2026-07-13
**Status:** Ready for planning

<domain>
## Phase Boundary

Establish a reproducible Neo N3 v3.10.1 protocol baseline: every build surface
uses one immutable VM revision, official network hardfork schedules are encoded
and regression-tested, canonical execution cannot select an unproven engine,
and state-root vote aggregation cannot cross consensus identities. This phase
also closes the workspace, fuzz, dependency, and container verification gates
needed to trust that baseline.

</domain>

<decisions>
## Implementation Decisions

### the agent's Discretion

All implementation choices are at the agent's discretion because this is a
pure infrastructure phase. Neo v3.10.1 and its official network configuration
are the protocol authorities. Reth and Substrate are architecture references,
not sources of Neo consensus behavior. Canonical execution must remain on the
local hardfork-aware VM until differential evidence proves another interpreter
equivalent. Existing user changes in the dirty worktree must be preserved and
reconciled rather than replaced.

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets

- Workspace-wide compatibility, fuzz, CI, Docker, and cargo-deny checks already
  exist and should be strengthened rather than replaced.
- Hardfork schedule tests live beside `neo-config` protocol settings.
- State-root consensus and VM stack-item suites already provide focused homes
  for adversarial identity and semantic regression tests.

### Established Patterns

- Consensus-sensitive behavior is protected by focused regressions plus full
  workspace format, check, test, doctest, and clippy gates.
- Architecture changes are documented in `design.md`; protocol facts cite the
  official Neo implementation or configuration at tag v3.10.1.
- The workspace uses pinned Git revisions for consensus-sensitive external
  code and keeps the fuzz package lockfile independently reproducible.

### Integration Points

- Root and fuzz Cargo manifests/lockfiles, CI workflows, and Docker build
  context must resolve the same VM revision.
- `neo-config` owns network hardfork activation schedules.
- `neo-execution` owns canonical interpreter selection, while `neo-vm` owns
  local hardfork-aware execution semantics.
- `neo-blockchain` owns state-root vote aggregation identity.

</code_context>

<specifics>
## Specific Ideas

Use the official `neo-project/neo-node` v3.10.1 source and network
configuration as the recorded authority for schedules and protocol behavior.
Retain exact dependency and container verification evidence rather than
claiming reproducibility from source inspection alone.

</specifics>

<deferred>
## Deferred Ideas

Fallible storage boundaries, database identity, coordinated lifecycle,
differential parity, live P2P interoperability, full MainNet replay, and
authenticated checkpoint fast sync belong to Phases 2 through 7.

</deferred>
