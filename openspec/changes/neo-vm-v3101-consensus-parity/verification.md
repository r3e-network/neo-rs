# Verification Record

**Captured:** 2026-07-14
**Scope:** Task 4.2 source, dependency, consistency, and specification gates

## Passed Gates

| Gate | Result |
|---|---|
| `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | 349 passed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --profile test --locked -- -D warnings` | passed |
| `cargo test --workspace --all-targets --profile test --locked` | passed |
| `cargo +1.89.0 check --manifest-path fuzz/Cargo.toml --locked --all-targets` | passed |
| Root and fuzz `cargo deny --locked check advisories bans licenses sources` | passed with informational policy warnings |
| `actionlint -no-color` and validator shell syntax | passed |
| `openspec validate neo-vm-v3101-consensus-parity --strict` | passed |
| `git diff --check` | passed |

Cargo metadata resolves only the workspace `neo-vm` package. An active-source
scan found no `StackValue`, runtime graph-conversion API, or external VM
dependency; the only `neo-vm-rs` text is the required provenance notice.

The pinned execution-spec corpus also completed against a fresh local
MainNet-configured node with 405 of 405 vectors passing. The retained local
report is `reports/compat-v310/20260714T014127Z/mainnet/neo-rs-vectors.json`.
This is Rust-versus-pinned-execution-spec evidence, not a recorded C# oracle
fixture and not hardfork-table or MainNet replay evidence.

## Recorded C# Differential Fixtures

Two checked-in C# generators execute against the immutable upstream projects
and record 32 observations:

| Fixture | Oracle | Cases | SHA-256 |
|---|---|---:|---|
| `neo-vm/tests/fixtures/csharp-v3.10.1-vm.json` | Neo.VM `004cd6070a940405818d9357638277dd44407e2e` | 23 | `1b476d388c16f1272744ef95d2e319302f7826da65c18126b46b42ba670ec89a` |
| `neo-execution/tests/fixtures/csharp-v3.10.1-application.json` | Neo `d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d` | 9 | `0887b41a4aae2507e6892b2d797aa32d7b3bfdceebe77263d0105156a3d13b9e` |

The case set covers exact implicit return counts, relaxed and strict parsing,
runtime script loading before and after Basilisk, inclusive context bounds,
strict jump bounds, `CALL`, `TRY`, and `ENDTRY` targets, notification cleanup,
null conversion, struct packing, slot mutation order, unhandled throws, strict
UTF-8, and pre-Echidna, Echidna-to-Gorgon, and post-Gorgon jump tables.

Fresh .NET 10.0.100 runs against the pinned checkouts were compared with
`scripts/oracles/v3101/verify-recorded.py`; all 23 Neo.VM and 9 Neo cases
matched. `scripts.tests.test_v3101_oracle_fixtures` locks the exact case sets,
provenance, generator paths, hardfork annotations, and drift rejection.

## Staged MainNet Replay: Height 100,000

An isolated production-path replay reached MainNet height 100,000 from genesis
through the built-in fast-sync package importer. The official package covered
heights 0 through 11,492,708, had MD5
`4C6F26CF56882E9E54AAC834E4FFFF69`, and matched that manifest checksum after
download. The extracted `chain.0.acc` was 10,031,375,631 bytes.

The post-run coordinated MDBX probes reported Ledger height 100,000 and
StateService height 100,000. The local state root
`0x649af63171ab3112de3b44031b25555cd8c4eaa0821084cf65183645f6cbd15c`
matched both configured independent reference RPCs. All 100,000 StateService
MPT applications completed and the failure count was zero.

This run did not pass the production-speed or checkpoint gates. Its importer
processed 100,000 blocks at 1,630.74 blocks/s, including 2,238
transaction-bearing blocks and 2,742 transactions. Transaction-bearing
processing measured 1,044.85 blocks/s against the required 1,500 blocks/s
floor. The final atomic MDBX store commit took 23.60 seconds; total bounded-run
throughput was 957.76 blocks/s. The milestone runner therefore returned
`sync-speed-too-slow` and intentionally created no checkpoint.

The validation-host artifacts are:

