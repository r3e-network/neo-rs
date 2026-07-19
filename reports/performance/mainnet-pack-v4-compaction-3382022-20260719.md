# MainNet v4 index compaction and verified GC at height 3,382,022

Date: 2026-07-19

## Result

The bounded streaming compactor completed one production-scale merge over the
MainNet authoritative state pack. It moved generation 19 with 24 v3 runs to
generation 20 with eight retained v3 L0 runs and one v4 L2 run. The merge read
224,024,920 input records, emitted 223,996,291 winner records, and wrote
11,591,808,326 bytes in 98.661 seconds.

The semantic gate passed. The full winner digest, complete committed-frame
scrub, sampled frame references, bounded indexed reads and misses, root
reachability, index checksums, post-write state, and a clean reopen all agreed.
A separate evidence-gated GC invocation then deleted 272 obsolete run files and
19 obsolete manifests, reclaimed 23,618,405,564 bytes (21.996 GiB), reopened
the pack, and reproduced the same evidence exactly.

This is a successful compaction and GC correctness result. It is not a node
blocks-per-second result, and its roughly 6 GiB peak RSS remains a resource
optimization target.

## Reproducibility

- Network: Neo N3 MainNet, magic `0x334F454E`.
- Canonical marker height: `3,382,022`.
- MDBX metadata/ledger authority:
  `data/neo-v3101-staged-replay/runs/ab-search-h3377022`.
- Original authoritative pack:
  `data/neo-v3101-staged-replay/authoritative-packs-3287022`.
- Isolated campaign pack:
  `data/neo-v3101-staged-replay/runs/streaming-compaction-v4-pack`.
- Base checkpoint height: `3,287,022`.
- Release verifier SHA-256:
  `eca5f636a4f771f0f8ac7f00763d9b611820f613bf77b50f19b87279f45bcc6c`.
- Final read-only revalidation commit and verifier SHA-256:
  `a24f4314414b0048bbce9844bcd5bcec2096f088` and
  `7c825a14d34d9fc044c5e56e6fef8294f33accdc4b7bfeefdb1d2440e9143259`.
- Host: VMware guest, 8 vCPUs reported as Intel Core Ultra 9 285K,
  67,379,171,328 bytes RAM, Linux 6.17.0-35-generic.
- Storage: 2,147,483,648,000-byte VMware virtual rotational disk, ext4 on
  `/dev/sda2`, mounted `rw,relatime`.

Both mutating commands used a 1 GiB compaction workspace limit, complete index
scrubs, deterministic lookup evidence, random-advised point mmaps, mandatory
marker authority, and clean reopen verification:

```text
env NPV_MDBX=data/neo-v3101-staged-replay/runs/ab-search-h3377022 \
  NPV_PACK=data/neo-v3101-staged-replay/runs/streaming-compaction-v4-pack \
  target/release/neo-pack-verify --mode authority \
  --network-magic 0x334F454E --max-index-memory-mb 1024 \
  --samples 0 --walk-cap 1 --lookup-digest-samples 100000 \
  --scrub-indexes --random-point-mmap --maintain

env NPV_MDBX=data/neo-v3101-staged-replay/runs/ab-search-h3377022 \
  NPV_PACK=data/neo-v3101-staged-replay/runs/streaming-compaction-v4-pack \
  target/release/neo-pack-verify --mode authority \
  --network-magic 0x334F454E --max-index-memory-mb 1024 \
  --samples 0 --walk-cap 1 --lookup-digest-samples 100000 \
  --scrub-indexes --random-point-mmap --gc
```

## Corpus

| Measure | Value |
| --- | ---: |
| Authority epoch / frames | 263 / 264 |
| Committed frame end | 55,392,359,055 bytes |
| Fully scrubbed frame rows | 225,073,562 |
| Fully scrubbed frame payload | 55,392,340,047 bytes |
| Materialized winners / puts | 225,009,452 / 225,009,452 |
| Tombstones | 0 |
| Winner value bytes | 46,831,056,072 |
| StateRoot | `0xd5f0b14d87ceaed5a4219d783f4ed592101438488dea6178684ac08a5d4ef939` |
| Marker payload SHA-256 | `f499135f867280b9e49278b0ba3a077520fd10040747734d1d21650776ea896d` |

