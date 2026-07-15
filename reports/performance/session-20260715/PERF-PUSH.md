# Performance push beyond 2k blocks/s

## Profile (uncoord pruning h100k→300k, tmpfs)

| Metric | Pre | Post (3-run mean) | Δ |
|--------|----:|------------------:|--:|
| Overall blocks/s | 10,039.8 | **11,658.7** | **+16.1%** |
| Empty blocks/s | 37,936 | **49,536** | **+30.6%** |
| Transaction blocks/s | 1,666 | **1,831** | **+9.9%** |
| Dense 290–300k blocks/s | 1,604 | 1,607 | ~0% |

Official root @300k matched every run.

## Changes retained
1. **Empty-block fast-forward batch** 128 → **1024** — cuts per-chunk setup for ~182k empties.
2. **`load_script_bytes` / `Script::new_relaxed_from_slice`** — TX import avoids intermediate `Vec` clone of script bytes.
3. **Async MPT change batch cap** 8192 → **12,288** — fewer MDBX commits on dense windows while staying work-bounded.

## Dense-window residual hotspots
- TX `load_execute` ~314–327 µs/tx (still dominant on dense TX blocks)
- MPT apply ~3.8s / 10k dense + backing commit ~1.7s
- Finalization ~3.6s / 10k-batch (async flush)

Next candidates: engine reuse across txs, NeoToken PostPersist/candidate scan, trie finalization locality.