| Artifact | Size | SHA-256 |
|---|---:|---|
| `/tmp/neo-rs-mainnet-stage/staged-result.json` | 174,487 bytes | `26bf8b07001fc64db56359356b99ce7d469a9bb264946d98af32e87ac9a47408` |
| `/tmp/neo-rs-mainnet-stage/logs/neo-node-milestone-h100000.log` | 28,112 bytes | `f226aba1f1329943953e43a0f3091db98ec8672f2e2c69ec436d74c2f1720862` |
| `/tmp/neo-rs-mainnet-stage/fast-sync-cache/chain.0.acc.zip` | 5,389,186,649 bytes | `c6ef5e0a63fa79e4f5d0698e0c5f62f247a96ee00427b69f40ecdfe22ff4ed85` |

This is retained evidence of correctness through height 100,000 and a retained
performance failure, not completion evidence for task 4.4. Heights 4,119,999
and 4,220,000, at least three restore-verified full-state checkpoints, and a
passing transaction-bearing speed proof remain required.

## Storage Profiling And Optimization Evidence

The cursor reuse and snapshot table-cache A/B, plus a warm two-window replay
from heights 821,000 through 841,000, are retained in
[`reports/performance/mainnet-sync-20260714.md`](../../../reports/performance/mainnet-sync-20260714.md).
The optimized run ended at height 841,000 with root
`0x8554b783e78604e3f6c3afabde84d00be3005f6e42fc2b3b22b3f04ef2548189`.
That root matched independent seed1 and seed2 `getstateroot` responses. The
snapshot table cache reduced the 10,000-block A/B import from 203.343336 to
177.632684 seconds (12.6%), while preserving zero MPT failures and the exact
height-821,000 root. Exact per-window stage totals are emitted in structured
progress logs and Prometheus counters; the transaction-bearing speed gate is
still intentionally unsatisfied.

The subsequent pruning-mode MPT campaign is recorded in the same performance
report. Seventy runs from 24 through 93 reached Ledger and StateService height
300,000 with zero MPT failures and the official root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.
The final retained five-run candidate averaged 1,509.38 blocks/s, but its
one-sided 95% lower confidence bound was 1,485.33 blocks/s. Later runs were
materially contaminated by unrelated compiler and fuzz workloads. These are
tmpfs, pruning-mode, archive-import measurements; they do not prove sustained
production throughput or satisfy staged/full MainNet replay tasks 4.4/4.5.

### Adaptive Ordered MDBX MPT Reads (2026-07-16)