The root node was reachable before maintenance, after maintenance, after clean
reopen, before GC, and after the post-GC reopen. Every verifier invocation
finished with `authority verification: ok (mandatory marker)`.

## Semantic evidence

The promotion evidence below was identical in the split baseline, maintenance
pre-state, candidate generation, post-state, clean reopen, pre-GC state, and
post-GC reopen.

| Evidence | Count | SHA-256 |
| --- | ---: | --- |
| Canonical winner records | 225,009,452 | `11f8de24ef72e495fd7c3c30e976c3bc6792e2243fb00a817176297141fba87e` |
| Frame-reference sample over full scrub | 100,000 | `da867229e5aaa982da926baf14af495fbe12b1c1005f568872146d652f7ddf36` |
| Promotion lookup sample | 4,096 | `351b31be8b9a6240fb87b5ec9c0c2262151fca76acd6fc7243f323f3ff4663f3` |
| Promotion sample key set | 4,096 | `243e083c0f08070b589680392125a3d2fff5832aca62a6e252263b7eb31317a2` |

The lookup gate returned 4,096 present values in five byte-bounded batches,
performed 4,096 independent point reads, and checked 256 deterministic absent
keys. The frame-reference phase checksum-validated all 264 frames and all
225,073,562 committed rows before resolving its sample.

The earlier pre-split baseline sampled 100,000 lookup keys and produced lookup
SHA-256
`5b97308750b6079e3fe03e2ec458936cc014e8e836fdd5741caa0b402280c408`.
Its sample-key SHA-256 was
`3ac9f6580d7debe73aea9dc754651d839c2a2a846047922b56959d821fdfb20c`.
Those lookup hashes differ because the later promotion gate deliberately uses
a 4,096-key lookup subset; the complete winner and frame-reference hashes are
unchanged.

## Compaction

| Measure | Before | After |
| --- | ---: | ---: |
| Manifest generation | 19 | 20 |
| Live runs | 24 | 9 |
| v3 / v4 runs | 24 / 0 | 8 / 1 |
| Live source records | 225,048,697 | 225,020,068 |
| Index record bytes | 11,252,434,850 | 11,251,003,400 |
| Decoded live-index metadata | 609,889,650 B | 58,776,406 B |
| Excess runs | 8 | 0 |

The selected merge consumed 16 runs and retained eight L0 runs. It removed
28,629 obsolete versions, reduced live-run count by 62.5%, and published one
immutable v4 L2 run. The measured compaction stage was:

| Measure | Value |
| --- | ---: |
| Workspace estimate / hard bound | 1,002,588,690 / 1,073,741,824 B |
| Input / output records | 224,024,920 / 223,996,291 |
| Output bytes | 11,591,808,326 |
| Stage wall time | 98.660965537 s |
| Input rate | 2,270,654 records/s |
| Output rate | 2,270,364 records/s |
| Output write rate | 117,491,333 B/s |

The authority verifier scrubbed all 24 input runs before mutation, scrubbed all
nine post-maintenance runs before closing, reopened generation 20, and scrubbed
all nine runs again. Pre/post/reopen semantic evidence matched exactly.

## Verified GC

GC ran only after generation 20 passed the semantic and reopen gate. It deleted
272 obsolete run files and 19 obsolete manifests and reclaimed
23,618,405,564 bytes. The live generation remained 20 with eight v3 runs and
one v4 run. A clean post-GC reopen repeated the full evidence and index scrub;
pre/post-reopen evidence matched exactly.

The observed post-GC directory contains 14 regular files and occupies
67,038,135,749 logical bytes (62.434 GiB):

- `frames.pack`: 55,392,359,055 bytes.
- Nine live index files: 11,645,773,694 bytes including headers and filters.
- One generation-20 manifest, checkpoint metadata, and the writer lock.
- Generation-20 manifest SHA-256:
  `cd2ef3f74cba33a3e93125916b0e44ef3edb30ddda5ff737d15fd3e2822f9c47`.
- Checkpoint SHA-256:
  `cb648e630968c35c36fb89119416cc5be534e3caf0d5ab9d21991b33121f13f6`.

## Process evidence

GNU `time -v` filesystem counts below are Linux 512-byte units. They are
process-attributed observations, not complete filesystem journal or device
traffic.

