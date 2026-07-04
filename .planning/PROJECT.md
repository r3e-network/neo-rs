# neo-rs Production Node Architecture Refactor

## What This Is

`neo-rs` is a Rust implementation of a Neo blockchain node, evolved as a production-grade architecture rather than a direct line-by-line port. This GSD project tracks the next architecture milestone: learning from reth and Polkadot/Substrate while preserving Neo C# protocol parity, with a focus on making synchronization, validation, execution, persistence, networking, RPC, state-root consistency, and observability robust enough for a real node.

## Core Value

A production-grade `neo-rs` node must import, validate, execute, persist, and expose canonical Neo chain state correctly and observably, while staying compatible with Neo C# consensus behavior.

## Requirements

### Validated

- ✓ Layered workspace architecture is established across 28 crates — existing design baseline.
- ✓ ADR-driven architecture documentation exists in `design.md` — 26 accepted ADRs before this milestone.
- ✓ Canonical hex utilities and key-building cleanup are in place — previous architecture repair milestone.
- ✓ `NeoValidateStage` exists as a tested pipeline stage scaffold — previous milestone, not yet fully wired into live import.
- ✓ Main architectural verification commands are known: `cargo check --workspace --tests`, `cargo test --workspace`, `cargo test -p neo-tests --test layer_boundary_tests`.

### Active

- [ ] Refactor the live block import validation path into a policy-aware validation pipeline.
- [ ] Wire `NeoValidateStage` into production import loops without changing execution, persistence, or commit ordering.
- [ ] Preserve Neo C# parity: do not treat block-production-only limits as peer import hard validity rules.
- [ ] Preserve consensus witness verification through the existing authoritative header verification path.
- [ ] Progressively stage execution, persistence, commit, and indexing boundaries using best practices from reth and Polkadot/Substrate.
- [ ] Improve state-root consistency validation and failure isolation for production sync and replay workflows.
- [ ] Improve RPC/network boundaries so service APIs remain decoupled from composition-root internals.
- [ ] Improve performance and observability around import stages, validation failures, commit latency, accepted-prefix behavior, and state-service flushes.
- [ ] Keep ADR documentation current for every architecture-level decision.
- [ ] Maintain workspace-wide compile and test health after each milestone.

### Out of Scope

- Rewriting the node from scratch — this project is a deep brownfield refactor, not a greenfield rebuild.
- Replacing Neo protocol semantics with reth or Polkadot semantics — external projects are references for engineering structure, not sources of consensus rules.
- Changing consensus-critical block acceptance behavior without parity evidence — correctness beats elegance.
- Moving stateful validation into concurrent import-queue preverification — canonical validation must stay in the ordered command loop.
- Large execution/persistence/commit rewrites before the validation pipeline is safely wired and tested — avoid breaking durable state boundaries.
- Cosmetic crate reshuffling without measurable verification, safety, or maintainability gain.

## Context

The current architecture has already gone through several repair iterations. The workspace uses a layered Rust architecture with 28 crates, domain-specific errors, provider-trait decoupling, sealed composition traits, staged pipeline scaffolding, and ADR-backed design decisions. `design.md` is the source of truth and currently reports architecture health around 9.4/10 before this new GSD project initialization.

Recent completed work includes:

- Splitting the `Store` god trait into focused capability traits.
- Sealing selected runtime composition traits.
- Removing dead `neo_execution::KeyBuilder` wrappers.
- Introducing canonical `neo_primitives::hex_util` and removing duplicated hex handling.
- Cleaning native-contract key-building APIs.
- Extracting `neo-blockchain::pipeline::validate_stage::NeoValidateStage` with focused tests.

The next high-leverage architecture move is not to create more abstractions on paper, but to connect the validation stage to the real production block import path with explicit policy boundaries. This must follow blockchain-node best practice: cheap/stateless preflight may run earlier, but stateful canonical validation belongs in the ordered import loop where tip, snapshot, and commit boundaries are consistent.

## Constraints

- **Protocol compatibility**: Neo C# parity is mandatory — refactors must not silently change consensus-critical acceptance rules.
- **Validation policy boundary**: `MaxTransactionsPerBlock` is a dBFT primary-side block production limit, not a generic peer import hard validity gate.
- **Consensus verification**: Existing header verification and witness verification remain authoritative until an equivalent, proven replacement exists.
- **Mutation ownership**: Canonical chain mutation stays inside the `BlockchainService` command loop; concurrent queues must not own stateful validation decisions.
- **Commit safety**: Execution, persistence, ledger hot-cache updates, store commits, state-service flushes, and bulk finalization ordering must not be changed casually.
- **Verification discipline**: Each milestone must pass relevant crate tests and workspace test compilation before being considered complete.
- **Documentation discipline**: Architecture decisions must be captured in `design.md` through ADR updates or new ADRs.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Use reth and Polkadot/Substrate as architecture references, not protocol authorities | They provide proven node-engineering patterns, but Neo C# defines Neo consensus behavior | ✓ Good |
| Prioritize policy-aware live import validation as the next milestone | It connects an existing stage abstraction to the real node path while keeping risk bounded | — Pending |
| Keep consensus witness verification in the existing header verification path | `NeoValidateStage` currently does not replace `neo_execution::Helper::verify_witness` semantics | — Pending |
| Keep stateful validation out of `BlockImportQueue::check` | Import queues can preverify cheaply, but they do not have ordered canonical tip/snapshot context | ✓ Good |
| Require ADR updates for each architecture-level refactor | Prevents architecture drift and preserves the reasoning behind trade-offs | ✓ Good |

---
*Last updated: 2026-07-04 after GSD initialization from architecture refactor goals*
