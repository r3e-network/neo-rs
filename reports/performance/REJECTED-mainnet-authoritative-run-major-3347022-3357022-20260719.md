# REJECTED: authoritative run-major sorted lookup, 3,347,023..3,357,022

Date: 2026-07-19

## Decision

Reject and revert the newest-run-first, run-major sorted lookup experiment. It
did not improve normalized deferred lookup time: the candidate used 18.626 us
per journal key, versus 18.080 us/key in the preceding key-major campaign. The
windows were not identical, so the 3.0% difference is not a causal regression;
it is nevertheless no evidence for retaining the additional merge algorithm.

More importantly, the new stage counters measured 25,213,120,512 bytes read
during lookup for 862,801 journal keys, or 29,222 bytes/key (28.538 KiB/key).
The experiment showed that lookup ordering was not the main problem. Sparse
index and payload probes were still using mappings with ordinary readahead,
causing large physical reads. The follow-up architecture therefore kept the
existing key-major semantics and targeted mmap advice instead.

The reported 228.085 blocks/s must not be compared directly with the adjacent
384.473 blocks/s campaign. This window contained 8,326 transactions and 87,977
projected changes, versus 3,175 transactions and 57,281 changes in the prior
window. The rejection rests on normalized lookup evidence and unnecessary
algorithmic complexity, not whole-window throughput.

## Candidate

- Traverse immutable index runs newest first and merge each run against the
  sorted journal keys.
- Preserve newest-epoch precedence, tombstones, duplicate-key coalescing, and
  the same pack/frame/marker formats.
- Avoid changing VM, StateRoot, publication, or recovery semantics.
- Retain ordinary mmap advice for both sparse index probes and indexed payload
  reads in this historical candidate.

The implementation was removed after this campaign. The simpler key-major
algorithm remains the semantic path.

## Reproducibility

- Binary: `target/release/neo-node`
- Binary SHA-256:
  `69e97f8bea48df851b10a2314baa7e5ba3bfc293cd6560d71a055ffb56cd6da1`
- Configuration:
  `data/neo-v3101-staged-replay/neo_mainnet_validate_authoritative_packs.toml`
- Durable MDBX:
  `data/neo-v3101-staged-replay/runs/authority-ab-h3287022`
- Authoritative pack:
  `data/neo-v3101-staged-replay/authoritative-packs-3287022`
- Chain archive:
  `data/neo-v3101-staged-replay/fast-sync-cache/chain.0.acc/chain.0.acc`
- Node log:
  `reports/performance/mainnet-authoritative-run-major-3347022-3357022-20260719-node.log`
- Parsed profile:
  `reports/performance/mainnet-authoritative-run-major-3347022-3357022-20260719-profile.json`
- Process evidence:
  `reports/performance/mainnet-authoritative-run-major-3347022-3357022-20260719-time.txt`

The run used durable MDBX, coordinated full-state StateService, authoritative
packs, deferred full-state finalization, opt-in random point mappings, and
strict execution-specialization Shadow.

## Correctness

- Imported blocks: 10,000 (`3,347,023..3,357,022`).
- Empty / transaction blocks: 5,703 / 4,297.
- Transactions: 8,326.
- Projected changes: 87,977.
- StateService attempts / failures: 10,000 / 0.
- Deferred lookup errors: 0.
- Public StateRoot at 3,357,022:
  `0x01a7fc5ed87fad667ebc0215f5678152d8ceb056ec7168dfb2702cd68a56dd2c`.
- Fresh `getstateroot(3357022)` calls to `seed1.neo.org:10332` and
  `seed2.neo.org:10332` returned that exact height and root.
- Mandatory marker: epoch 253, frame end 55,032,008,429. Its internal
  little-endian root bytes reverse to the public root above.
- Independent post-marker reopen and scrub: 254 frames, 6 live runs,
  223,731,156 puts, zero tombstones, 55,031,990,141 payload bytes; the current
  root node was reachable.
- Authority verifier result: `ok (mandatory marker)`.

## Measurements

| Metric | Result |
| --- | ---: |
| Blocks / transactions | 10,000 / 8,326 |
| Import time / throughput | 43.843279 s / 228.085 blocks/s |
| Finalization/store | 25.070122 s |
| Deferred journal keys | 862,801 |
| Deferred sorted lookup | 16.070508 s |
| Lookup time per key | 18.626 us |
| Lookup-stage physical reads | 25,213,120,512 bytes |
| Lookup-stage reads per key | 29,222 bytes (28.538 KiB) |
| Lookup-stage major / minor faults | 3,637 / 12,105 |
| Backing publication | 22.782317 s |
| MDBX coordinated commit | 5.839626 s |
| VM transaction execute | 14.084539 s |
| Process filesystem input | 50,936,512 x 512-byte blocks |
| Process major faults | 43,320 |
| Peak RSS | 4,796,572 KiB |

The prior key-major campaign spent 9.748074 seconds for 539,278 journal keys,
or 18.080 us/key. Since the corpora and live-run layouts differ, that
comparison is directional only. The direct stage I/O result is the stronger
finding: the access pattern was dominated by readahead amplification.

## Evidence

- `mainnet-authoritative-run-major-3347022-3357022-20260719-node.log`
- `mainnet-authoritative-run-major-3347022-3357022-20260719-profile.json`
- `mainnet-authoritative-run-major-3347022-3357022-20260719-time.txt`
- `pack-authority-run-major-postmarker-3357022-20260719.log`
- `pack-authority-run-major-postmarker-3357022-20260719-time.txt`
