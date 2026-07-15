# Lock-free call-flag checks + shared implicit RET

## Change
1. `ApplicationEngine::has_call_flags` / `get_current_call_flags` use VM-synced
   `ExecutionEngine` flags (maintained on context load/unload and script load)
   instead of locking `ExecutionContextState` on every syscall.
2. Syscall dispatch checks `engine.has_call_flags` directly.
3. End-of-script implicit RET reuses a process-wide `LazyLock<Arc<Instruction>>`
   instead of `Arc::new(Instruction::ret())` per frame exit.

## A/B (uncoord h100k→300k, tmpfs, 3 runs)

Control = post-FxHash mean (~12.18k overall).

| Metric | Control | Candidate mean | Δ |
|--------|--------:|---------------:|--:|
| Overall blocks/s | 12,181 | **12,284** | **+0.85%** |
| TX blocks/s | 1,960 | **1,972** | **+0.60%** |
| Dense 290–300k | 1,722 | **1,735** | **+0.73%** |
| load_execute_us | ~312–325 | **~302–314** | slightly lower |

All three overall runs above control mean. Official root `@300k` **MATCH**.

## Decision
**Retain.**
