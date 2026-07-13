# neo-rs Production Node Architecture Refactor

## What This Is

`neo-rs` is a Rust implementation of a Neo blockchain node, evolved as a production-grade architecture rather than a direct line-by-line port. This GSD project tracks the next architecture milestone: learning from reth and Polkadot/Substrate while preserving Neo C# protocol parity, with a focus on making synchronization, validation, execution, persistence, networking, RPC, state-root consistency, and observability robust enough for a real node.

## Core Value

A production-grade `neo-rs` node must import, validate, execute, persist, and expose canonical Neo chain state correctly and observably, while staying compatible with Neo C# consensus behavior.

## Current Milestone: v1.0 Production-ready Neo N3 v3.10.1 Node

**Goal:** Demonstrate end-to-end Neo N3 v3.10.1 compatibility and production
operability through reproducible builds, fail-closed storage, differential
protocol evidence, live network sync, full MainNet replay, and authenticated
checkpoint fast sync.

**Target features:**
- Exact Neo v3.10.1 execution, validation, networking, and persisted-state parity.
- Transaction-bearing genesis-to-tip MainNet replay with retained evidence.
- Authenticated checkpoint state bootstrap followed by canonical catch-up.

## Requirements

### Validated

- ✓ Layered workspace architecture is established across 29 workspace members — existing design baseline.
- ✓ ADR-driven architecture documentation exists in `design.md` — ADR-001 through ADR-043 before this milestone.
- ✓ Canonical hex utilities and key-building cleanup are in place — previous architecture repair milestone.
- ✓ `NeoValidateStage` exists as a tested pipeline stage scaffold — previous milestone, not yet fully wired into live import.
- ✓ Main architectural verification commands are known: `cargo check --workspace --tests`, `cargo test --workspace`, `cargo test -p neo-tests --test layer_boundary_tests`.

### Active

- [ ] Keep every consensus dependency pinned and every network hardfork schedule aligned with Neo v3.10.1.
- [ ] Refactor the live block import validation path into a policy-aware validation pipeline.
- [ ] Wire `NeoValidateStage` into production import loops without changing execution, persistence, or commit ordering.
- [ ] Preserve Neo C# parity: do not treat block-production-only limits as peer import hard validity rules.
- [ ] Preserve consensus witness verification through the existing authoritative header verification path.
- [ ] Make storage reads, snapshot creation, service startup, and shutdown fail closed without hiding backend errors as absent state.
- [ ] Build hardfork-aware differential execution and state-transition evidence against Neo v3.10.1.
- [ ] Prove P2P interoperability and sustained canonical synchronization against real Neo N3 peers.
- [ ] Complete a transaction-bearing genesis-to-tip MainNet replay with retained block, transaction, and state-root parity reports.
- [ ] Replace archive replay fast sync with an authenticated checkpoint/state bootstrap and normal canonical catch-up, using reth and Substrate patterns where they fit Neo.
- [ ] Improve performance and observability around peers, import stages, validation failures, commit latency, accepted-prefix behavior, and state-service flushes.
- [ ] Keep ADR documentation current for every architecture-level decision.
- [ ] Maintain workspace-wide format, compile, test, doctest, clippy, fuzz, and deployment health after each phase.

### Out of Scope

- Rewriting the node from scratch — this project is a deep brownfield refactor, not a greenfield rebuild.
- Replacing Neo protocol semantics with reth or Polkadot semantics — external projects are references for engineering structure, not sources of consensus rules.
- Changing consensus-critical block acceptance behavior without parity evidence — correctness beats elegance.
- Moving stateful validation into concurrent import-queue preverification — canonical validation must stay in the ordered command loop.
- Treating an optimized VM, downloaded archive, or reference RPC as trusted without an explicit and tested trust model.
- Cosmetic crate reshuffling without measurable verification, safety, or maintainability gain.

## Context

