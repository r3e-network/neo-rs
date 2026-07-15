# MainNet Sync Profiling

Captured 2026-07-14 against the retained full-state MDBX checkpoint at height
811,000. The reference checkpoint root is
`0x9df126e57283eef16d2d1afa26a85ecc7ee1384ebcd8b3a9a4127a8b8068c1ce`.

## Cursor and Snapshot A/B

All runs imported the same 10,000 blocks (811,001 through 821,000) and ended
at the same StateService root:
`0xc94105f375f4f38438f129a75d4e172d36f655b82ebe3c78ca2a06179302a3ba`.

| Variant | Import seconds | Blocks/s | Finalization seconds | Backing commit EWMA (us) | Mutation EWMA (us) |
|---|---:|---:|---:|---:|---:|
| Fresh MDBX cursor per write | 204.336671 | 48.938842 | 191.139630 | 6,261 | 7,221 |
| Reused writable cursor | 203.343336 | 49.177909 | 190.051617 | 5,659 | 8,031 |
| Reused cursor + cached snapshot table | 177.632684 | 56.295946 | 165.118316 | 5,790 | 6,148 |

The snapshot table cache improves end-to-end import time by 12.6% versus the
cursor-only run. The gain is in trie/snapshot work, not the durable commit;
the backing-commit EWMA is slightly higher in the cached run. MPT failures were
zero in every run.

## Warm Two-Window Replay

The rebuilt node continued the same database from height 821,000 through
841,000 in one process. Both progress windows reported zero MPT failures:

| Window | Batch blocks/s | Apply total (us) | Apply avg (us/block) | Mutation total (us) | Backing total (us) |
|---|---:|---:|---:|---:|---:|
| 821,001-831,000 | 55.491538 | 167,170,000 | 16,717 | 105,121,190 | 61,380,271 |
| 831,001-841,000 | 58.455718 | 157,830,000 | 15,783 | 102,862,087 | 54,122,688 |

The final import report was 351.277318 seconds at 56.935074 blocks/s, with
325.016998 seconds in finalization. The local canonical Ledger and
StateService heights were both 841,000. The local root
`0x8554b783e78604e3f6c3afabde84d00be3005f6e42fc2b3b22b3f04ef2548189`
matched `getstateroot(841000)` from both seed1 and seed2 RPCs.

The retained structured log is
[`mainnet-sync-h821-841-node.log`](mainnet-sync-h821-841-node.log), SHA-256
`df970925d1eb2952c6302b57365d88760241ba13ae8d0fa50257132c509b7106`.
The process-level sample is
[`mainnet-sync-h821-841-pidstat.log`](mainnet-sync-h821-841-pidstat.log),
SHA-256 `bd2f0c4a64107c0c0352619d498ba778dd2f2a5091f8081c00d352651b02a5c4`.

These are performance and correctness observations for continued engineering;
they do not close the OpenSpec full-MainNet replay gates or the 1,500
transaction-bearing blocks/s production-speed gate.

## Ordered DataCache Prefix Overlay

`DataCache::find` previously scanned every pending key for each prefix query.
During deferred catch-up this made NeoToken candidate and GAS lookups scale with
the complete overlay instead of the matching prefix. The ordered change index
changes that lookup from `O(C)` to `O(log C + P)`, where `C` is the number of
pending changes and `P` is the number of matching changes. Forward/backward
ordering, tombstones, and layered parent/child cache semantics are covered by
the storage test suite.

Both A/B runs used the same full-state MDBX checkpoints and chain archive:

| Range | Baseline blocks/s | Ordered index blocks/s | Empty blocks/s | Transaction blocks/s |
|---|---:|---:|---:|---:|
| 100,001-200,000 | 1,529.56 | 1,604.80 | 2,524 -> 6,940 | 1,388 -> 3,916 |
| 200,001-300,000 | 649.31 | 691.05 | 1,785 -> 4,864 | 235.64 -> 234.83 |

At height 300,000 the final 10,000-block window improved from 151.68 to
163.49 blocks/s. NeoToken `PostPersist` fell from 2,824 to 23 us and its
candidate scan fell from 4,503 to 171 us. The remaining dense-window cost is
not the candidate query: 11,143 transactions spend about 9 seconds in VM
execution while StateService emits 2.67 million MDBX entries (453 MB), with
about 29 seconds in cursor writes and 18 seconds committing transactions.

