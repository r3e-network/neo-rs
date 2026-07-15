# Call-flags restore + warm contract cache (2026-07-15 continue)

## Problem
After dual-DB uncoord restore, uncoord h100k→300k sat at **~7.5k overall /
~740 dense / ~740µs load_execute** vs retained callflags control **~12.3k /
~1.73k / ~300µs** (official root MATCH either way).

## Finding 1: lock-free call flags had been reverted
`ApplicationEngine::has_call_flags` / `get_current_call_flags` were locking
`ExecutionContextState` every syscall instead of using VM-synced
`ExecutionEngine::has_call_flags` from e06bcec1.

**Restored** lock-free path. Same-tree re-measure: still ~7.5k (neutral vs the
reverted form on this tree — syscall volume is not the whole 2.5× gap).

## Finding 2: warm contract/script cache across TXs in a block
`prepare_next_transaction` no longer clears `contracts` / `contract_scripts`.
Updates still go through `put_contract_cache` / `remove_contract_cache`.

### A/B (uncoord dual-DB, h100k→300k, tmpfs, 3 runs, root MATCH)

| Metric | Same-tree control | Warm-cache mean | Δ |
|--------|------------------:|----------------:|--:|
| Overall b/s | 7,491 | **7,632** | **+1.9%** |
| TX b/s | 897 | **918** | **+2.4%** |
| Dense 290–300k | 738 | **753** | **+2.0%** |

All three overall runs above control mean. **Retain.**

## Open: 12.3k → 7.5k gap
Historical callflags logs (`/tmp/.../callflags-ret/ab/c1.log`) show load_execute
**314µs** and overall **12.2k** with the same toml shape on this machine earlier
today. Current binary still **~750µs / ~7.5k** after:
- lock-free call flags
- post_execute host gate
- dual-DB uncoord
- `target-cpu=native` rebuild (no help)
- e06 DataCache checkout (worse: ~2k overall — keep current DataCache)

VM jump_table / script / evaluation_stack are **identical** to e06bcec1.
`e06bcec1` itself does not clean-build (stale `execution_profile` refs +
`BTreeSet::drain`). The 12.3k binary was a dirty tree.

Next probes:
1. Recover/replay the exact dirty tree that produced callflags c1.log.
2. Instruction-count + ns/instruction on dense TX samples.
3. Host `pre_execute` fee path: eliminate HostPtr per-instruction dispatch
   (monomorphized fee charge on the engine).
