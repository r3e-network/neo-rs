## Context

ADR-045 already establishes the workspace `neo-vm` crate as the sole interpreter and `StackItem` as the sole mutable VM value model. A follow-up comparison with official Neo v3.10.1 (`d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d`) and Neo.VM v3.10.1 (`004cd6070a940405818d9357638277dd44407e2e`) found mismatches in script construction, end-of-script return handling, control-flow bounds, fault cleanup, and several public VM operations.

Some mismatches are historically consensus-sensitive. Before MainNet `HF_Basilisk` at height 4,120,000, deployed contract scripts were not required to pass strict whole-script validation. Unreachable malformed bytes could therefore exist in accepted scripts, while `System.Runtime.LoadScript` was strict in every hardfork era.

## Goals / Non-Goals

**Goals:**

- Match official v3.10.1 behavior for the confirmed VM and ApplicationEngine findings.
- Preserve historical hardfork behavior needed to replay MainNet from genesis.
- Cover each correction with a focused regression that fails under the previous Rust behavior.
- Keep protocol authority and the runtime object graph unambiguous.

**Non-Goals:**

- Reintroducing `neo-vm-rs`, `StackValue`, or a second interpreter as a differential runtime.
- Claiming complete MainNet compatibility from focused unit tests alone.
- Optimizing sync throughput before execution and state-root correctness gates pass.
- Changing Neo wire formats or inventing Rust-specific protocol behavior.

## Decisions

### 1. Keep one VM and compare it to official source and fixtures

The local `neo-vm` remains the only executable VM. Differential evidence comes from upstream source-derived fixtures and, later, recorded C# oracle vectors rather than executing a second Rust interpreter in production. This avoids restoring two semantic authorities or a lossy runtime-value conversion boundary.

### 2. Make script validation mode explicit at the loading boundary

Relaxed scripts use a lazy instruction cache and decode only reached instructions. Strict scripts parse and validate the entire byte sequence before a context is loaded. Contract loading selects the hardfork-appropriate mode; `System.Runtime.LoadScript` always constructs a strict script. Call sites must express the mode explicitly rather than relying on a misleading default constructor.

### 3. Centralize context bounds without weakening jump bounds

An execution context accepts positions in `0..=script.len()`. Position `script.len()` represents end-of-script and executes the synthetic `RET`; positions greater than the length fault. Calls, exception targets, cloned contexts, and initial positions use that checked context rule. The opcode-level `JMP` helper retains C#'s stricter `0..script.len()` rule. `CALL` to exactly the script end is valid and immediately performs normal return-count validation.

### 4. Route synthetic operations through normal handlers

End-of-script uses the ordinary `RET` handler so return-count checks, context hooks, result propagation, and instruction accounting cannot diverge. Similar fixes should reuse normal opcode paths instead of duplicating partial behavior in the engine loop.

### 5. Finalize ApplicationEngine faults before exposing artifacts

When execution enters `FAULT`, the ApplicationEngine captures the exception and clears all notifications before callers can build `ApplicationExecuted` artifacts. Nested context rollback remains responsible for snapshot and per-context counters; top-level fault finalization provides the C# `ApplicationEngine.OnFault` guarantee.

### 6. Test observable behavior, including mutation ordering

Tests assert VM state, invocation/result stacks, notification output, exception text where protocol tooling exposes it, and whether operands remain on the stack after validation failures. Tests use bytecode that distinguishes strict whole-script parsing from lazy reached-instruction decoding.

## Risks / Trade-offs

- **[Historical scripts depend on relaxed parsing]** -> Keep relaxed construction lazy and add a pre-Basilisk unreachable-trailing-byte regression.
- **[Making pointer setters fallible touches many control-flow paths]** -> Introduce one checked context API and migrate callers with focused `CALL`, `TRY`, `ENDTRY`, and initial-position tests.
- **[Fault cleanup can erase diagnostics]** -> Clear notifications only; retain the captured exception, VM stacks, gas, and trace data.
- **[Unit parity can be mistaken for chain parity]** -> Keep full MainNet replay and state-root agreement as separate, explicit release gates.
- **[Concurrent refactoring obscures regressions]** -> Run focused crate suites first, then locked workspace checks after the dirty cutover tree settles.

## Migration Plan

1. Land regression tests for the confirmed mismatches.
2. Correct script modes, control-flow bounds, and fault cleanup in small commits or reviewable batches.
3. Correct lower-level API parity findings and add mutation-order tests.
4. Run NeoVM and ApplicationEngine suites, then the v3.10.1 consistency checks.
5. Run recorded block/state vectors and staged MainNet replay before changing compatibility claims.

Rollback is code-only: revert an individual semantic batch together with its tests. No persisted database migration is introduced by this change.

## Open Questions

- Which official C# execution vectors should become the minimum CI corpus before Phase 3 closes?
- Should historical malformed-script fixtures be embedded directly or generated from a pinned upstream test harness?
- What checkpoint cadence gives useful MainNet replay evidence without making normal pull-request CI impractical?

## Upstream Source Ledger

[`upstream-sources.md`](upstream-sources.md) maps every corrected behavior to
the immutable Neo or Neo.VM v3.10.1 source used by its regression.