The height-200,000 root matched the retained checkpoint and both reference
RPCs at
`0x35fcc7d5c624516be1f1a97178476f7cbb8d282ca2a5ac9e9fc25b08088d2f9e`.
The height-300,000 root likewise matched the local baseline and both reference
RPCs at
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.

Raw artifacts:

- [`neo-rs-async-full4096-coalesce2048-h100-200k-node.log`](neo-rs-async-full4096-coalesce2048-h100-200k-node.log), SHA-256 `6826a3a788bba9866238aed02076b97fe7d357a446a19952d79dd35df45f9a00`
- [`neo-range-index-h100-200k-node.log`](neo-range-index-h100-200k-node.log), SHA-256 `0280eb2e8be378b6aec4c55222f41890fa001662357077b99ece7828d6b830cd`
- [`neo-rs-async-full4096-coalesce2048-h200-300k-node.log`](neo-rs-async-full4096-coalesce2048-h200-300k-node.log), SHA-256 `2ac5c779165e322ec802d5c9f200b88aadad9c48b1914d7a24950ef5a4518a19`
- [`neo-range-index-h200-300k-node.log`](neo-range-index-h200-300k-node.log), SHA-256 `304fa98b0eba1788450b4e5754541cc522f2fc529239a07084fc1c7147317d74`

## Work-Aware StateService Batching

A same-checkpoint tmpfs A/B isolated the effect of bounding an asynchronous
StateService batch by projected changes as well as block count. Both variants
used `full_state=true`, `coordinated=false`, the same height-100,000 checkpoint,
and replayed through height 300,000.

| Variant | 100k-300k blocks/s | 290k-300k blocks/s | MDBX transactions | Cursor writes | Commit |
|---|---:|---:|---:|---:|---:|
| Block-only batching | 2,034.39 | 239.20 | 11 | 25.06 s | 2.11 s |
| 8,192 projected-change cap | 2,952.42 | 517.84 | 31 | 4.28 s | 3.71 s |

The final window contained the same 212,319 projected state changes. Smaller
transactions increased commit count and commit time, but removed the strongly
nonlinear cursor cost: total measured MDBX time fell from 29.18 to 9.99
seconds. The capped run ended at the accepted height-300,000 root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.

This does not yet validate the production-default coordinated mode. That path
queues the complete canonical import boundary before applying StateService
changes, so the asynchronous work cap does not constrain its atomic MDBX
transaction. The correct follow-up is to split the canonical deferred import
at an adaptive work budget while committing Ledger and StateService together;
weakening MDBX durability is not an acceptable substitute. A durable-disk A/B
for the chosen work budget also remains required.

Raw artifacts:

- [`neo-rs-datacache-range-ram-h100-300k-node.log`](neo-rs-datacache-range-ram-h100-300k-node.log), SHA-256 `146f95a0d63f80f59d224c5ffb1ad2cc74db054430f28c2592fe6eb225557b58`
- [`neo-rs-datacache-workcap8192-ram-h100-300k-node.log`](neo-rs-datacache-workcap8192-ram-h100-300k-node.log), SHA-256 `5b39e80bb68ceef44a7aa4e9184bb007e8bce32842e72b569ac920fd147473e4`

## Pruning Comparison

An isolated reflinked copy of the 851,000 database was replayed through
861,000 with only `[state_service].full_state=false`. It reached
`483.283968 blocks/s` in `20.691769s`; MPT apply was `6,470,000 us` total
(`647 us/block`) and backing commit was `2,505,338 us` total. The local root
`0xbd16829d880af8e12a788b1c98d44176e9ab84c722c7442beb13e2e6b6f87bc2`
matched seed1 `getstateroot(861000)`. This is a current-state correctness and
performance result, not permission to discard historical state in the default
full-state replay mode. Transaction execution and bounded archive scanning
still keep this below the 1,500-2,000 blocks/s target.

Raw artifacts:

