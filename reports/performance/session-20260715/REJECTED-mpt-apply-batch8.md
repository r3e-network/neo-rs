# Rejected: MPT apply-batch blocks = 8 (default)

**Date:** 2026-07-15  
**Hypothesis:** Large `FAST_SYNC_MPT_APPLY_BATCH_BLOCKS` (4096 = burst capacity)
pins multi-MB MPT overlays that thrash the TX execute core’s caches, inflating
`load_execute` from ~300µs → ~750µs.

**Change:** `try_new_async_with_limits(..., 4096)` →
`try_new_async_with_capacity(...)` (default apply batch = 8).

## A/B (uncoord dual-DB MPT, h100k→300k, tmpfs, 2 runs)

| Metric | Control (batch≈large, current tree) | Candidate mean (batch=8) | Δ |
|--------|------------------------------------:|-------------------------:|--:|
| Overall blocks/s | ~7,495 | **4,167** | **−44%** |
| Dense 290–300k | ~741 | **740** | ~0% |
| TX blocks/s | ~840–920 | **862** | noise |
| Empty blocks/s | ~50k | **~13.2k** | **−74%** |
| load_execute_us | ~748–800 | **782** | no recovery |
| batch_blocks_avg | hundreds–thousands | **8** | confirmed |

Official root@300k: **MATCH** both runs  
(`0xf0e121ac28b2de37e7e0321b0e0ef926f8b1ff9e85ee6f03c0c6c242a5f31088`).

## Verdict

**REJECT and keep large apply batch (4096).** Small batches massively increase
MDBX commit frequency / MPT queue wait on the empty-heavy window, crushing
overall and empty throughput without recovering dense `load_execute`.

Gap to ~12k remains pure `execute` (~2–2.5×), not MPT apply-batch size.
