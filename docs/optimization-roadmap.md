# Neo-RS Node Optimization Tech-Stack Roadmap

Living document for the node performance campaign. Purpose: one place that
maps measured hotspots to the optimization techniques that attack them, with
each technique's status, evidence, and next step. Update it whenever an
experiment lands or is rejected. Correctness is invariant everywhere: every
change must keep MainNet state roots byte-identical (verified by strict
shadow replay + seed root parity), and every verdict needs machine-readable
evidence under `reports/performance/`.

## 1. Measured Baselines (2026-07, Intel Ultra 9 285K VM, 8 vCPU, 62 GB, ext4 VMware disk)

| Regime | Throughput | Binding constraint |
|---|---:|---|
| Light heights 100k-300k, tmpfs, uncoordinated pruning | ~11.5k blocks/s overall; dense ~1.6k; empty ~50k | VM execute (`load_execute` ~318µs/tx) |
| Coordinated full-state durable, heights 1.9M-2.0M, ~50 GB MDBX | 24-70 blocks/s | **Finalization = 86-90% of wall time** |
| Strict-shadow validation (adds artifact comparison) | 27-41 blocks/s | Same finalization hotspots + shadow capture |
| Strict-shadow full-state durable, heights 3.247M-3.267M, 116 GB MDBX | 50.7-51.8 blocks/s overall; 262-325 transaction blocks/s; 35-37k empty blocks/s | **Finalization = 94.7-95.6% of wall; cursor resolve = 77.9-79.4%** |

The dominant campaign target is the second regime: sustained full-state
MainNet sync. Finalization splits into:

- **Deferred MPT finalization lookup** — 118-173µs/probe, 120-160 s per 10k
  blocks. Serial cold-page-fault sweep of ~131k content-addressed keys per
  co-commit window to recover C# reference counts.
- **MDBX commit windows** — ~100-120 s per 10k blocks: overlay visit ~69 s
  (incl. ~21µs/entry sink bookkeeping), cursor write ~46 s, durable
  commit ~47 s; ~1.1M puts / ~249 MB per 10k blocks.
- VM execution is *not* the bottleneck here (~10-30 s per 10k blocks).

## 2. Tech-Stack Map by Layer

### 2.1 Storage engine (highest leverage)

