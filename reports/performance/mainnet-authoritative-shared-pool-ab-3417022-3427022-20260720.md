# StateRoot-Enabled Authoritative Pack Shared-Pool A/B

Status: **accepted with `batch_value_workers = 2` on this host**. Four workers
were rejected. The accepted pooled result is **436.69 blocks/s**, still far
below the required 2,000 blocks/s with StateRoot enabled.

## Change

Large sorted authoritative-pack value reads now use a fallible process-wide
Rayon pool capped at eight threads. Each store caches
`min(configured_workers, available_parallelism)` at open, preflights the pool
before any recovery mutation, keeps index lookup ordered, partitions immutable
payload reads without splitting duplicate `(offset, length)` locations, and
publishes results in caller order. The path remains opt-in; the default is one
worker.

Authority reads additionally reject any indexed node above 1 MiB or aggregate
batch above 256 MiB before result allocation.

## Environment

- Host: 8 logical CPUs, 62 GiB RAM.
- Filesystem: ext4 on `/dev/sda2`.
- Node binary SHA-256:
  `97c8f02fa47abfd82a236a3706a5f7830b9444dde429e586e7c214f7e472e78d`.
- Source base: `5182fb1abea13a6fcfbe3b828125a63290ee2653` plus the
  uncommitted candidate diff used for this campaign.
- MainNet archive SHA-256:
  `6043a5c91735087bfb4dda33a2755603f58b6ce3706104510e726ea9aa78b1c0`.
- Both sides explicitly passed `--enable-stateroot`; no StateRoot-disabled
  performance run was performed.
- Both sides enabled full coordinated StateService, deferred full-state
  finalization, `track_during_catchup`, authoritative packs, random point
  mappings, and strict specialization shadow replay.
- Before every run, `sync` completed and `POSIX_FADV_DONTNEED` was applied to
  that side's MDBX files, pack files, and the shared archive.

## Accepted Workers=2 A/B

The two replicas started at block `3,417,022` with public StateRoot
`0x609fcaf511e1a89882b18b394569f508a799d7099b545d3f7dfebf07f92eb327`.
Worker assignment was swapped between replicas for the second window.

| Window | Workers=1 | Workers=2 | BPS delta |
|---|---:|---:|---:|
| `3,417,023..3,422,022` | 375.4809 | 388.5336 | +3.4763% |
| `3,422,023..3,427,022` | 488.8187 | 498.4779 | +1.9754% |
| Pooled 10,000 blocks | **424.7186** | **436.6920** | **+2.8191%** |

| Pooled metric | Workers=1 | Workers=2 | Signed delta |
|---|---:|---:|---:|
| Import elapsed | 23.5450 s | 22.8994 s | -2.7420% |
| Transaction-block throughput | 261.4157 blocks/s | 266.1807 blocks/s | +1.8228% |
| Finalization/store | 12.9895 s | 12.5324 s | **-3.5189%** |
| Deferred pack lookup | 3.7513 s | 3.2976 s | **-12.0933%** |
| MPT mutate changes | 5.1135 s | 5.0599 s | -1.0480% |
| Pack backing publication | 7.6257 s | 7.2380 s | -5.0848% |
| MDBX commit windows | 3.3062 s | 3.3891 s | +2.5071% |

Both windows had zero MPT apply failures and zero deferred lookup errors. The
first window ended at public StateRoot
`0x4f3294d72c4670aabd0f61abe3140477f1a780518f7629004996441b728f8d78`;
the second ended at
`0x371a6a8b013bac0ae66a9f02c661c1262c61a016b4791013dd869cec1547e159`.
Both replicas matched at both heights.

## Rejected Workers=4

Two earlier swapped 5,000-block windows produced pooled throughput
`426.9774 -> 428.7408 blocks/s` (`+0.4130%`), while finalization became
`1.2055%` slower and deferred lookup became `0.1053%` slower. Four workers are
therefore not accepted for this device. The implementation permits bounded
host-specific tuning, but the retained MainNet configuration uses two.

## Authority And Physical Verification

At block `3,427,022`, both replicas reopened against the same mandatory marker:

- Epoch: `284`.
- Frame end: `56,110,147,467`.
- Marker root internal bytes:
  `59e14715ec9c86dd131079b416a0612c26c161c6029f6ae60aac3b018b6a1a37`.
- Tip payload SHA-256:
  `713a4298e3c09c0fac8dfaa9794698bb55226e177ae53d91d48bd7aadf114f5e`.
- Opened geometry: 285 frames, 4 live runs, 227,678,349 index entries,
  zero compaction debt, and a reachable current root node.

The retained primary pack then passed a complete committed-frame scrub:
285 frames, 227,747,729 rows, 56,110,126,947 payload bytes, and
47,455,713,245 value bytes. The verifier ended with
`authority verification: ok (mandatory marker)` and peak RSS of about 542 MiB.

After verification, the duplicate MDBX/pack replica was deleted, freeing about
87 GiB. The retained primary is at height `3,427,022` and is configured with
two batch value workers.

## Decision

The shared bounded pool and workers=2 configuration are retained because the
swapped MainNet windows improved both end-to-end throughput and the targeted
lookup/finalization stages while preserving exact roots and authority. This is
a measured **2.82%** end-to-end gain, not a path to the 2,000 blocks/s target by
itself. The next optimization must target the materially larger pack backing
publication and MPT mutation stages rather than increasing read workers.
