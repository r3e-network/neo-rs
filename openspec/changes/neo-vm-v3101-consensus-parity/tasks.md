## 1. Baseline And Authority

- [x] 1.1 Verify the production dependency graph contains only workspace `neo-vm` and no `neo-vm-rs`, `StackValue`, or runtime graph-conversion path
- [x] 1.2 Record the official Neo and Neo.VM v3.10.1 source locations used by every behavioral regression

## 2. Script And Control-Flow Semantics

- [x] 2.1 Route implicit end-of-script return through the normal `RET` handler and test exact `RVCount` enforcement
- [x] 2.2 Make relaxed scripts lazily decode reached instructions and add a pre-Basilisk unreachable-malformed-byte regression
- [x] 2.3 Make `System.Runtime.LoadScript` construct and validate a strict script and test invalid jump and `CONVERT Any` inputs
- [x] 2.4 Enforce inclusive `0..=script.len()` context bounds while retaining strict `< script.len()` jump-opcode bounds
- [x] 2.5 Add regressions for `CALL` to script end and `TRY` or `ENDTRY` targets beyond script end

## 3. Fault And VM Operation Semantics

- [x] 3.1 Clear ApplicationEngine notifications on `FAULT` before execution artifacts are exposed and test `Notify` followed by `ABORT`
- [x] 3.2 Preserve `Null` in Map, Pointer, and InteropInterface `StackItem::convert_to` calls
- [x] 3.3 Emit `PACKSTRUCT` when the script builder pushes a Struct
- [x] 3.4 Validate slot and index operands before popping the source value in slot-store operations
- [x] 3.5 Preserve invocation frames when an unhandled throw faults and add diagnostic-state regressions
- [x] 3.6 Decode `ABORTMSG` with strict UTF-8 and cover valid and invalid message bytes

## 4. Verification And Replay Evidence

- [x] 4.1 Run focused `neo-vm`, `neo-execution`, and execution-artifact test suites
- [x] 4.2 Run dependency-hygiene, v3.10.1 consistency, formatting, and OpenSpec validation checks
- [x] 4.3 Add recorded C# differential fixtures covering every corrected semantic across applicable hardfork tables
- [x] 4.4 Replay staged MainNet ranges including pre-Basilisk history and compare execution artifacts and state roots to official checkpoints
- [ ] 4.5 Complete full MainNet replay and record the final matching state-root evidence before declaring v3.10.1 compatibility complete
