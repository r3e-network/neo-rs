# Performance gap: historical ~12k vs current ~7.5k (2026-07-15)

## Confirmed
Older release binaries on **this machine, same harness, same baseline, same
chain.acc, dual-DB uncoord** still hit historical throughput:

| Binary | mtime | overall | dense | load_execute_us |
|--------|-------|--------:|------:|----------------:|
| vm-stack-attach control | 10:15 | **11,304** | **1,622** | **328** |
| mpt-prefetch | 15:09 | **11,867** | **1,632** | **361** |
| callflags c1 (log only) | 18:54 | **12,248** | **1,737** | **314** |
| current `target/release` | 21:18 | **7,495** | **741** | **748** |

Root MATCH for current. Empty-block path is unchanged (~50k empty b/s).
`engine_create_us=15` on fast and slow (reuse present either way).
`load_script_us≈15` both. **Gap is pure `execute` (~2.0–2.5×).**

Fee skip experiment: light heights (~150k) load_execute ~30µs with or without
opcode fees — fee path is not the dense-window 2.5×.

## Implication
Regression is in **source built after the morning/midday binaries**, not the
machine, not dual-DB config, not root correctness.

## Next (priority)
1. Identify git SHA / dirty tree of `mpt-prefetch-20260715-150850/bin/neo-node`
   (or vm-stack-attach control) and rebuild from that exact tree.
2. Or binary-search by swapping crates: rebuild with morning-era `neo-vm` +
   `neo-execution` only against current node/storage.
3. Dense-only microbench: record `instructions_executed` + wall ns on a fixed
   TX set under both binaries.

## Interim retained wins (still valid)
- Dual-DB `coordinated=false` path
- post_execute host gate  
- Warm contract/script cache across TXs (+~2% on 7.5k tree)
- Lock-free call flags (restored)

## 2026-07-15 evening update

### MPT apply-batch=8 A/B — REJECTED
- Candidate mean overall **4,167** (−44% vs ~7.5k), empty ~13k (−74%), dense/load_execute
  unchanged (~740 / ~780µs). Root MATCH.
- See `REJECTED-mpt-apply-batch8.md`. Kept `FAST_SYNC_MPT_APPLY_BATCH_BLOCKS=4096`.

### Known-fast binary reconfirm (same harness)
Binary: `data/neo-v3101-staged-replay/runs/mpt-prefetch-20260715-150850/bin/neo-node`
(BuildID `00d56e15…`, mtime 15:09)

| Metric | Fast reconfirm | Current ~7.5k tree |
|--------|---------------:|-------------------:|
| Overall | **11,154** | ~7,500 |
| Dense 290–300k | **1,573** | ~740 |
| TX blocks/s | **1,804** | ~860 |
| Empty | **50,101** | ~50k |
| load_execute_us | **318** | ~750 |
| execute_us | **318** | ~750 |
| engine_create_us | 15 | 15 |

Confirms gap is **not machine/harness/root** — same tmpfs dual-DB uncoord chain.acc.

### Source provenance notes
- Fast binary has `NEO_MPT_PREFETCH_BATCH_KEYS` string; **not present** in current HEAD or stashes.
- No git commit between morning and `f9960ebd` (17:28); 15:09 binary is **dirty-tree** WIP.
- Since `c8bc5877`, committed `neo-vm` jump_table / evaluation_stack are unchanged;
  only `script.rs`, execute/host/call-flags, engine reuse, and execution crates moved.
- Recovering the exact dirty tree (or re-landing MPT prefetch + any uncommitted VM
  hot-path WIP) is still the primary path to ~12k.
