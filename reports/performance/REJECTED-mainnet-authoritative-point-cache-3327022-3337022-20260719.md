# REJECTED: authoritative exact payload-offset cache, 3,327,023..3,337,022

Date: 2026-07-19

## Decision

Reject the 64 MiB exact immutable payload-offset cache. At the final height it
recorded 54 hits and 21,474 misses across 21,528 reads: a 0.250836% hit rate.
It held 21,474 entries using 6,515,220 bytes, with zero evictions and zero
oversized bypasses. Capacity was not the limitation; exact `(offset, length)`
identities almost never repeated across these MainNet trie reads.

The cache therefore cannot materially reduce the dominant authoritative
point-read cost. Its code should not be promoted. This rejection rests on the
direct hit-rate evidence and does not depend on a throughput comparison.

## Correctness

- Imported 10,000 blocks to height 3,337,022 with 10,000 / 0 StateService
  apply attempts/failures.
- Local and public StateRoot:
  `0xdff84355d7176620006c6a94d66b71130076c6a8422cf3d2d66761cf5df282d6`.
- Fresh `getstateroot(3337022)` calls to `seed1.neo.org:10332` and
  `seed2.neo.org:10332` returned that exact height and root.
- Trie store hits/misses: 21,527 / 0.
- Mandatory authority marker: epoch 243, frame end 54,656,506,818, block
  3,337,022. Its internal little-endian root bytes reverse to the public root
  above.
- Full scrub: 244 frames, 222,324,859 puts, zero tombstones,
  54,656,489,250 payload bytes; the current root node was reachable.
- Independent authority verification exited successfully.

## Cache Evidence

| Metric | Result |
| --- | ---: |
| Maximum capacity | 67,108,864 bytes |
| Resident bytes | 6,515,220 |
| Entries | 21,474 |
| Hits | 54 |
| Misses | 21,474 |
| Total reads | 21,528 |
| Hit rate | 0.250836% |
| Evictions | 0 |
| Oversized bypasses | 0 |

## Contaminated Performance Evidence

The node reported 33.806030 seconds of import time and 295.805 blocks/s, but
this window is not a valid causal speed comparison. The starting marker's
independent verifier opened 16 live index runs; the post-window verifier opened
only 4. Background index compaction therefore ran during the campaign.

The process wall clock was 61.34 seconds, 27.53 seconds longer than the node's
internal import timer. Peak RSS reached 16,627,760 KiB, filesystem inputs were
57,480,032, filesystem outputs were 6,950,184, and 31,007 major page faults
occurred. Those values include compaction and shutdown interference. They must
not be used to claim that the cache caused either the 295.805 blocks/s result
or the regression relative to adjacent windows.

Future point-read campaigns must isolate or explicitly schedule index
compaction. The next candidate should target random mmap/readahead behavior or
lookup layout directly, with the exact cache absent.

## Evidence

- `mainnet-authoritative-point-cache-3327022-3337022-20260719-node.log`
- `mainnet-authoritative-point-cache-3327022-3337022-20260719-profile.json`
- `mainnet-authoritative-point-cache-3327022-3337022-20260719-time.txt`
- `pack-authority-trie-resolve-postmarker-3327022-20260719.log`
- `pack-authority-point-cache-postmarker-3337022-20260719.log`
- `pack-authority-point-cache-postmarker-3337022-20260719-time.txt`