| Technique | Status | Evidence / next step |
|---|---|---|
| Fused MPT reference resolution at the commit cursor (one B-tree descent per key instead of two sweeps) | **DONE 2026-07-17** — deferred lookup 145-233 s → 0.0 s; light windows 2.4-3.0x (up to 95.8 blocks/s), root parity MATCH | `reports/performance/ACCEPTED-fused-cursor-resolution.md` |
| Ordered deferred-journal cursor (bounded forward walk with sparse-key fallback) | **OPT-IN 2026-07-20** — exact root/reopen parity; genesis h1k cursor-resolve 5.742 ms → 4.610 ms (19.7%) | `reports/performance/cursor-resolution-forward-ab-h1k-20260720.md`; keep `NEO_MDBX_CURSOR_RESOLUTION_MODE=search` by default and require high-height A/B before promotion |
| Commit-window cost at high tx density (cursor read-modify-write + ~21µs/entry overlay-visit bookkeeping) | **NOW TOP HOTSPOT** (438 s/10k at ~17k txs: 356 s journal resolution at cursor, 58.5 s durable commit, 24 s cursor write, ~23 s bookkeeping) | Split read_stored vs write_stored with new counters, then attack the dominant side; sorted keys give no locality (uniform hashes, ~7.6k rows between adjacent probes) |
| Append-frame persistence prototype (write amp 142x→1.39x, 29x campaign write speed, 12k blocks/s synthetic) | Prototype passes bakeoff gate; **not** production-shaped: newest-run linear search makes reads/restart 100-700x slower | `reports/performance/append-persistence-1m-comparison-20260717.md`. Next: bounded verified lookup hierarchy (filters, leveled compaction, mmap top index, snapshot leases, recovery marker, crash injection), then shadow-mode integration |
| Coordinated dual-namespace co-commit (Ledger + StateService in one MDBX txn) | DONE, in use | `neo-storage/src/mdbx/store.rs` `commit_coordinated_overlays` |
| Deferred full-state finalization (journal per window, resolve at end) | DONE (opt-in default-on for campaign) | `reports/performance/fullstate-deferred-finalization-ab.md` |
| Coordinated change budget 16,384 | DONE (+6.9%) | `mdbx-budget-ab-1300000-1330000.md`; 32,768 and unbounded REJECTED |
| MPT negative cache for proven-absent keys | DONE (kept as IO hygiene; perf-neutral) | `session-20260715/SUMMARY.md` |
| Parallel write-intent readers (8) at high height | **REJECTED** (173 vs 118-153µs/entry; disk-bound contention) | `REJECTED-write-intent-readers-8-highheight.md` |
| Cross-batch MPT node cache | **REJECTED** (zero hits — content-addressed keys are always new) | `mdbx-catchup-experiments-1781k-1817k.md` |
| Prefix-occupancy bitmap on write-intent reads | **REJECTED** (cost moved to cursor writes 0.39→8.51 s) | `prefix-occupancy-write-intent-ab.md` |
| Merge-cursor / adaptive merge writes | **REJECTED** (sparse keys) | `mdbx-cursor-merge-ab.md` |
| no-meta-sync / safe-no-sync / no_meminit | **REJECTED** (non-durable) | `mdbx-catchup-experiments-1781k-1817k.md` |
| Prefetch overlap: pre-fault journaled leaf pages during the mutation phase | SUPERSEDED — fused cursor removed the separate read sweep; remaining cost is inherent write-descent + fault latency | — |
| Bloom/presence filter to skip journal probes | **REJECTED by analysis** (analytic, no code) | Absent-key UPSERT re-faults the same leaf the probe just faulted, so the filter saves only CPU descents (~30-60µs), not the 150-270µs fault; filter for ~5B hashes costs GBs |
| **Async co-commit overlap** (background RW txn commit of window N overlapping VM execution of N+1) | DEPRIORITIZED 2026-07-17 — measured only ~6% of wall (execution is 6% of wall; commit is 9-27x execution, so overlap can hide at most execution time) | Re-evaluate only after per-entry commit cost falls ~5-10x (append-frame engine) |
| **Append-frame storage engine** (sequential frames + derived indexes; write amp 142x→1.39x, 29x write speed proven) | **TOP PRIORITY** — cursor resolution alone is now 77.9-80.2% of high-height wall. Phase 0 read-path/compaction prototype passed. Phase 1 now has marker-bounded recovery, bounded streaming verification, cold-first two-phase publication (durable prepare -> MDBX marker -> manifest activation), a kernel-held cross-process writer/recovery lease, and an offline O(frame)-memory checkpoint builder. Three strict MainNet checkpoints through 3,277,022 retained exact roots and sampled pack bytes; a 1,000-row real-MDBX checkpoint smoke reopened and matched 1,000/1,000 exact values. | Still shadow-only. Add complete footer/segment identity and hard limits; run and verify an uncapped complete base checkpoint; finish the `0xf0` node/non-node metadata commit split; then run authoritative same-window A/B requiring zero MDBX `0xf0` reads/writes. `openspec/changes/append-only-mpt-node-packs/`, `mainnet-shadow-observed-3267022-3277022.md`, `pack-checkpoint-smoke-3277022-20260718.md` |
| **Pack-store fixed-cardinality metrics** (append/sync/index stage totals, point/batch reads, layout, compaction debt) | **DIAGNOSTIC API 2026-07-20** — shared by live stores and pinned snapshots; no protocol or visibility change | `neo-state-packs::PackStore::metrics()`; no end-to-end throughput delta claimed. Wire into authoritative node telemetry after the storage authority gate passes |
| Overlay-visit sink bookkeeping (~21µs/entry ≈ 23 s/10k) | OPEN | Profile `neo-storage/src/mdbx/store.rs:1330-1450` per-entry work: allocations, sort (`visit_raw_overlay`), metrics sampling; consider pre-sorted invariant or buffer reuse |

### 2.2 MPT / state service

| Technique | Status | Evidence / next step |
|---|---|---|
| Per-block projection + windowed apply (16,384-change budget, 8 windows/10k blocks) | DONE | `commit_handlers.rs:352-448` |
| Root-hash/trie-commit pipeline metrics (per-stage µs) | DONE | `state_service_mpt_*` metric family |
| Trie commit (~0.8 s/window) and backing sort | Low priority (small) | Revisit after storage layer |
| Fail-closed journal checks (visited + journal_committed_at_cursor) | DONE 2026-07-17 | `mpt_store.rs` `publish_prepared_coordinated` |

### 2.3 Execution / VM

