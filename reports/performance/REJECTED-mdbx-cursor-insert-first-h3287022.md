# REJECTED: MDBX insert-first cursor resolution at height 3,277,022

Date: 2026-07-18

## Question

The deferred full-state journal resolves each node against MDBX at the write
cursor. Because 99% of the nodes in this window are new, test whether an
insert-first `NO_OVERWRITE` operation is materially faster than the established
`set_range` plus positioned `UPSERT`/`CURRENT` path.

The candidate was opt-in and disabled by default. Append-pack shadow writes
were disabled in both arms to isolate MDBX; strict ordinary-authoritative VM
specialization shadow remained enabled.

## Method

- Source checkpoint: MainNet height 3,277,022.
- Window: blocks 3,277,023 through 3,287,022, 10,000 blocks and 4,807
  transactions.
- Two offline physical MDBX clones with identical 124,554,051,584-byte source
  files; control ran first and candidate second.
- Durable MDBX, full StateService history, deferred finalization, the same
  chain.acc archive, config, release binary, and host.
- Release `neo-node` SHA-256:
  `2f14b5ca6b3f966828077d29ce30f4e694c752bab5176f462eb26190d28da7cd`.
- Host: VMware guest, 8 vCPUs, 62 GiB RAM, ext4 on `/dev/sda2`.

## Result

| Metric | Control | Insert-first | Change |
| --- | ---: | ---: | ---: |
| Import wall | 298.353 s | 292.062 s | 2.11% faster |
| All-block throughput | 33.52 blocks/s | 34.24 blocks/s | 2.15% faster |
| Cursor resolution | 240.865 s | 239.871 s | **0.41% faster** |
| Resolved nodes | 661,896 | 661,896 | identical |
| Normalized cursor resolution | 363.90 us/node | 362.40 us/node | 0.41% faster |
| Present nodes | 5,383 | 5,383 | identical |
| Absent nodes | 656,513 (99.19%) | 656,513 (99.19%) | identical |
| Cursor-stage physical reads | 3,778,637,824 B | 3,763,666,944 B | 0.40% lower |
| Cursor-stage major faults | 922,519 | 918,864 | 0.40% lower |
| Cursor-stage minor faults | 679,065 | 656,682 | 3.30% lower |
| Durable commit | 35.168 s | 29.841 s | 15.15% faster |
| Maximum RSS | 12,285,932 KiB | 7,800,924 KiB | order/cache-sensitive |

The cursor result is effectively neutral and far below the 10% acceptance
threshold. The 2.11% wall difference is mostly explained by the unrelated
5.33-second durable-commit difference. Live `pidstat` samples in both arms
showed roughly 3,400-4,300 major faults/s and 14-17 MiB/s reads with only
23-39% process CPU, confirming that cold random MDBX page access remains the
dominant cost. libmdbx already reuses the positioned leaf where possible, so
insert-first cannot remove the random page fault or copy-on-write update.

## Correctness

- Both arms produced StateRoot
  `0x64b8a0473213719b929de17f1dcbd9b1d15f503b185420f221524167b6426876`
  at height 3,287,022.
- `seed1.neo.org` and `seed2.neo.org` returned the same root.
- Both strict specialization-shadow runs exited successfully with no artifact
  mismatch or infrastructure failure.
- Both databases reopened through `neo-db-probe`.
- A deterministic 1,000-entry MPT state sample had the same SHA-256 in both
  arms: `2d937013dc53f2a357fbdc6b7797f468772efae63afd8d53063c7f4c433aa1fc`.

## Decision

Reject and remove the insert-first MDBX branch. Retain the exact
present/absent, physical-I/O, and fault counters because they identify the
real regime without changing storage semantics. The structural performance
path remains authoritative append-only node packs plus bounded ordered stage
overlap, not another MDBX cursor micro-optimization.

Evidence:

- `mdbx-cursor-insert-first-control-3277022-3287022-node.log`
- `mdbx-cursor-insert-first-control-3277022-3287022-time.txt`
- `mdbx-cursor-insert-first-candidate-3277022-3287022-node.log`
- `mdbx-cursor-insert-first-candidate-3277022-3287022-time.txt`
