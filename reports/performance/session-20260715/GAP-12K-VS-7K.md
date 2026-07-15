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