| Technique | Status | Evidence / next step |
|---|---|---|
| VM jump-table hot path (stash restore) | DONE (~11.5k, 317µs) | commit `e8cc7239` |
| Lock-free call-flag checks + shared implicit RET | DONE | commits `e06bcec1`, `23f852ad` |
| ApplicationEngine reuse across multi-tx blocks | DONE | commit `056bd1a1`; OnPersist→PostPersist reuse REJECTED (neutral) |
| Warm contract/script cache | DONE (+~2%) | `session-20260715/SUMMARY.md` |
| FxHashMap for DataCache + script maps | DONE | commit `f4477115`; engine-internal FxHash REJECTED |
| Eager script/instruction pre-parse | **REJECTED** (neutral) | `session-20260715/REJECTED-eager-*.md` |
| **Parallel transaction execution** (speculative, read/write-set conflict detection, deterministic re-execution) | IN PROGRESS — pinned block-prefix and detached transaction overlays exist; opt-in capture records bounded exact present/absent `PinnedPrefix` point reads, ignores own-overlay reads, rejects ranges/whole-store fail-closed, and deterministically revalidates the first changed/created/deleted key in canonical key order | Next capture context and native-cache dependencies, bind them into a complete transaction-owned artifact, then validate and apply strictly in transaction order with deterministic sequential replay on conflicts. OnPersist, Ledger VM-state writes, PostPersist, roots, and durability remain sequential authority. |
| Engine/cache object reuse | Existing transaction cache and `ApplicationEngine` reuse retained; new pooling DEPRIORITIZED by current profile | Engine construction was only 52-95 ms per 10k-block window (0.03-0.05% wall). Require allocator evidence before adding pool complexity. |
| Multi-level contract/script/plan cache | Immutable method metadata, script caches, lazy instruction caches, and bounded execution-plan cache exist | Never cache stateful outputs. Add or enlarge a cache only when exact identity/version dependencies and a measured miss hotspot justify it. |
| Verified script/contract short paths | Guarded plan executor and one candidate specialization exist, disabled by default and ordinary-authoritative Shadow gated | Continue trace-selected candidates only; exact script bytes, hardfork/update identity, gas/fault/stack/calls/storage/events/diagnostics must match workspace `neo-vm`. |
| Specialized candidate paths (Flamingo factory pair key v1) | Shadow-gated evidence gathering | `neo-execution/src/application_engine/shadow.rs`; promotion gate: bounded MainNet shadow windows |

### 2.4 Sync / import pipeline

| Technique | Status | Evidence / next step |
|---|---|---|
| Empty-block fast path | DONE (31-50k blocks/s) | import metrics `empty_blocks_per_second` |
| Constant-time initialized-chain startup probe | DONE 2026-07-18; 200.09 s -> 57.6 ms at height 3,247,022 | NeoToken committee point read first; legacy partial-store prefix scan remains fallback. Regression forbids the normal full-prefix materialization. |
| chain.acc index positioning (resume without rescan) | DONE | `neo-blockchain/src/handlers/import.rs` |
| Bounded optimistic header-witness verification pool | **OPT-IN FOUNDATION 2026-07-20** — typed receipts, ordered publication fence, synchronous fallback, fixed-cardinality counters; no end-to-end node speedup established | `neo-blockchain::pipeline::signature_verification`; `reports/performance/optimistic-signature-verification-20260720.md`; complete node wiring remains coupled to the ChainSpec migration |
| Checkpoint fast sync (authenticated state bootstrap + canonical catch-up) | Phase 6 of v1.0 milestone (`.planning/ROADMAP.md`) | Depends on Phase 5 full replay proof |
| Live P2P catch-up after import | In use | environment-dependent throughput |
| Block download/verify pipelining ahead of persist | OPEN | Persist is the bottleneck; verify-ahead only pays after storage work lands |

### 2.5 Validation & profiling infrastructure

| Tool | Status | Notes |
|---|---|---|
| Strict ordinary-authoritative shadow replay (candidate vs ordinary artifact comparison) | DONE | `shadow.rs`; `allow_artifact_overflow` gate + configurable artifact bounds added 2026-07-17 for >64 MiB pathological txs |
| march.sh: automated window march + seed-root parity probes | DONE | `data/neo-v3101-staged-replay/march.sh`; do not edit while running |
| Stage-level metrics (import/finalize/MPT/MDBX/VM) | DONE | emitted in `chain.acc import progress/complete` JSON |
| `perf` / gdb sampling | **BLOCKED** on this host (`perf_event_paranoid=4`, `ptrace_scope=1`) | Need sysctl change (root) or run node under a tracer from launch |
| A/B experiment protocol | Established | Same binary+config+store, adjacent windows; record verdict + evidence in `reports/performance/*.md`; rejections documented as `REJECTED-*.md` |

## 3. Ranked Forward Queue

1. ~~Land fused cursor resolution~~ **DONE** (see §2.1). New top hotspot:
   **commit-window per-entry cost** (cursor resolve 77.9-79.4% of current wall)
   — attack via the authoritative append-frame vertical slice, not incremental
   MDBX tuning.
2. ~~Async co-commit overlap~~ **DEPRIORITIZED** (~6% of wall; revisit after
   the append engine lands).
3. **Shadow-twin overhead** (~2% of wall): candidate per-instruction route
   checks + double artifact capture; reducible without weakening the proof.
4. **Parallel speculative transaction execution** — dependency-capture work may
   proceed in parallel, but scheduler/promotion follows the sequential pack
   gate. VM execution is currently 3.3-4.2% of wall, yet it must also fall below
   5 seconds per 10k blocks before a 2,000 blocks/s claim is possible.
5. **Checkpoint fast sync** — milestone Phase 6; becomes the main UX win after replay correctness is fully proven.

## 4. Standing Rules

- No optimization lands without: strict-shadow window(s) with zero mismatch, seed root parity at probe heights, and a written A/B record.
- Neutral or negative results are recorded as `REJECTED-*.md` with data, and the default stays unchanged.
- Durability flags (sync modes) are never weakened for speed.
- The shadow/harness memory guards (`ExecutionArtifactLimits`) are validation tools, not protocol limits; sizing them is always allowed, bypassing them silently never is.