The current architecture has already gone through several repair iterations.
The workspace has 29 members with domain-specific errors, provider-trait
decoupling, sealed composition traits, staged pipeline scaffolding, and
ADR-backed design decisions. `design.md` is the source of truth and currently
records ADR-001 through ADR-043 with architecture health around 9.5/10 before
this production-readiness milestone.

Recent completed work includes:

- Splitting the `Store` god trait into focused capability traits.
- Sealing selected runtime composition traits.
- Removing dead `neo_execution::KeyBuilder` wrappers.
- Introducing canonical `neo_primitives::hex_util` and removing duplicated hex handling.
- Cleaning native-contract key-building APIs.
- Extracting `neo-blockchain::pipeline::validate_stage::NeoValidateStage` with focused tests.

The production-readiness roadmap now begins with a reproducible Neo v3.10.1
protocol baseline, then moves through fail-closed persistence, differential
parity, live P2P sync, full MainNet replay, verified checkpoint fast sync, and
release hardening. Cheap/stateless preflight may run early, but stateful
canonical validation stays in the ordered import loop where tip, snapshot, and
commit boundaries are consistent.

## Constraints

- **Protocol compatibility**: Neo C# parity is mandatory — refactors must not silently change consensus-critical acceptance rules.
- **Validation policy boundary**: `MaxTransactionsPerBlock` is a dBFT primary-side block production limit, not a generic peer import hard validity gate.
- **Consensus verification**: Existing header verification and witness verification remain authoritative until an equivalent, proven replacement exists.
- **Mutation ownership**: Canonical chain mutation stays inside the `BlockchainService` command loop; concurrent queues must not own stateful validation decisions.
- **Commit safety**: Execution, persistence, ledger hot-cache updates, store commits, state-service flushes, and bulk finalization ordering must not be changed casually.
- **Execution authority**: Canonical execution uses the local hardfork-aware VM until differential evidence proves an alternative equivalent.
- **Replay evidence**: Full MainNet replay and state-root parity are release gates; unit/integration test counts cannot substitute for them.
- **Fast-sync trust**: MD5 can detect accidental corruption but is not authenticity proof; checkpoint sources and roots require an explicit trust policy.
- **Verification discipline**: Each phase must pass relevant crate tests and workspace-wide source gates before being considered complete.
- **Documentation discipline**: Architecture decisions must be captured in `design.md` through ADR updates or new ADRs.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Use reth and Polkadot/Substrate as architecture references, not protocol authorities | They provide proven node-engineering patterns, but Neo C# defines Neo consensus behavior | ✓ Good |
| Establish a reproducible v3.10.1 protocol baseline before broader refactors | Moving dependency and hardfork inputs invalidate every later parity claim | ✓ Adopted |
| Keep canonical execution on the local hardfork-aware VM | The external interpreter has confirmed hardfork and coercion divergences | ✓ Adopted |
| Keep consensus witness verification in the existing header verification path | `NeoValidateStage` currently does not replace `neo_execution::Helper::verify_witness` semantics | ✓ Adopted |
| Keep stateful validation out of `BlockImportQueue::check` | Import queues can preverify cheaply, but they do not have ordered canonical tip/snapshot context | ✓ Good |
| Require full MainNet replay/state-root parity before a production claim | Short synthetic runs cannot establish whole-chain correctness | ✓ Adopted |
| Evolve fast sync toward authenticated state checkpoints | Current archive replay remains O(full history) and has an insufficient trust boundary | ✓ Adopted |
| Require ADR updates for each architecture-level refactor | Prevents architecture drift and preserves the reasoning behind trade-offs | ✓ Good |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `$gsd-transition`):
1. Move verified requirements to Validated with retained evidence.
2. Record newly discovered requirements, invalid assumptions, and decisions.
3. Keep the project description and scope aligned with the running node.

**After each milestone:**
1. Recheck the core value and every production claim against retained evidence.
2. Audit exclusions and deferred work before archiving the milestone.
3. Update architecture, operations, and release documentation together.

---
*Last updated: 2026-07-13 after production-readiness review and roadmap reset*
