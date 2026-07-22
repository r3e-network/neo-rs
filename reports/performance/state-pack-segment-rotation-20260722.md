# State-Pack Multi-Segment And Bounded-History Evidence

## Decision

Commit `94f853517fb629e99f394ca8c9c92ee3f719a52e` is accepted for
multi-segment correctness, crash recovery, bounded file history, and startup
cost. It is **not** accepted or described as a write-throughput optimization:
the identical durable component campaign changed from **7,908.34 to 7,685.70
represented blocks/s**, a **-2.82%** regression.

The candidate reduced reopen time by **66.94%**, campaign-end file count by
**99.61%**, allocated campaign storage by **34.58%**, and total campaign wall
time by **5.93%**. Those improvements do not establish node throughput. This
report contains **no StateRoot-enabled MainNet node BPS evidence** and does not
satisfy the 2,000 blocks/s node requirement.

## Reproducibility

- Baseline revision: `5edc17135c08af3144e374ebd0e659d14336b493`.
- Baseline binary SHA-256:
  `659eaea904982f7f4bddcd9d3e8c0810372c44fc9111295ec733320fc46307f4`.
- Candidate revision: `94f853517fb629e99f394ca8c9c92ee3f719a52e`.
- Candidate binary SHA-256:
  `35f5a0d718b4bbe9f6fe9f0910c7a9a89cff1e788637927216ebb0afaf9177a6`.
- Hardware: Intel Core Ultra 9 285K, 8 visible CPUs.
- Filesystem: ext4 shared VM volume.
- Durability: append `sync_data` plus directory fsync.
- Cache condition: warm after prefill.
- Workload: `neo-n3-mainnet-1877001-1887000`, seed
  `5640001189223798919`.
- Shape: 115,140,640 prefill rows, 1,007,960 operations, 10,000
  represented blocks, 8 durable campaign commits.

Baseline command:

```bash
/tmp/append-persistence-bench-baseline \
  --database /tmp/neo-statepacks-segment-ab/baseline-1-db \
  --output /tmp/neo-statepacks-segment-ab/baseline-1.json \
  --scale full \
  --hardware-profile intel-core-ultra-9-285k-8-vcpu \
  --filesystem-profile ext4-shared-vm-volume \
  --durability-profile append-sync-data-directory-fsync \
  --read-cache-state warm-after-prefill
```

Candidate command:

```bash
/tmp/append-persistence-bench-candidate-94f85351 \
  --database /tmp/neo-statepacks-segment-ab/candidate-94f85351-db \
  --output /tmp/neo-statepacks-segment-ab/candidate-94f85351.json \
  --scale full \
  --hardware-profile intel-core-ultra-9-285k-8-vcpu \
  --filesystem-profile ext4-shared-vm-volume \
  --durability-profile append-sync-data-directory-fsync \
  --read-cache-state warm-after-prefill
```

## Results

| Metric | Baseline | Candidate | Signed delta |
|---|---:|---:|---:|
| Represented blocks/s | 7,908.34 | 7,685.70 | -2.82% |
| Campaign wall | 1.264 s | 1.301 s | +2.90% |
| Campaign physical writes | 401,575,936 B | 401,588,224 B | +0.003% |
| Campaign peak RSS | 404,049,920 B | 500,805,632 B | +23.95% |
| Campaign-end regular files | 7,922 | 31 | -99.61% |
| Campaign-end allocated bytes | 61,976,268,800 B | 40,543,698,944 B | -34.58% |
| Prefill wall | 250.385 s | 256.878 s | +2.59% |
| Reopen and validation | 32.966 s | 10.899 s | -66.94% |
| Total wall | 286.125 s | 269.170 s | -5.93% |

The candidate performed 392 bounded GC batches during the full run, reclaiming
3,942 superseded runs, 3,959 manifests, and 21,412,426,656 bytes. The final
layout contains eight append segments, one manifest, and 18 live index runs.
The baseline deferred all reclamation to one final GC, which is why its
campaign measurement accumulated thousands of files.

## Correctness

Both arms produced identical prefill and campaign operation digests. The
candidate reopened 3,522 frames and 18 runs, verified 16,414 keys, and matched
the expected digest exactly:

```text
f6ee371feaec91b4ff77d88c5e8186410e37a7c93f06438779a9e9f7b82d0d34
```

The implementation also passed:

- `neo-state-packs`: 167 passed, 0 failed, 6 ignored.
- `neo-node` authoritative state-pack tests: 11 passed, 0 failed.
- Strict all-target/all-feature Clippy for `neo-state-packs` and `neo-node`.
- 31 architecture layer-boundary tests and 11 repository-hygiene tests.
- Strict OpenSpec validation and `git diff --check`.

Recovery coverage includes authenticated multi-segment selection, torn tails,
orphan suffixes, missing/corrupt derived runs, bounded binary-carry rebuild,
manifest-selected allocation preflight, generation overflow, and subprocess
crashes at staging sync, run promotion, run-directory sync, manifest sync, and
manifest rename. External-horizon crash tests also prove orphan suffix removal,
sentinel preservation, continued append, and reopen equivalence.

## Publication Diagnostic

The same candidate revision reports the successor-view ownership diagnostic as
`no throughput evidence`. Shared immutable segment prefixes avoid clone-all
work as segment count grows:

| Segments | Clone-all publications/s | Shared-prefix publications/s | Speedup |
|---:|---:|---:|---:|
| 1 | 28,583,018 | 29,689,068 | 1.04x |
| 64 | 1,924,606 | 29,734,606 | 15.45x |
| 256 | 499,751 | 29,980,197 | 59.99x |
| 1,024 | 126,207 | 29,968,236 | 237.45x |
| 4,096 | 31,495 | 29,947,293 | 950.85x |

This diagnostic proves the ownership shape is O(1) in mapped payload bytes and
O(number of sealed `Arc`s) only when rotation changes the prefix. It cannot be
converted into node BPS.

## Raw Evidence

- `state-pack-segment-rotation-20260722-baseline.json`, SHA-256
  `d8cb4060c5c42daeff0480ac1725bea31a8ae7828d0efa5ce5075d6b37433e0f`.
- `state-pack-segment-rotation-20260722-candidate.json`, SHA-256
  `db36c1bf3d9463c68ea8de6e597399e17fa0e112ab5a57b9e4a1c56739daa66f`.

The large temporary databases are not retained after report and digest
verification.
