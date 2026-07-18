# REJECTED: 8 parallel write-intent readers at 1.95M full-state heights

Date: 2026-07-17

## A/B

`NEO_MDBX_WRITE_INTENT_READ_THREADS=8` (candidate) vs default serial (control)
on adjacent strict-shadow MainNet windows, same binary
(SHA-256 `3465fb8a…bbd` + stack-graph artifact-limit raise), same store,
same chain.acc, coordinated deferred full-state finalization:

| Window | Mode | Overlay entries | Deferred lookup | µs/entry | Wall |
|---|---|---:|---:|---:|---:|
| 1,927,022-1,937,022 | serial | 1,051,445 | 160.5 s | 153 | 326.3 s |
| 1,937,022-1,947,022 | serial | 1,149,636 | 135.5 s | 118 | 308.8 s |
| 1,957,022-1,967,022 | 8 readers | 713,511 | 123.5 s | **173** | 249.7 s |

The candidate's lower wall time is fully explained by a lighter window (6,016
transactions vs 12,844/16,683; 713k overlay entries vs ~1.1M). Normalized per
overlay entry the parallel read pool is *slower* (173 µs vs 118-153 µs): at
these heights the ~50 GB full-state MDBX table makes probes cold-page-fault
bound on the shared virtual disk, so eight readers queue on the same device
and add merge overhead. This mirrors the earlier global parallel-read
rejection (`session-20260715/parallel-batch-ab.json`, ~5% dense regression)
and does not contradict the h811 measurement where the table was smaller and
page-cache resident.

Correctness held: strict shadow reported zero mismatches and the local root
at 1,967,022 matched seed1
(`0x7fe08990008942fb73da86c45ff53ea0312ddf1d5279000fdaf9ba46e380bfa8`).

## Decision

Default remains serial (`NEO_MDBX_WRITE_INTENT_READ_THREADS=1`). Parallel
write-intent reads stay opt-in for small/resident-table regimes only. The
deferred-lookup cost must be removed structurally (fusing reference
resolution into the commit cursor, eliminating the second B-tree sweep), not
hidden with read parallelism.

Evidence: `mainnet-shadow-observed-1957022-1967022-wi8-node.log`,
`mainnet-shadow-observed-1927022-1937022-node.log`,
`mainnet-shadow-observed-1937022-1947022-node.log`.
