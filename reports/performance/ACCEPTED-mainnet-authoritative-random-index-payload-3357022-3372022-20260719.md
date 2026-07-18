# Accepted random-advised sparse index and payload lookup, 3,357,023..3,372,022

Date: 2026-07-19

## Decision

Retain separate `MADV_RANDOM` mappings for indexed sparse reads as an opt-in
authoritative-pack accelerator. The accepted architecture applies random
advice to both sparse index probes and the payload reads located by those
indices. Ordinary mappings remain available for sequential frame validation,
full scrub, and compaction.

The first 10,000-block window changed sparse index probes while leaving the
offset-sorted batch payload reads on the ordinary mapping. It reached 8.136
us/key and read 8,372 bytes/key during deferred lookup. The following
5,000-block window also moved those indexed batch payload reads to the
random-advised mapping. It reached 3.698 us/key and read 76.25 bytes/key. Its
measured 20,807,680 read bytes equal exactly 5,080 4 KiB pages, matching the
5,080 major faults and showing that the stage no longer triggered excess
physical readahead.

These are adjacent, different MainNet corpora rather than a same-range A/B.
The normalized read reduction is therefore strong operational evidence, not a
claim of a controlled 109.8x speedup. The latest whole-window result was
510.731 blocks/s, still far below the declared 1,500-2,000 blocks/s promotion
target.

The option remains disabled by default. It changes mmap advice only; it does
not change frame bytes, lookup precedence, tombstones, snapshots, markers,
recovery, or StateRoot computation.

## Reproducibility

Shared campaign inputs:

- Configuration:
  `data/neo-v3101-staged-replay/neo_mainnet_validate_authoritative_packs.toml`
- Durable MDBX:
  `data/neo-v3101-staged-replay/runs/authority-ab-h3287022`
- Authoritative pack:
  `data/neo-v3101-staged-replay/authoritative-packs-3287022`
- Chain archive:
  `data/neo-v3101-staged-replay/fast-sync-cache/chain.0.acc/chain.0.acc`

Sparse-index window (`3,357,023..3,367,022`):

- Binary SHA-256:
  `7d3537176213525d464ee82a1b4e56929f26adfa1f32c78332f569595d31c149`
- Node log:
  `reports/performance/mainnet-authoritative-random-index-batch-3357022-3367022-20260719-node.log`
- Parsed profile:
  `reports/performance/mainnet-authoritative-random-index-batch-3357022-3367022-20260719-profile.json`
- Process evidence:
  `reports/performance/mainnet-authoritative-random-index-batch-3357022-3367022-20260719-time.txt`

Sparse-index-and-payload window (`3,367,023..3,372,022`):

- Binary SHA-256:
  `4f659ce51338b1766ed55d81eec36a5ef4a7e1b536c4670bc6b5f54773487923`
- Node log:
  `reports/performance/mainnet-authoritative-random-index-payload-3367022-3372022-20260719-node.log`
- Parsed profile:
  `reports/performance/mainnet-authoritative-random-index-payload-3367022-3372022-20260719-profile.json`
- Process evidence:
  `reports/performance/mainnet-authoritative-random-index-payload-3367022-3372022-20260719-time.txt`

Both runs used durable MDBX, coordinated full-state StateService,
authoritative packs, deferred full-state finalization, and strict
execution-specialization Shadow.

## Correctness

Across both windows:

- Imported blocks: 15,000 (`3,357,023..3,372,022`).
- Transactions: 5,051.
- Projected changes: 82,574.
- StateService attempts / failures: 15,000 / 0.
- Deferred lookup errors: 0.
- No Shadow mismatch or infrastructure failure appeared in either node log.

At height 3,367,022:

- Public StateRoot:
  `0x9dacc11cb7933574c6b604514abfdc875a69ea30ccb517708c45f945318f7b5c`.
- Fresh `getstateroot(3367022)` calls to both public seeds returned that exact
  height and root.