- [`mainnet-sync-pruning-h851-861-node.log`](mainnet-sync-pruning-h851-861-node.log), SHA-256 `bfaebade78de0a51579ab04e778a61db8041d82d3b2db3479f99e54a036662f3`
- [`mainnet-sync-pruning-h851-861-pidstat.log`](mainnet-sync-pruning-h851-861-pidstat.log), SHA-256 `dac059d0dfdeffeab056ee64cf607a74184e1e4628f476eb9575744e46fe7039`

## Dense VM And Contract Execution

All dense measurements below replayed the same height 290,000 ledger checkpoint
through 300,000: 10,000 blocks, 5,688 transaction-bearing blocks, and 11,143
transactions. StateService was disabled for these runs to isolate ledger and VM
execution. Every retained run reached height 300,000 without an import error.

| Change | Baseline blocks/s | Optimized blocks/s | Change |
|---|---:|---:|---:|
| ContractState-aware storage context | 711.92 | 1,182.00 | +66.03% |
| Context/hash and small-integer batch | 1,169.66 | 1,457.60 | +24.62% |
| Direct resolved StdLib dispatch | 1,473.33 | 1,502.69 | +1.99% |
| Typed StdLib hot methods | 1,514.93 | 1,523.24 | +0.55% |

The retained combined tree was then measured three times at 1,765.17,
1,765.31, and 1,766.09 blocks/s (mean 1,765.53). Transaction-bearing
throughput averaged 1,044.67 blocks/s and transaction execution took 5.4448
seconds. Relative to the immediately preceding typed-StdLib runs, the immutable
`Arc<ContractState>` cache and identity-preserving shared Script storage improved
total throughput by 15.91%, transaction-bearing throughput by 16.65%, and
transaction time by 14.27%. These two batches were adjacent pinned runs, not an
interleaved A/B, so the result is reported as a retained-tree delta rather than
attributed to one source line.

Script identity is consensus-observable through VM Pointer equality. An initial
experiment incorrectly reused the same `Arc<Script>` between independent
contract loads. Ledger-only throughput was about 1,771 blocks/s, but full-state
replay first diverged at height 274,157 and ended at the wrong height-300,000
root `0xb5ca4c51cdcff4129f2e43f04b5f8878f4f588c916c15c2e867e30c9703585c7`.
That variant was rejected. The retained implementation gives each load a
distinct Script identity while sharing immutable bytecode and instruction
storage, and preserves pre-Domovoi snapshot permission reads.

Raw retained ledger artifacts are in [`current-combined-ledger`](current-combined-ledger/).
The rejected stateful log has SHA-256
`5bd8f2bfee8523f4b743fce1f0e597d272cb3804010802c400bd16fd0c17cf3d`.

## Stateful Correctness Gate

