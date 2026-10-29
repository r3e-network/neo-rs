# Neo N3 Protocol Consistency Verification

## Overview

**Goal:** make neo-rs ↔ official C# Neo N3 **state-equivalent** in a way that is
**machine-verifiable**, not marketing language.

A claim of “100% protocol consistency” is allowed **only** when all of the
following are true for the claimed height range `[0, H]`:

1. Rust node (or offline replay) produces a **state root** at every height `h ∈ [0, H]`.
2. That root **byte-for-byte equals** the C# reference root for the same `h`
   (from a trusted C# node / seed / committed golden file).
3. The comparison is **fail-closed**: any mismatch, missing local root, or
   unreachable reference for a required height is a hard failure (non-zero exit).
4. Evidence is **reproducible** (script + inputs + recorded report artifact).

Until that holds through tip (or a declared `H`), do **not** claim full MainNet
protocol equivalence. Partial verified ranges may be claimed only as
`verified contiguous [0, H]` with the report attached.

## Why state roots

If two honest Neo N3 nodes execute the same blocks and disagree on
`getstateroot(h)`, their contract storage / MPT diverged. Peers will disagree on
application state even if block hashes still match. State-root equality is the
authoritative cross-implementation consistency signal.

## Verification ladder

| Level | What it proves | Gate |
|-------|----------------|------|
| L0 | Serialization / hashes / vectors | Always-on unit/CI tests |
| L1 | Real MainNet block bytes → header/merkle/hash | `protocol_compliance` CI |
| L2 | Dense C# golden roots committed + RPC/DB compare tooling | Scripts + fixtures |
| L3 | Contiguous Rust↔C# roots for `[0, H]` with zero mismatches | Operator / nightly with synced DB |
| L4 | Continuous tip follow vs C# seeds, mismatch halts promotion | `continuous-stateroot-validation.py` |

**100% claim** ≡ L3 with `H = tip` (or L4 with verified contiguous history from genesis and no open divergence).

## RPC limiter + two-lane design (caveat)

The local `neo-rpc` server rate-limits JSON-RPC **per IP**, including loopback.
`getstateroot` sits in the **Standard** tier: **100 req/s, burst 200**. The
limiter lives in `neo-rpc` (`handlers.rs` `process_object` → `rate_limiter.rs`)
and its keys are **not** emitted into the generated
`data/Plugins/RpcServer/RpcServer.json` (which the node truncate-rewrites on
every start anyway) and are **not** exposed via `[rpc]`/`RpcSettings`. So the
limit is effectively fixed in code; there is no config/JSON workaround.

Consequences — do **not** conclude the RPC lane is "broken":

* A **dense** sweep via RPC is infeasible: 13.14M heights at ~100 req/s ≈ 36 h
  just for the local leg. Extra workers only produce `-32001 "Too many
  requests"`, they do not add throughput.
* `max_concurrent_connections` is **40** (generated `RpcServer.json`), so any
  keep-alive client must use **≤ 40** connections.

Resulting architecture:

| Lane | Mechanism | Constraints |
|------|-----------|-------------|
| **Dense** | **In-import gate**: importer runs with `data/reference_stateroots.jsonl` present; `neo::state_service` compares the computed root to the C# reference at **every** executed height in-process and aborts at the first divergence. Not rate-limited. | Needs a complete reference file (precondition gate). Expect an abort at a known divergence. |
| **Sampled** | **RPC** `getstateroot` vs live C# seeds. | **stride ≥ 100**, `--parallel ≤ 40`. A sampled run never produces a "verified contiguous" claim. |

Runner: `scripts/mainnet-full-validation.sh campaign` (dense) and `… verify`
(sampled); reports under `outputs/` via `scripts/mainnet_gate_report.py`. A
dense run that aborts exits non-zero and writes only `.campaign-aborted`; a
success marker is written **only** on a clean full pass. Known expected-stop
heights: 5107 (`CommitteeInfoContract.setInfo`), 21373 (GasToken fee-burn),
980196, 1465790.

## API / tooling reference

| Tool | Role |
|------|------|
| `scripts/download_stateroots.py` | Fetch C# `getstateroot` → `data/reference_stateroots.jsonl` |
| `scripts/compare-local-csharp-rust-stateroots.py` | Batch RPC compare local Rust vs C# |
| `scripts/continuous-stateroot-validation.py` | Fail-closed continuous compare while syncing |
| `scripts/verify-protocol-consistency.py` | Orchestrator: golden refresh, compare, report |
| `neo-core/tests/mainnet_state_roots_vs_csharp.rs` | Bulk DB vs jsonl (ignored without data) |
| `neo-core/tests/mainnet_block_*_repro.rs` | Single-height divergence reproducers |

### Environment

- `NEO_RUST_RPC` — local neo-rs RPC (default `http://127.0.0.1:10332`)
- `NEO_CSHARP_RPC` — C# reference (default `http://seed1.neo.org:10332`)
- `NEO_BULK_ROOT_DB_PATH` / `NEO_BULK_ROOT_MAX_HEIGHT` — bulk DB test overrides

## Design

1. **C# is oracle** for MainNet state roots (official seeds / self-hosted C# node).
2. **Never skip heights** in a claimed contiguous range.
3. **Known-bad local DB** must not be used to greenwash: if storage is stale,
   re-sync or cap `H` explicitly in the report.
4. Divergence handling: stop sync promotion → open reproducer → fix → re-verify
   contiguous range from last good height.

## Behaviour lane (application logs)

State roots cover every storage write, but not execution output that never reaches
storage: notifications, result stacks, exception text. The behaviour lane compares
`getapplicationlog` for the block (OnPersist/PostPersist) and every transaction.

```bash
python scripts/verify-protocol-consistency.py compare-applogs \
  --local http://127.0.0.1:10332 --start 0 --end 200000 --stride 1
```

- Compared per execution: `trigger`, `vmstate`, `exception`, `gasconsumed`,
  `stack`, `notifications` (key case and hex case normalised).
- Fail-closed: any difference, any log missing on either side, or zero compared
  items fails. A height whose fetches failed is retried; a field mismatch
  stops the run on the first attempt. `--collect-all` keeps scanning.
- Each finished height is appended to
  `docs/protocol-consistency/reports/compare-applogs-<start>-<end>-s<stride>.jsonl`.
  A later run of the same range skips heights already marked ok and rechecks
  failures. The JSON report passes only when that journal covers every height.
- The Rust side needs ApplicationLogs enabled. `--import-acc` registers
  ApplicationLogs and TokensTracker before importing (as neo-cli does).
  Import runs in fast-sync mode, which skips committing handlers unless they
  opt in. `ApplicationLogsService` opts in, and each import checkpoint flushes
  its store, because the import RocksDB profile disables the write-ahead log.
- Tests: `scripts/tests/test_verify_protocol_consistency.py`.

## Runtime divergence gate (live nodes)

Replay campaigns prove history; a running node has no reference file. Its live
oracle is the StateValidator-signed `StateRoot` broadcast by the network.

**Behaviour.** When `StateStore::on_new_state_root` receives a root whose witness
verifies against the designated StateValidators at that height, and the local
root for the same height has a different hash, the node:

1. persists a divergence record (state store key `0x05`: height, local root,
   validated root) so a restart cannot clear it;
2. logs at `error` and counts it in `state_root_divergences` metrics;
3. refuses every later local state-root update, so block persistence stops with
   `state root divergence at height H ...` instead of building more state on a
   fork that the network does not agree with.

C# StateService silently rejects such a root and keeps going. Halting is
deliberately stricter: a node whose state differs from a validator-signed root
must not keep serving or extending that state.

**API.** `StateStore::state_root_divergence() -> Option<StateRootDivergence>`
returns the recorded divergence. Unsigned roots, roots failing witness
verification, and roots for heights without a local root never trigger the gate.

**Recovery.** Investigate the height with a reproducer, fix, then resync the
state store (the record lives in it).

**Tests.** `state_store::tests::validated_root_mismatch_halts_local_root_updates`
covers the record, its persistence across a reopened store, and the refusal of
further local updates; `verifies_state_root_witness_against_designated_state_validators`
covers the matching path.

## Usage examples

```bash
# Refresh committed golden checkpoints (small, CI-friendly)
python scripts/verify-protocol-consistency.py refresh-goldens

# Compare a running neo-rs node to C# for a contiguous range (fail-closed)
python scripts/verify-protocol-consistency.py compare-rpc \
  --local http://127.0.0.1:10332 \
  --reference http://seed1.neo.org:10332 \
  --start 0 --end 10000

# Continuous validation while syncing (halts reporting on first mismatch)
python scripts/continuous-stateroot-validation.py --once \
  --local http://127.0.0.1:10332 \
  --reference http://seed1.neo.org:10332
```

## Test coverage / evidence

- L0/L1: existing `neo-core` protocol compliance tests in CI.
- L2: `tests/fixtures/protocol_consistency/csharp_stateroot_goldens.jsonl` + orchestrator.
- L3/L4: require a synced neo-rs with StateService; reports under
  `docs/protocol-consistency/reports/`.

## Current status (authoritative)

See `docs/protocol-consistency/STATUS.md`. Partial historical windows were
validated in past plans; **tip-level equivalence is not currently proven**, and
known single-height divergences remain documented in `mainnet_block_*_repro.rs`
comments until fixed and re-verified.
