# Engine reuse across multi-tx blocks

## Change
Multi-tx blocks reuse one `ApplicationEngine` between transactions:
- `ExecutionEngine::reset_execution_session` — clear stacks/counters, keep jump table + interop
- `ApplicationEngine::prepare_next_transaction` — rebind container/cache/gas, clear per-tx bookkeeping
- `native_persist` transactions loop takes/puts the engine around each TX

## A/B (uncoord pruning h100k→300k, tmpfs, 3 runs)

Control = post empty-FF/script/async-cap mean (~11.7k overall).

| Metric | Control | Candidate mean | Δ |
|--------|--------:|---------------:|--:|
| Overall blocks/s | 11,658.7 | **11,784.0** | **+1.1%** |
| TX blocks/s | 1,831.1 | **1,856.6** | **+1.4%** |
| Dense 290–300k | 1,607.4 | **1,629.7** | **+1.4%** |
| Empty blocks/s | 49,536 | 50,749 | +2.4% (noise) |
| native_tx_us (dense window) | 385 | **369** | −4% |
| load_execute_us | 327 | 325 | ~flat |

Official root `@300k` = `0xf0e121ac…f31088` **MATCH** all 3 runs. MPT failures = 0.

## Decision
**Retain** — consistently faster with root parity. Gain is modest because most early blocks are 1-tx; multi-tx dense windows see the engine-create path drop inside the aggregate `native_tx_us`.

## Residual hotspots (dense)
- TX `load_execute` still ~320–330 µs/tx (dominant)
- MPT dense window apply ~3.2s / 10k + backing commit ~1.3s + finalization ~3.6s
- NeoToken committee candidate scan still appears in onpersist profiles