The retained tree replayed full-state MainNet from the accepted height-100,000
checkpoint through height 300,000. It processed 200,000 blocks at 3,102.00
blocks/s overall, recorded zero MPT failures, and produced the official root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`,
confirmed independently from seed1 RPC. The replay log SHA-256 is
`624314614b8e35c59553f587e3abbbe8d7f4588f4f56c6ee1f6965e58f863224`.

The dense final 10,000-block stateful window remains below target at 569.48
blocks/s. Its 212,319 state changes spend 17.19 seconds in MPT application, so
the next throughput work belongs in StateService/MDBX rather than VM execution.

## Pruned MPT Dense-Window Campaign

Runs 24-93 replayed the same height-100,000 checkpoints through height
300,000. The table reports the final 290,001-300,000 window. CV uses sample
standard deviation. The one-sided 95% lower bound is
`mean - t(0.95,n-1) * s / sqrt(n)`; for each five-run candidate,
`t(0.95,4) = 2.1318468`.

| Retained change | Control runs | Candidate runs | Mean blocks/s | CV | Candidate 95% LCB | MPT apply | Mutation | Trie commit |
|---|---|---|---:|---:|---:|---:|---:|---:|
| Sample MDBX cursor timing | 24,27,29,30,32 | 25,26,28,31,33 | 1,487.77 -> 1,490.08 | 1.02% -> 0.93% | 1,476.91 | 6.354s -> 6.352s | 4.428s -> 4.428s | 0.320s -> 0.322s |
| Inline durable-miss keys | 54,57,59,60,62 | 55,56,58,61,63 | 1,479.55 -> 1,502.34 | 2.75% -> 1.95% | 1,474.35 | 6.395s -> 6.296s | 4.449s -> 4.430s | 0.327s -> 0.286s |
| Borrow MPT store `Arc` | 84,87,89,90,92 | 85,86,88,91,93 | 1,496.34 -> 1,509.38 | 2.71% -> 1.67% | 1,485.33 | 6.319s -> 6.266s | 4.450s -> 4.395s | 0.287s -> 0.285s |

The sampled timer replaced exhaustive timing of 387,098 cursor writes and
reduced overlay visitation by 29.001 ms. Inline durable-miss keys improved
mean throughput by 1.54%, reduced MPT apply by 99.038 ms, and reduced trie
commit by 41.716 ms (12.74%). Borrowing the store `Arc` improved mean
throughput by 0.87% and reduced mutation by 55.343 ms.

A raw ten-run repeatability series produced a mean of 1,432.36 blocks/s,
13.05% CV, a range of 916.44-1,536.68, and a one-sided 95% lower bound of
1,323.99 blocks/s. The final retained candidate's lower bound is 1,485.33, so
these measurements do not establish sustained 1,500-2,000 blocks/s.

All 70 runs exited successfully at Ledger and StateService height 300,000,
with zero MPT failures and official root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.

These are pruning-mode microbenchmarks on tmpfs with `full_state=false`,
`coordinated=false`, archive verification disabled, and CPU affinity 2,6.
They start from trusted height-100,000 checkpoints, do not exercise P2P
download or production durable-disk behavior, and do not satisfy the staged
or full-MainNet replay gates.

## Follow-Up Mutation And MDBX Experiments

An owned-first-resolution experiment avoided the first decoded-node clone and
re-decoded only repeated resolutions. Diagnostic counters observed 255,813
store-backed first moves and 8,024 repeated store decodes (3.04%). The only
uncontaminated adjacent pair was negative at 1,501.16 control versus 1,490.15
candidate blocks/s. A compiler job began during run 96 and later fuzz and
JavaScript jobs occupied benchmark CPUs, invalidating the remaining throughput
comparison. The extra retained bytes and re-decode path were removed.

The next candidate keeps the ordered reusable MDBX cursor for puts but uses
MDBX's single-call transaction delete for tombstones instead of cursor lookup
followed by cursor delete. Runs 114-118 completed under the same persistent
background load; run 119 is excluded because two clippy jobs started during
the run. The two usable controls averaged 1,372.71 blocks/s and the three
candidates averaged 1,387.51 (+1.08%). More importantly, the directly affected
stages moved consistently: backing commit fell by 19.75 ms, overlay visitation
by 12.60 ms, estimated cursor writes by 8.63 ms, and MDBX transaction commit by
3.44 ms. This low-risk candidate is retained provisionally, but the sample is
not a sustained-throughput proof and requires a clean 5x5 repeat.

The frozen binary hashes are:

- no-store-`Arc` control: `6a798759d694311cfaf813dc60fe2d9e2b409f1c0130ca0bf52e7b02a2cf7374`
- rejected owned-first candidate: `90e152296a0d9aca330f17ed677ba8024dd8682ac13b0d02367bb23cb8afdbe2`
- provisional transaction-delete candidate: `7d847d9dac8ef728c49e54bda48fa96ca793b5e9809a71c5b3bf433dec8a97f3`

The natural-order SHA-256 manifest for all 119 raw logs is
[`mpt-known-old-hash-ab/SHA256SUMS`](mpt-known-old-hash-ab/SHA256SUMS), with
SHA-256 `1a67c5176c83d1f85569e718330911c799c01fe71a4f05aebf7811ce9c153651`.

## MPT Mutation Amplification Diagnostic

An instrumented pruning-mode replay restored the accepted full-state
height-100,000 checkpoint and replayed through height 300,000. The run used
the provisional transaction-delete implementation and recorded exact
per-trie counters without hot-path atomics. It ended at Ledger and StateService
height 300,000 with zero MPT failures and the official root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.

The final 290,001-300,000 window contained 212,319 projected storage changes:

| Diagnostic | Total | Per change |
|---|---:|---:|
| `put_node_cached` finalizations | 2,799,699 | 13.19 |
| Repeated branch/extension finalizations | 1,839,460 | 8.66 |
| Actual node SHA-256 computations | 2,989,666 | 14.08 |
| Serialized node payload | 455,867,440 bytes | 2,147 bytes |

Repeated shared ancestors were 65.70% of all cached node finalizations. The
sum of the block-end overlay working-set samples was 75,479,151 entries, while
the final published overlay contained 319,326 entries. This establishes
substantial same-block ancestor churn and justifies a prototype that defers
dirty-ancestor serialization/hashing until the block root is finalized.

The instrumented binary SHA-256 was
`d219b98d69f9ed113d3467267641b053ed7297e105b4f2831ad62675adcfca19`.
The raw log is
[`mpt-mutation-profile-h100-300k-node.log`](mpt-mutation-profile-h100-300k-node.log),
SHA-256 `917c149b7fdaf7b68639859e320f57248b697d7b49b68e8e3ad041623a4b84ec`.
The host was not isolated and the binary included instrumentation, so its
throughput is diagnostic only and is excluded from the MDBX A/B series.

## Deferred Pruning Finalization

The pruning trie now delays serialization, hashing, and cache insertion for
dirty materialized nodes until the block root is finalized. Finalization walks
the dirty subtree in post-order exactly once. Full-state mode retains the eager
C# path. An internal accounted hash is distinct from the memoized current hash,
so a transient root probe cannot decrement a coincidentally equal durable node.

The implementation was accepted only after eager/deferred tests compared every
block root and the complete persisted MPT namespace, including serialized
reference counts. The corpus covers prefix splits, branch compression,
commit/reopen histories, transient duplicate hashes, 253 identical leaves,
injected commit retry failure, and fixed-seed randomized histories. This audit
also removed an old-leaf reference increment that is absent from pinned Neo
Modules v3.7.5 (`8c6b64b75cb2d133714d4a843f2dcb84dd16ddec`).

Five runs restored the same immutable height-100,000 base and replayed through
height 300,000 with the frozen release binary. Every run ended at Ledger and
StateService height 300,000, recorded zero MPT failures, and matched the
official root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.

| Run | 290,001-300,000 blocks/s | 100k-300k transaction blocks/s | MPT apply | Driver time |
|---:|---:|---:|---:|---:|
| 1 | 1,633.23 | 1,871.37 | 3.736 s | 19.559 s |
| 2 | 1,631.20 | 1,860.95 | 3.794 s | 19.638 s |
| 3 | 1,597.48 | 1,816.57 | 3.980 s | 20.170 s |
| 4 | 1,628.69 | 1,856.23 | 3.838 s | 19.633 s |
| 5 | 1,637.02 | 1,862.62 | 3.870 s | 19.676 s |

Dense-window throughput averaged 1,625.52 blocks/s with 0.98% CV. Its
one-sided 95% lower bound is 1,610.30 blocks/s. Transaction-bearing throughput
over the full 100k-300k range averaged 1,853.55 blocks/s with a 1,833.16 lower
bound. Within the dense final window, transaction-bearing blocks averaged
985.47 blocks/s; this is now the primary ceiling. Relative to the instrumented
eager diagnostic, dense total throughput improved 10.85%, overall 100k-300k
throughput improved 23.63%, and MPT apply time fell 40.45%.

| Dense-window mutation work | Eager | Deferred | Change |
|---|---:|---:|---:|
| Node finalizations | 2,799,699 | 934,236 | -66.63% |
| Repeated ancestor finalizations | 1,839,460 | 0 | -100.00% |
| Hash computations | 2,989,666 | 939,137-939,164 | -68.59% |
| Serialized payload bytes | 455,867,440 | 141,015,863 | -69.07% |
| Mutation + root hash + trie commit | 4.890 s | 2.136 s mean | -56.33% |

The frozen binary SHA-256 is
`3de24420da2e316c70a4cfad2202a8f377b66c05d1d21065d1ffb1e90060aa60`.
Raw-log and input hashes are recorded in
[`mpt-deferred-h100-300k-SHA256SUMS`](mpt-deferred-h100-300k-SHA256SUMS).

A diagnostic replay of the same range logged transactions whose VM execution
exceeded 1 ms. It captured 1,198 transactions, 1.892 seconds of execution, and
11,419,996 instructions, for a weighted 165.66 ns/instruction. Of these, 1,197
halted and one entered the transaction's ordinary persisted `FAULT` state;
there were no import or MPT failures. The slowest transaction was
`0xeee88f8433c8807936ca4cb45d3ea54a7cc54a9b3d4adcba529287aceffbb92c`
at height 294,174: 127,101 instructions in 34.621 ms. Its 15,100-byte script
repeatedly invokes GhostMarket `listToken` on
`0xcc638d55d99fc81295daccbaf722b84f179fb9c4`.

The retained tmpfs databases were probed after shutdown. Ledger and
StateService were both at height 300,000, and the persisted root matched the
official checkpoint. The structured evidence is
[`mpt-deferred-h290-300k-slowtx-probe.json`](mpt-deferred-h290-300k-slowtx-probe.json);
the source log is
[`mpt-deferred-h290-300k-slowtx-node.log`](mpt-deferred-h290-300k-slowtx-node.log).

An opt-in replay then profiled only that slowest transaction. Its 127,101
instructions included 31,000 pushes, 27,500 slot operations, 22,701 control
flow operations, 20,700 stack operations, and 14,000 compound operations. The
top opcodes were `PUSH0` (8,100), `SYSCALL` (7,000), `DUP` (5,400), `PUSH1`
(5,100), `ROT` and `LDARG0` (4,800 each), `PACK` (4,300), `SWAP` (4,000),
and `SETITEM` (3,800). Evaluation-stack activity comprised 87,400 pushes,
83,700 pops, 10,600 peeks, and 11,600 insert/remove/swap operations.

The targeted run replayed height 100,000 through 294,174 with zero MPT
failures. Its local root
`0x7730fcf47c8b61d45ae8c2bb0dfe88510c9ae05fddbd5922ae638cc072a6ec6f`
matched a live seed1 RPC query. The instrumented 59.291 ms transaction time is
not benchmark evidence: opt-in atomic counters deliberately trade speed for
exact counts, and unrelated host CPU load was present. The profile summary is
[`vm-profile-h100-294174-eee88f-summary.json`](vm-profile-h100-294174-eee88f-summary.json),
and the full log is
[`vm-profile-h100-294174-eee88f-node.log`](vm-profile-h100-294174-eee88f-node.log).

These are pruning-mode tmpfs archive-import results pinned to CPUs 2 and 6 on
a non-isolated host. They establish repeatable local dense-window performance
and state-root correctness, but do not close the durable-disk, live-P2P,
staged-artifact, or genesis-to-tip MainNet replay gates.

## Rejected MDBX Membership Filter

A process-local Bloom filter was tested against the same pruning-mode
height-811,000 checkpoint after ordered batch lookup had raised the control to
528.23 blocks/s. The filter was sized for all 36,782,069 StateService rows at a
`1e-5` false-positive target. Construction scanned the complete named table,
allocated 881,395,712 bits (105.1 MiB), and took 391.248 seconds. The resulting
10,000-block import fell to 24.84 blocks/s, with 2,789,496 major faults, 10.0
GiB peak RSS, and 402.513 seconds of measured import time.

After construction, trie commit work fell to 0.147 seconds, confirming that
negative durable lookups are the immediate cold-path hotspot. The design is
still rejected: its startup scan is prohibitive, its fixed capacity saturates
during genesis-to-tip growth, independent MDBX handles can make the
process-local filter stale, and the filter mutex makes reentrant overlay reads
deadlock-prone.

The run ended with Ledger and StateService both at height 821,000. An
independent `neo-db-probe` read returned
`0xc94105f375f4f38438f129a75d4e172d36f655b82ebe3c78ca2a06179302a3ba`,
matching fresh seed1 and seed2 `getstateroot(821000)` responses. The rejected
binary SHA-256 is
`843d38463e398d9b3938c2acafe70d06e7e22443a6ef8658099e6d56520c907c`.
The retained log is
[`mpt-bloom-rejected-h811-821-node.log`](mpt-bloom-rejected-h811-821-node.log),
SHA-256 `426c4519573d80e89aaa08061743222a8dbdc9a9a5832385e92f2437027803fa`.

## Rejected Cross-Block Cache Retention

A follow-up retained finalized MPT cache entries between block checkpoints in
the coordinated batch. It preserved the official height-821,000 root and had
zero lookup or MPT failures, but saved only 1,950 exact lookups (0.57%). Trie
commit regressed from 12.235 to 28.867 seconds, finalization from 15.221 to
31.918 seconds, and throughput from 528.23 to 281.10 blocks/s. Peak RSS rose
from 11.36 to 11.78 GB. The candidate also emitted 1,374 harmless extra
absent-key tombstones because clean cache retention does not preserve the
write batch's `absent_from_base` provenance.

The result rejects decoded-cache retention as the cross-block abstraction.
The needed design must defer unresolved reference transforms themselves and
reconcile only surviving hashes at the final batch boundary. The rejected
binary SHA-256 is
`eea030b5bee1ca9d1b56fccbcee4baf45f2c022ff6c6fd82279a9186931a0d33`.
The retained log is
[`mpt-cross-block-cache-h811-821-node.log`](mpt-cross-block-cache-h811-821-node.log),
SHA-256 `99d1d6be2d6000f14c98feec18687c090d28b6509277b983eb18b4f529e4178c`.

## Buffered Archive Resume

The same replay replaced one `read(4) + seek` pair per skipped `chain.acc`
record with bounded buffered consumption. Resume scanning through 811,000
records fell from 15.338 seconds to 0.130 seconds (117.6x) while retaining
record-size validation and truncated-payload rejection. This changes full
driver latency, not the import-only blocks/s denominator; it is retained as a
separate fast-sync improvement.

## Redundant Stack Attachment

`ExecutionContext::push` attached compound values to the engine reference
counter and then called `EvaluationStack::push`, which performs the same
attachment. Primitive values make this check a no-op, but already-attached
Array, Struct, and Map values took a second object mutex. The targeted profile
observed 87,400 stack pushes, 14,000 compound opcodes, and 4,300 `PACK`
instructions in the selected transaction.

A retained `NEWARRAY0`/`DROP` microbenchmark exercises that exact context push
path. Removing the outer attachment reduced its mean time from 315.872 to
307.800 us per 1,000 cycles (-2.56%). Criterion's 95% change interval was
-2.84% to -2.31% (`p < 0.05`).

Three interleaved binary A/B pairs then restored the same height-100,000 base
and replayed through height 300,000 with profiling disabled:

| Variant | Dense blocks/s | Transaction blocks/s | Transaction time | MPT apply |
|---|---:|---:|---:|---:|
| Duplicate attachment | 1,621.29 | 1,849.79 | 9.653 s | 3.811 s |
| Single attachment | 1,653.41 | 1,879.57 | 9.500 s | 3.781 s |
| Change | +1.98% | +1.61% | -1.59% | -0.79% |

Every candidate run exceeded every control run. All six runs reached Ledger
and StateService height 300,000, recorded zero MPT failures, and matched the
official root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.
The host was not isolated, so the replay series supports retaining the change
but does not replace the five-run sustained-throughput gate. Machine-readable
results and artifact hashes are in
[`vm-stack-attach-ab-summary.json`](vm-stack-attach-ab-summary.json).

## Profiler Limitations

Linux `perf`/BPF counters are restricted in this environment. `gprofng` also
reported `itimer could not be set`; its empty profile is invalid and was not
used. Qualitative CPU evidence came from repeated all-thread GDB interrupts
during the dense range. Every retained optimization was decided by pinned
multi-run replay and state-root checks, not by sampling alone.

## Session 2026-07-15 addendum

Baseline re-run (pruning, uncoordinated, tmpfs, h100k→300k) reached **1,599.79** dense
blocks/s and **11,523.73** overall with official root
`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`.

Retained: fail-closed `try_get_result`, MPT negative cache for proven-absent keys,
fast-sync optional SHA-256 package authenticity. Rejected as default: parallel MDBX
batch reads (≈−5.5% dense A/B). See
[`session-20260715/SUMMARY.md`](session-20260715/SUMMARY.md).
