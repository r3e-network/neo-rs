# Rejected: reuse OnPersist engine for PostPersist

## Change
Hold the OnPersist `ApplicationEngine` across the TX stage and rebind via
`prepare_for_post_persist` instead of constructing a second engine.

## A/B (uncoord h100k→300k, 3 runs vs callflags baseline ~12.28k)
| Metric | Control | Cand | Δ |
|--------|--------:|-----:|--:|
| Overall | 12,284 | 12,257 | **−0.22%** |
| TX | 1,972 | 1,972 | ~0% |
| Dense | 1,735 | 1,730 | −0.3% |

Roots matched. `postpersist_us` ~22 µs (already cheap). **Rejected / reverted**.