- Mandatory marker: epoch 257, frame end 55,171,662,051.
- Independent reopen and scrub: 258 frames, 10 live runs, 224,253,903 puts,
  zero tombstones, 55,171,643,475 payload bytes; the current root was
  reachable.

At height 3,372,022:

- Public StateRoot:
  `0x86a5c2b884b7f04bfbb87e9d52fad86f0af2c7e75704853dcb090ca3931549b1`.
- Fresh `getstateroot(3372022)` calls to `seed1.neo.org:10332` and
  `seed2.neo.org:10332` returned that exact height and root.
- Mandatory marker: epoch 259, frame end 55,245,398,814.
- Independent reopen and scrub: 260 frames, 12 live runs, 224,527,664 puts,
  zero tombstones, 55,245,380,094 payload bytes; the current root was
  reachable.
- Both authority verifier runs returned `ok (mandatory marker)`.

Marker roots are stored in internal little-endian byte order; reversing those
bytes yields the public StateRoots above.

## Measurements

| Metric | Random sparse index | Random sparse index + payload |
| --- | ---: | ---: |
| Blocks | 10,000 | 5,000 |
| Transactions | 3,347 | 1,704 |
| Projected changes | 53,519 | 29,055 |
| Import time | 19.740669 s | 9.789881 s |
| Blocks/s | 506.568 | 510.731 |
| Finalization/store | 12.527290 s | 5.430970 s |
| Deferred journal keys | 521,215 | 272,887 |
| Deferred sorted lookup | 4.240778 s | 1.009184 s |
| Lookup time per key | 8.136 us | 3.698 us |
| Lookup-stage physical reads | 4,363,452,416 B | 20,807,680 B |
| Lookup-stage reads per key | 8,371.69 B | 76.25 B |
| Lookup-stage major / minor faults | 8,255 / 1,571 | 5,080 / 574 |
| Trie store resolution | 2.303521 s | 1.203724 s |
| MPT mutation | 2.601812 s | 1.366543 s |
| Backing publication | 9.709589 s | 3.956487 s |
| MDBX coordinated commit | 4.941933 s | 2.687178 s |
| VM transaction execute | 5.021599 s | 3.146353 s |
| Process filesystem input | 10,206,832 x 512 B | 1,589,144 x 512 B |
| Peak RSS | 3,458,196 KiB | 2,816,472 KiB |

The final window's stage read count is only 0.91% of the 2.285 GB that would be
expected at the preceding window's normalized 8,371.69 bytes/key. Because the
corpora differ, this calculation describes avoided read amplification rather
than end-to-end speedup.

## Remaining Bottlenecks

Sparse deferred lookup is no longer the dominant finalization stage. In the
latest 5,000-block window, backing publication took 3.956487 seconds and the
coordinated MDBX commit took 2.687178 seconds. MDBX overlay visitation consumed
2.195107 seconds, cursor writes consumed 2.175817 seconds for 19,771 entries,
and durable commit consumed 0.491678 seconds. VM transaction execution still
consumed 3.146353 seconds.

The next performance work should target authoritative publication and MDBX
metadata/Ledger writes without weakening the marker-before-publication rule.
This acceptance does not satisfy the sustained throughput, adversarial
recovery, or production promotion gates.

## Evidence

- `mainnet-authoritative-random-index-batch-3357022-3367022-20260719-node.log`
- `mainnet-authoritative-random-index-batch-3357022-3367022-20260719-profile.json`
- `mainnet-authoritative-random-index-batch-3357022-3367022-20260719-time.txt`
- `pack-authority-random-index-batch-postmarker-3367022-20260719.log`
- `pack-authority-random-index-batch-postmarker-3367022-20260719-time.txt`
- `mainnet-authoritative-random-index-payload-3367022-3372022-20260719-node.log`
- `mainnet-authoritative-random-index-payload-3367022-3372022-20260719-profile.json`
- `mainnet-authoritative-random-index-payload-3367022-3372022-20260719-time.txt`
- `pack-authority-random-index-payload-postmarker-3372022-20260719.log`
- `pack-authority-random-index-payload-postmarker-3372022-20260719-time.txt`
