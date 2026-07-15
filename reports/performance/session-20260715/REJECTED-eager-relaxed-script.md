# Rejected: eager parse for relaxed scripts

## Hypothesis
Pre-parse well-formed relaxed scripts into `InstructionCache::Eager` to remove
per-opcode `RwLock` on TX/contract execute.

## Result (3-run uncoord h100k→300k vs engine-reuse control)
| Metric | Control | Candidate | Δ |
|--------|--------:|----------:|--:|
| Overall | 11,784 | 10,772 | **−8.6%** |
| TX | 1,857 | 1,578 | **−15.0%** |
| Dense | 1,630 | 1,365 | **−16.2%** |
| execute_us | ~320 | ~420–500 | worse |

Roots still matched. Cause: relaxed path is also used for full NEF contract
scripts; eager full-body parse dominates method-local execution.

## Decision
**Rejected / reverted.**

## Follow-up: TX-only eager (`new_relaxed_eager_from_slice` on load_script_bytes)

Contract NEFs stayed lazy. 3-run vs engine-reuse control: overall **−0.04%**,
TX **−0.19%**, dense **−0.67%** (noise). execute_us ~313–318 vs ~320 control.
**Rejected / reverted** — not a clear win.
