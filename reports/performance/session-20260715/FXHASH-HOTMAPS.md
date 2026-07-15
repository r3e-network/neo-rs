# FxHashMap on DataCache + Script instruction cache

## Change
1. `DataCache` dictionary: `std::HashMap` → `rustc_hash::FxHashMap` (every Storage.Get/Put).
2. `Script` instruction cache (eager + lazy): `HashMap` → `FxHashMap` (every opcode decode/lookup).

## A/B (uncoord h100k→300k, tmpfs, 3 runs)

Control = post engine-reuse deferred-flag mean (~11.85k overall).

| Metric | Control | Candidate mean | Δ |
|--------|--------:|---------------:|--:|
| Overall blocks/s | 11,854 | **12,181** | **+2.8%** |
| TX blocks/s | 1,865 | **1,960** | **+5.1%** |
| Dense 290–300k | 1,633 | **1,722** | **+5.5%** |

Runs: overall 12,163 / 12,273 / 12,106. Official root `@300k` **MATCH** all 3.

## Decision
**Retain.** DataCache-only was noise; combining with instruction-map FxHash lands a clear dense/TX gain.

## Residual
TX `load_execute` still ~312–325 µs mean; finalization ~3.6 s class; MPT window apply still large.