Deferred MPT finalization now has an ordered raw-read surface. MDBX walks a
single cursor while sorted keys remain local and falls back to the existing
seek-per-key path after 64 forward steps, bounding sparse absent-key work. The
candidate replay from the full-state height-100,000 checkpoint through height
300,000 completed with zero MPT failures and matched the official root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088` (also
matched by `seed1.neo.org:10332 getstateroot(300000)`). Its final
290,001-300,000 window measured 1,764.43 blocks/s and 1,987.90
transaction-bearing blocks/s, with 805,945 finalization backing misses. The
raw log and SHA-256 are retained in
[`reports/performance/ordered-mpt-ab.md`](../../../reports/performance/ordered-mpt-ab.md).

This is a single tmpfs, CPU-pinned candidate run. It demonstrates the target
band for this replay window but is not a sustained multi-run durable-disk or
live-P2P proof.

### Full-State Deferred Finalization A/B (2026-07-16)

An eager-control versus explicit-opt-in A/B replayed heights 1,001 through
30,000 from the verified height-1,000 checkpoint. Both variants reached height
30,000 with zero MPT failures and matched the official root
`0x61062a078463b88fa18eac86722dd8d3faf50a6ec2ee8e5e70c06c16546a9347` and
`seed1.neo.org:10332 getstateroot(30000)`. Reopened probes at five historical
heights returned identical roots and values; the complete final storage dump
also matched.

The direct `neo-db-probe --mpt-dump-root 30000 --dump-limit 1000` comparison
found 221 non-truncated leaves in both snapshots. Canonicalizing the complete
`mpt_root_storage` JSON (excluding the database path) produced the identical
SHA-256 `d0a383c313005636f2284ae266f477f1e91a0292faa9af3b3343301a3adc2ed2`.
The exact deferred replay log, release node, and release database probe were
hashed after the comparison:

| Artifact | SHA-256 |
|---|---|
| `fullstate-exact-h1k-30k.log` | `5ff49b0236954e13174f73151a5fcce2fbcc5979ced8b60bdba3f3d8c1a17184` |
| `target/release/neo-node` | `3dd01bf225987d7c466f3f7d85ed023ad5f8b8ff79f7a9a98c6107a2a8e84143` |
| `target/release/neo-db-probe` | `12465b86696c404c6a9a88c368b0fddf7ad4626e0346f9f537b319ead16ebe96` |

The opt-in `[state_service].defer_full_state_finalization = true` candidate
reduced wall time from 6.10 s to 4.07 s and the final 9,000-block MPT apply
from 2.452 s to 1.386 s. It records every serialized mutation and the focused
durable-overlay test matches the complete eager raw namespace and reachable
reference bytes. The default remains eager and C#-compatible, and the
candidate is not used to close replay tasks 4.4 or 4.5. Full details and hashes
are in
[`reports/performance/fullstate-deferred-finalization-ab.md`](../../../reports/performance/fullstate-deferred-finalization-ab.md).

### Prefix Occupancy Write Intent (2026-07-16)

The 30-bit occupancy bitmap was tested against deferred full-state finalization
and rejected for the write-coupled lookup path. Filtering definite misses
reduced read work, but it removed the MDBX page warming provided by those
authoritative probes and increased cursor writes from 0.39 seconds to 8.51
seconds on a 1,000-block sample. A new explicit
`try_get_many_bytes_sorted_for_write` storage intent keeps the filter for
ordinary reads while forcing authoritative ordered probes before the MPT
overlay commit. A follow-up 1,000-block run reached height 813,000 with the
official root and restored cursor writes to 1.22 seconds. Full details and
hashes are in
[`reports/performance/prefix-occupancy-write-intent-ab.md`](../../../reports/performance/prefix-occupancy-write-intent-ab.md).

### Coordinated Full-State Commit Budget A/B (2026-07-16)

An explicit `NEO_COORDINATED_IMPORT_CHANGE_BUDGET=0` stress run was compared
with the default 8,192 projected-change bound over heights 811,001-821,000.
Both variants matched the official height-821,000 root with zero MPT failures.
The single 670,552-entry MDBX transaction reduced overall throughput to 123.6
blocks/s and expanded cursor traversal to 48.59 seconds, while the bounded
default measured 210.7 blocks/s with nine commits. The unbounded candidate is
rejected; details are in
[`reports/performance/fullstate-coordinated-budget-ab.md`](../../../reports/performance/fullstate-coordinated-budget-ab.md).

The existing 12,288-change async bound was tested as a coordinated candidate
in two additional durable runs. Both used six commits, matched the official
height-821,000 root, and had zero MPT failures, but measured 212.9/907.1 and
204.2/833.0 overall/transaction-bearing blocks/s. Those low-height runs were
not sufficient to change the default at the time; the 8,192 value remains a
historical control and is still available through the environment override.

### High-Height Coordinated Commit-Budget A/B (2026-07-16)

Fresh clones of the verified height-1,300,000 full-state database replayed
heights 1,300,001 through 1,330,000 with durable `NoWriteMap` MDBX and sixteen
write-intent readers. Both variants reached the official root
`0x10875b33bbdcdd1e90271ba871e213f48e34f3c390579110eb0daf3751f58aa1`,
matched `seed1.neo.org:10332`, and reopened with zero MPT failures.

The measured 16,384 projected-change bound reduced MDBX commits from 11 to 6,
finalization from 226.43 s to 210.64 s, and import time from 243.17 s to
226.32 s. Overall throughput rose from 123.37 to 132.55 blocks/s, and
transaction-bearing throughput rose from 910.87 to 969.34 blocks/s. This
root-matching high-height evidence promotes 16,384 to the default bounded
catch-up budget; `NEO_COORDINATED_IMPORT_CHANGE_BUDGET=8192` remains a
reproducible control and `0` remains an explicit unbounded opt-out.

Full artifacts and exact structured metrics are in
[`reports/performance/mdbx-budget-ab-1300000-1330000.md`](../../../reports/performance/mdbx-budget-ab-1300000-1330000.md).

A same-range 32,768-change candidate was then tested over heights 1,400,001
through 1,420,000. It matched the height-1,420,000 root and reopened without
MPT failures, but slowed import from 104.80 s to 119.40 s and increased cursor
work from 5.50 s to 7.92 s. The larger bound is rejected; the exact comparison
is recorded in
[`reports/performance/mdbx-budget-ab-1400000-1420000.md`](../../../reports/performance/mdbx-budget-ab-1400000-1420000.md).

### Continued Durable MainNet Replay (2026-07-16)

The retained full-state database continued from height 1,100,000 through
1,400,000 in bounded windows. Every checkpoint reopened with zero MPT
failures and matched `seed1.neo.org:10332 getstateroot`: 1,200,000,
1,300,000, and 1,400,000 roots are recorded in the continuation reports.
The 1,300,001 through 1,400,000 run used the new 16,384 default and ended at
height 1,400,000 with root
`0x06062f647755a4550e7dff72039dad5e4f57cf7b2ec99260bea984c70012df7c`.
The range contained 103,460 transactions and measured 180.93 overall and
721.21 transaction-bearing blocks/s; durable finalization remained the
dominant cost. This is correctness-positive continuation evidence, not full
MainNet-tip completion or a passing 1,500-block/s production speed proof.

The retained artifact is
[`reports/performance/mainnet-continuation-1300000-1400000.md`](../../../reports/performance/mainnet-continuation-1300000-1400000.md).
The large raw node log and replay database were intentionally removed after
their bounded summary and root evidence were recorded.

The next durable continuation reached height 1,500,000 with root
`0x058f29d359ec64eb394d061f44c9ba3e4991b41795ce745aaa3d8d55c9ff26af`,
matching the reference RPC after reopen and recording zero MPT failures. Its
full metrics are in
[`reports/performance/mainnet-continuation-1400000-1500000.md`](../../../reports/performance/mainnet-continuation-1400000-1500000.md).

The subsequent 100,000-block continuation reached height 1,600,000 with root
`0xe8e59ae6c7b10318c66d75d781188b7661ad17071448a71bcf0cb566bc6702a3`,
again matching the reference RPC after reopen with zero MPT failures. The
structured profile is recorded in
[`reports/performance/mainnet-continuation-1500000-1600000.md`](../../../reports/performance/mainnet-continuation-1500000-1600000.md).

The next continuation reached height 1,700,000 with root
`0xbe328747a11cf1b004f6e75524160bc8dbfddaca666f17cef25bc6fe50bed95e`,
matching the reference RPC after reopen with zero MPT failures. It is a
transaction-dense durable-MDBX stress baseline (152.26 overall blocks/s,
436.93 transaction-bearing blocks/s, and 44.9 GB peak RSS), recorded in
[`reports/performance/mainnet-continuation-1600000-1700000.md`](../../../reports/performance/mainnet-continuation-1600000-1700000.md).

### MDBX Sorted Worker Dispatch (2026-07-16)

Parallel ordered reads now call the bounded sorted-cursor worker for each
contiguous chunk and retain the authoritative seek fallback for sparse keys.
The height-811,000 to 812,000 replay matched the official root with zero MPT
failures. This window's sparse content-addressed keys still triggered the
fallback, so the change is retained for correct ordered workloads but is not
counted as a throughput win. The structured evidence is recorded in
[`reports/performance/mdbx-sorted-worker-ab.md`](../../../reports/performance/mdbx-sorted-worker-ab.md).

### MDBX Reader Repeat A/B (2026-07-16)

The same restore-verified height-811,000 checkpoint was replayed through
812,000 with eight and sixteen opt-in MDBX read workers. Both runs reached the
official root
`0xacd96890371c7c1df925cb172d2df7b5e87731b26f6bff7fa9a8284ed7598fac` with
zero MPT failures and matching Ledger/StateService heights. The fresh
16-reader run measured 182.30 overall and 904.42 transaction-bearing blocks/s,
versus 155.14 and 744.28 with eight readers. A second fresh 16-reader run also
matched the root but measured 172.64 overall and 686.49 transaction-bearing
blocks/s, so reader-count variance was too high to justify a default change.
A separate run with only
`NEO_MDBX_WRITE_INTENT_READ_THREADS=16` set (global parallelism unset) also
matched the root with zero MPT failures and measured 1.600 s for deferred
lookups. Later high-height evidence showed worse normalized cost for parallel
readers, so both environment switches were removed from production on
2026-07-22.
Full details and artifact hashes are in
[`reports/performance/mdbx-reader-repeat-ab.md`](../../../reports/performance/mdbx-reader-repeat-ab.md).

### MDBX Merge-Cursor A/B (2026-07-16)

An opt-in `NEO_MDBX_CURSOR_WRITE_MODE=merge_cursor` writer was root-verified on
durable full-state ranges. It uses `CURRENT` for exact cursor rows, bounds
sparse forward walks, and switches the remainder of a sparse overlay to the
proven search writer. The first per-key bounded candidate was rejected after
88.88 seconds of cursor work versus 3.23 seconds for search. The adaptive
candidate preserved root/reopen parity but did not improve the sparse MainNet
workload. The merge branch and environment switch were removed on 2026-07-22,
leaving the independent-search writer as the only production path. Evidence is recorded in
[`reports/performance/mdbx-cursor-merge-ab.md`](../../../reports/performance/mdbx-cursor-merge-ab.md).

### Continued Durable MainNet Replay Through Height 1,740,000 (2026-07-16)

The retained full-state database continued from height 1,700,000 through
1,740,000. Every bounded checkpoint reopened with matching Ledger and
StateService heights, zero MPT failures, and exact `seed1.neo.org:10332`
state-root agreement. The latest roots are recorded in:

- [`mainnet-continuation-1700000-1710500.md`](../../../reports/performance/mainnet-continuation-1700000-1710500.md)
- [`mainnet-continuation-1710500-1720000-writeintent16.md`](../../../reports/performance/mainnet-continuation-1710500-1720000-writeintent16.md)
- [`mainnet-continuation-1720000-1730000-writeintent16.md`](../../../reports/performance/mainnet-continuation-1720000-1730000-writeintent16.md)
- [`mainnet-continuation-1730000-1740000-writeintent16.md`](../../../reports/performance/mainnet-continuation-1730000-1740000-writeintent16.md)

The write-intent reader pool improved warm sparse lookup behavior but measured
72.62-104.32 end-to-end blocks/s on these durable windows, far below the
requested 1,500-2,000 blocks/s. Durable MPT/MDBX page writes and commit fences
remain the dominant measured bottleneck; correctness and performance claims
are kept separate.

The next durable continuation reached height 1,780,000 with root
`0x0dafbca58a202f3549fec514f441445c70520153b3acbee3ad5e20d803fc0f27`, matching
`seed1.neo.org:10332 getstateroot(1780000)` after reopen with zero MPT failures.
The 10,000-block window measured 124.65 end-to-end blocks/s (166.26 blocks/s
by height polling); telemetry metrics were disabled for this run. Its exact
structured report and node log are retained in
[`mainnet-continuation-1770000-1780000-writeintent16.md`](../../../reports/performance/mainnet-continuation-1770000-1780000-writeintent16.md).

## Honest Non-Pass Status

`bash scripts/validate-v310-consistency.sh` was run with localhost excluded
from the environment proxy. Neither the required Neo C# v3.10.1 endpoint nor
NeoGo endpoint was reachable in that run, so the validator exited 75 and
reported the comparison as unevaluated. Exit 75 is neither a parity match nor
a mismatch and does not satisfy tasks 4.3 through 4.5.

Repository-wide `openspec validate --all --strict` still reports 14 legacy
schema failures: six older changes contain no parsed specification deltas and
eight base specifications predate the required Purpose/Requirements shape.
The active change itself passes strict validation; the unrelated legacy
catalog was not rewritten as part of this consensus patch.

## Remaining Evidence Boundary

Task 4.5 remains open. Full compatibility still requires genesis-to-tip
MainNet replay and final state-root agreement.

### Continued Durable Replay After Height 1,780,000 (2026-07-16)

The retained full-state database continued through heights 1,795,000 and
1,817,000 with the durable production path. Both checkpoints reopened with
matching Ledger and StateService heights, zero MPT failures, and exact
`seed1.neo.org:10332` state-root agreement. The 1,785,000-1,795,000 window
measured 276.37 overall and 859.74 transaction-bearing blocks/s; the next
1,807,000-1,817,000 window measured 193.34 overall and 637.73
transaction-bearing blocks/s. Full stage breakdowns are retained in:

- [`mainnet-continuation-1785000-1795000.md`](../../../reports/performance/mainnet-continuation-1785000-1795000.md)
- [`mainnet-continuation-1807000-1817000-production.md`](../../../reports/performance/mainnet-continuation-1807000-1817000-production.md)

The additional MDBX flags, non-durable sync modes, pruning clone, and bounded
MPT cache were measured against exact roots but did not produce a safe,
sustained speed improvement. Their decisions and raw evidence are recorded
in [`mdbx-catchup-experiments-1781k-1817k.md`](../../../reports/performance/mdbx-catchup-experiments-1781k-1817k.md).

The same production path continued to height 1,827,000 with root
`0xec4e3d923618dbf803c488e6257c4104a216c6dd2d572b38eca966d59c472aba`, exact
reference agreement after reopen, and zero MPT failures. The window measured
226.81 overall and 813.10 transaction-bearing blocks/s; its stage profile is
[`mainnet-continuation-1817000-1827000-production.md`](../../../reports/performance/mainnet-continuation-1817000-1827000-production.md).

Two more durable production windows advanced the retained database through
height 1,847,000. Both fresh-process reopen checks found matching Ledger and
StateService heights, zero MPT failures, traversable persisted tries, and exact
agreement from both `seed1.neo.org:10332` and `seed2.neo.org:10332`:

- Height 1,837,000 root
  `0x9e2a33d44de098b7728b4c6356cb457aabf725bfd4a0427857f37df3fc2217ab`;
  [`mainnet-continuation-1827000-1837000-production.md`](../../../reports/performance/mainnet-continuation-1827000-1837000-production.md).
- Height 1,847,000 root
  `0x52f4d659ee7941abc2ca90d80ffdeb3712d92e08bab14bc8ee5623516dc6f8c0`;
  [`mainnet-continuation-1837000-1847000-production.md`](../../../reports/performance/mainnet-continuation-1837000-1847000-production.md).

The first window measured 171.71 overall and 633.78 transaction-bearing
blocks/s; the second measured 200.92 overall and 662.49 transaction-bearing
blocks/s. Moving the local Prometheus endpoint ahead of startup archive import
made the second window observable through ten successful live samples without
starting RPC or P2P before catch-up. The production speed gate remains failed.

At these heights, durable full-state publication remains the controlling
limit: the two windows spent 37.26 and 33.92 seconds in MDBX work, versus 3.51
and 3.37 seconds in VM execution. The measured workload is approximately 99.9%
content-addressed puts and exhibits large random B-tree write amplification.
The architecture and crash-recovery requirements for the next prototype are
recorded in
[`mpt-persistence-architecture.md`](../../../reports/performance/mpt-persistence-architecture.md):
append-only MPT node packs with derived sorted-run indexes, while MDBX retains
atomic publication authority for Ledger, root metadata, and the committed pack
high-water mark. This is an implementation direction, not task 4.5 completion
or a 2,000 blocks/s claim.

The optimistic-execution audit also found that the transaction-engine reuse
path retained Policy `ExecFeeFactor` and `StoragePrice` across transactions.
Official v3.10.1 creates every transaction engine from the latest same-block
snapshot. `prepare_next_transaction` now restores constructor defaults and
refreshes both values after rebinding that snapshot; a snapshot-backed
regression proves changed Policy values become visible without consuming or
resetting pending transaction writes. Optimistic execution remains deferred
until complete point/range dependency tracking and deterministic fallback are
implemented.
