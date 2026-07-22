# Positioned state-pack index v5

Date: 2026-07-22

Code commit: `aaa463796f2c688786e1e3db6feef6cc511f77a9`

Status: **accepted as a required format and recovery foundation; rejected as
a performance optimization**.

## Outcome

Index v5 makes every put record name its segment, value offset, and value
length in one fixed 64-byte encoding. Tombstones have one canonical zero
location. The store now accepts only the current v5 index format, uses the
mmap-backed blocked Bloom filter for every run, and rejects prior/unknown
versions instead of maintaining parallel readers.

This is required before safe multi-segment positioned reads can be completed,
but the identical durable component campaign regressed from **5,998.77 to
5,831.04 represented blocks/s (-2.80%)**. The only material improvement was
decoded-index memory, **458,560 to 297,216 bytes (-35.18%)**. This commit is
therefore not described as a speedup.

## Identical campaign

- Workload: deterministic projection of MainNet `1,877,001..1,887,000`.
- Prefill: 1,048,576 rows in 32 durable frames.
- Campaign: 131,072 operations, 1,300 represented blocks, 8 durable commits.
- Host label: Intel Core Ultra 9 285K, 8 visible CPUs, ext4 shared VM volume.
- Durability: append write, `sync_data`, atomic run publication, directory
  sync, compaction, reopen, and digest verification.
- Baseline source: `2af9dda040c60eba36f538079af24d2eb0871c02`.
- Candidate source: `aaa463796f2c688786e1e3db6feef6cc511f77a9`.
- Baseline binary SHA-256:
  `0c10116f20fe023d7fe8b79823ad6dbfd19e5fb46d222b462c10bb626807ae07`.
- Candidate binary SHA-256:
  `659eaea904982f7f4bddcd9d3e8c0810372c44fc9111295ec733320fc46307f4`.

| Metric | v3 baseline | v5 candidate | Delta |
|---|---:|---:|---:|
| Represented blocks/s | 5,998.77 | 5,831.04 | -2.80% |
| Campaign wall time | 216.711 ms | 222.945 ms | +2.88% |
| Process physical writes | 55,984,128 B | 60,932,096 B | +8.84% |
| Campaign index build | 6.547 ms | 7.275 ms | +11.12% |
| Compaction wall time | 348.426 ms | 377.199 ms | +8.26% |
| Compaction bytes written | 57,656,064 B | 73,253,632 B | +27.05% |
| Live index bytes | 61,111,488 B | 77,563,392 B | +26.92% |
| Decoded index memory | 458,560 B | 297,216 B | **-35.18%** |
| Point-read mean | 239 ns/key | 319 ns/key | +33.47% |
| Sorted-read mean | 184 ns/key | 198 ns/key | +7.61% |
| Reopen verification | 192.054 ms | 195.734 ms | +1.92% |

Both arms produced the same prefill digest, campaign digest, and reopened
state digest. The candidate reopened 40 authenticated frames and 8 live runs,
then verified 32,798 present/absent keys without a mismatch.

Raw report hashes:

- Baseline JSON: `df6d30b9f4267c4df8a1ee73c73809076f72a74f5d2fdbefd49a167bec85616b`.
- Baseline JSONL: `afb71b53c15b96459c1b4784fec9e26a891bd8dd1e1093f0888e59bcc0b83764`.
- Candidate JSON: `6c2ced71e04f8760a064684225d25b19e2c2d207f5ccbf272e01bef135ca21c9`.
- Candidate JSONL: `aa28f561c90be6e144f4b7adf02b5087fe7ab5a84d21b81a27bbfc20cc7ad787`.

## Production safety corrections

The review rejected an intermediate implementation that enforced the default
4 GiB segment target before complete rotation existed. Production append is
still single-segment until write rotation, recovery, positioned read routing,
and migration are implemented together; the OpenSpec segment task remains
open.

Recovery now authenticates and preflights every selected frame against the
cumulative decoded-index memory bound before truncating or replacing any pack
artifact. Cleanup removes only exact engine-generated run/manifest names and
preserves unrelated operator files. The v5 recovery estimator also no longer
double-counts the 64-byte record section.

## StateRoot-enabled node result

No node-level blocks/s result is established for this commit. A current
release binary (`631ef285e524c9b61903abb49c6c161e80ca5d760f795bbca55ef94283d7bb9d`)
was started with StateRoot enabled and verified archive import from height
`3,447,022` toward `3,452,022`. Startup correctly stopped before importing:

```text
authoritative state-pack marker exists; refusing to fall back to the stale MDBX MPT node namespace
```

The compact database intentionally omits the authoritative MPT node namespace,
and no current-format v5 pack for that marker is available. Clearing or
forging the marker would invalidate the replay, so it was not done. The
5,831.04 figure above is only a storage-component campaign and must not be
compared with the 2,000 StateRoot-enabled MainNet node requirement.

The latest valid optimistic-signature result remains the separate same-window
MainNet measurement of **255.04 to 346.63 blocks/s (+35.91%)** with StateRoot
enabled. It overlaps bounded header ECDSA preverification with ordered import,
but still executes the canonical NeoVM witness and never publishes state before
the verification fence.

## Verification

- `neo-state-packs`: 109 passed, 2 ignored subprocess helpers; doc tests pass.
- `neo-node`: 430 main-node tests, 44 database-probe tests, 7 pack-builder
  tests, 5 pack-verifier tests, and focused integration tests pass.
- Benchmark harness: 22 passed.
- StateService: 135 passed.
- Strict all-target/all-feature Clippy passes for `neo-state-packs` and
  `neo-node`; benchmark lib/bins Clippy passes with warnings denied.
- Formatting, layer-boundary tests (31 passed), strict OpenSpec validation,
  and `git diff --check` pass.

The benchmark crate's unrelated all-target Clippy invocation still requires
the absent generated VM contract-map fixture referenced by `vm_execution.rs`;
the changed benchmark library and binaries pass strict Clippy independently.
