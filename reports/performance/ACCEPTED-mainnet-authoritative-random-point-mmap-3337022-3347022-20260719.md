# Accepted opt-in random point mmap, 3,337,023..3,347,022

## Decision

Retain the separate `MADV_RANDOM` pack/index mappings as an opt-in
authoritative-pack accelerator. The MainNet window completed with exact roots
and reduced trie store-resolution time from 10.556788 seconds in the adjacent
instrumented baseline to 1.550941 seconds. This is a 6.807x stage reduction;
normalized per store hit, 497.305 us fell to 69.221 us (7.184x).

The option remains disabled by default. This was not a same-range A/B, and the
node still reached only 384.473 blocks/s, far below the declared 1,500-2,000
blocks/s promotion target.

## Candidate

- Normal immutable mappings remain authoritative for sorted batch reads,
  validation, scrub, and compaction.
- Exact point reads use separate mappings carrying `MADV_RANDOM` only when
  `[storage.state_packs].random_point_mmap=true`.
- The two mappings share the kernel page cache and do not duplicate pack files.
- Advice failure is fatal only when the operator explicitly enables the option;
  the default path does not call `madvise`.
- The setting changes no frame, index, manifest, marker, lookup ordering,
  tombstone, snapshot, or StateRoot bytes.

## Reproducibility

- Binary: `target/release/neo-node`
- Binary SHA-256:
  `955c3dcd664a9675fba5eb42b009a5b8479c60f7788ee142504810b3c5915dcb`
- Configuration:
  `data/neo-v3101-staged-replay/neo_mainnet_validate_authoritative_packs.toml`
- Durable MDBX:
  `data/neo-v3101-staged-replay/runs/authority-ab-h3287022`
- Authoritative pack:
  `data/neo-v3101-staged-replay/authoritative-packs-3287022`
- Chain archive:
  `data/neo-v3101-staged-replay/fast-sync-cache/chain.0.acc/chain.0.acc`
- Node log:
  `reports/performance/mainnet-authoritative-random-mmap-3337022-3347022-20260719-node.log`
- Parsed profile:
  `reports/performance/mainnet-authoritative-random-mmap-3337022-3347022-20260719-profile.json`
- Process evidence:
  `reports/performance/mainnet-authoritative-random-mmap-3337022-3347022-20260719-time.txt`

The run used durable MDBX, coordinated full-state StateService, authoritative
packs, deferred full-state finalization, strict execution-specialization
Shadow, and no concurrent index compaction. Live runs increased from four to
eight during the four canonical commit windows.

## Correctness

- Imported blocks: 10,000 (`3,337,023..3,347,022`).
- Empty / transaction blocks: 7,647 / 2,353.
- Transactions: 3,175.
- Projected changes: 57,281.
- StateService attempts / failures: 10,000 / 0.
- Deferred lookup errors: 0.
- Local root at 3,347,022:
  `0xbea7a27414b14c19a88925e71ef947eb0581b04dd148c29c138d357154ea6d42`.
- Fresh `getstateroot(3347022)` calls to `seed1.neo.org:10332` and
  `seed2.neo.org:10332` returned that exact root.
- Mandatory marker: epoch 247, frame end 54,801,224,862.
- Independent post-marker scrub: 248 frames, 222,866,029 puts, zero
  tombstones, 54,801,207,006 payload bytes; the current root node was reachable.
- Verifier evidence:
  `reports/performance/pack-authority-random-mmap-postmarker-3347022-20260719.log`.

An opt-in no-op reopen before the replay reached a readable node in 0.65
seconds with 24 major faults and 846,936 KiB peak RSS.

## Measurements

| Metric | Instrumented baseline | Random mmap candidate | Change |
|---|---:|---:|---:|
| Blocks/s | 335.611 | 384.473 | 1.146x |
| Finalization/store | 21.652 s | 17.765 s | 1.219x |
| MPT mutation | 10.929 s | 1.851 s | 5.904x |
| Trie store resolution | 10.557 s | 1.551 s | 6.807x |
| Trie store hits | 21,228 | 22,406 | corpus |
| Resolution per store hit | 497.305 us | 69.221 us | 7.184x |
| Deferred sorted lookup | 4.851 s | 9.748 s | next hotspot |
| MDBX coordinated commit | 5.051 s | 5.380 s | 0.939x |
| VM transaction execute | 5.842 s | 5.894 s | comparable |
| Filesystem input (512-byte blocks) | 44,137,552 | 28,627,080 | -35.14% |
| Major faults | 30,046 | 35,477 | +18.08% |
| Peak RSS | 2,435,212 KiB | 3,393,684 KiB | +39.36% |

The higher fault count with lower physical input is consistent with disabling
large random-read readahead, but it is not by itself a causal proof. RSS and
whole-window throughput remain corpus- and cache-state-sensitive and are not
promotion evidence.

## Next bottleneck

Point resolution is no longer the dominant mutation cost. The candidate spent
9.748 seconds in sorted deferred lookup and 15.700 seconds in backing
publication. The next measured work is a true newest-run-first sorted merge
plus stage-level physical-read and fault counters, followed by another exact
MainNet root/reopen/scrub campaign.