| Run | Wall | Max RSS | Major faults | FS input | FS output |
| --- | ---: | ---: | ---: | ---: | ---: |
| Pre-split baseline | 158.65 s | 11,280,984 KiB | 22,840 | 355,158,016 | 48 |
| Split-evidence baseline | 91.78 s | 5,922,464 KiB | 37,964 | 68,784,840 | 40 |
| Maintain + all gates | 488.54 s | 6,412,972 KiB | 151,264 | 387,253,920 | 22,640,384 |
| Verified GC + all gates | 194.06 s | 5,592,588 KiB | 77,571 | 171,266,864 | 72 |
| Final binary, read-only source pack | 83.26 s | 36,480,316 KiB | 40,106 | 131,684,320 | 24 |

The maintain invocation's filesystem output is 11,591,876,608 bytes after the
512-byte conversion, closely tracking the reported 11,591,808,326-byte v4 run.
Most of the 488.54-second process wall is the repeated pre/candidate/post/reopen
evidence and three index scrubs; the streaming merge itself took 98.66 seconds.

## Final binary read-only revalidation

After the promotion changes and final tombstone-offset scrub fix were committed,
the release verifier at commit
`a24f4314414b0048bbce9844bcd5bcec2096f088` reopened the retained generation-19
MainNet source pack in read-only verification mode. It scrubbed all 24 v3 runs
and 225,048,697 records, resolved the canonical root, merge-walked every winner,
scrubbed all 264 frames and 225,073,562 frame rows, replayed 100,000 frame
references, and exercised 4,096 bounded lookup winners plus 256 misses.

All canonical hashes and counts exactly matched the campaign evidence. The run
finished with `authority verification: ok (mandatory marker)` in 83.26 seconds;
winner merge, frame-reference, and lookup phases took 25.424, 36.549, and 0.937
seconds respectively. It performed no maintenance, GC, or pack mutation.

The run's `ru_maxrss` was 36,480,316 KiB (34.790 GiB), substantially above the
5.648 GiB split-evidence baseline despite identical semantics and lower wall
time. This confirms that verifier mmap residency still varies materially with
cache state and host memory pressure. The low-memory gate remains failed; no
bounded-RSS claim is made from the lower earlier samples.

## Limitations

- This campaign did not import blocks or execute transactions. It proves the
  compaction and GC state-preservation boundary, not 1,500-2,000 blocks/s or
  any other node throughput target.
- Maintenance peaked at 6,412,972 KiB (6.116 GiB) RSS and GC at 5,592,588 KiB
  (5.334 GiB). The 1 GiB limit bounds compaction workspace, not every resident
  mmap page and verifier structure. Final-binary read-only revalidation peaked
  at 36,480,316 KiB (34.790 GiB), so further RSS work is required.
- After activation the mandatory marker makes the pack authoritative, so MDBX
  `0xf0` node comparison is intentionally skipped. This gate combines marker
  identity, root reachability, complete winner/frame evidence, bounded lookup
  evidence, checksummed index scrubs, and clean reopen; it does not rerun an
  independent full MDBX state namespace digest.
- The complete winner digest covers every materialized key and the frame scrub
  covers every committed row, while direct lookup proof is bounded to 4,096
  present keys and 256 deterministic misses.
- Timing, RSS, and fault counts depend on cache state and this particular
  VMware/ext4 host. They are not hardware-independent guarantees.
- The resulting manifest intentionally mixes one v4 L2 run with eight v3 L0
  runs. It proves compatible incremental migration, not a full rewrite.
- This MainNet run did not induce a power loss. The separate deterministic
  fault-injection matrix covers publication crash boundaries.

## Evidence files

- `mainnet-pack-v4-pre-3382022-20260719.log`
- `mainnet-pack-v4-pre-3382022-20260719-time.txt`
- `mainnet-pack-v4-split-evidence-3382022-20260719.log`
- `mainnet-pack-v4-split-evidence-3382022-20260719-time.txt`
- `mainnet-pack-v4-maintain-3382022-20260719.log`
- `mainnet-pack-v4-maintain-3382022-20260719-time.txt`
- `mainnet-pack-v4-maintain-3382022-20260719-pidstat.txt`
- `mainnet-pack-v4-gc-3382022-20260719.log`
- `mainnet-pack-v4-gc-3382022-20260719-time.txt`
- `pack-compaction-workspace-guard-20260719.md`
