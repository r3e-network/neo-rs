# Rejected MPT knobs (session continue)

Control/candidate share engine-reuse + surrounding WIP; only the named knob differs.
Official root `@300k` matched every run.

## Deferred node finalization flag (`Trie::new` defer_node_finalization)

| | Eager control mean | Deferred cand mean | Δ |
|--|--:|--:|--:|
| Overall | 11,773 | 11,854 | **+0.7%** |
| Dense | 1,632 | 1,633 | +0.05% |
| TX | 1,867 | 1,865 | −0.1% |

Within noise; dense/TX flat. **Not retained as a measured win** (WIP may still land for correctness/batch path later).

## ASYNC_MAX_APPLY_BATCH_CHANGES 12,288 → 24,576

| | Control (12k) | Cand (24k) | Δ |
|--|--:|--:|--:|
| Overall | 11,854 | 11,717 | **−1.2%** |
| Dense batch_blocks_avg | ~555 | ~900 | larger batches, slower wall |

**Rejected / reverted** to 12,288.

## Residual
TX load_execute ~315–325 µs still dominates dense; finalization ~3.6–3.9 s.
