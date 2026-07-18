## Context

The node executes application transactions through workspace `neo-vm` and
dispatches native-contract methods directly to Rust implementations. `Script`
already shares a segmented lazy instruction cache, and native method lookup is
already resolved by name, arity, hardfork, and binding index. A measured
MainNet window from height 1,877,001 through 1,887,000 spent about 9.02 seconds
executing 12,882 transactions and 94.94 seconds finalizing state. Execution is
therefore worth optimizing, especially for transaction-heavy adversarial
blocks, but it is not the controlling end-to-end bottleneck on that corpus.

Contract or method identity is not enough to cache an execution result. Gas,
faults, stacks, storage effects, notifications, calls, witness behavior, and
native state can depend on transaction, block, protocol, invocation, and prior
state. Contract hashes can also survive contract updates. Any short path that
omits one dependency can produce a consensus split.

## Goals / Non-Goals

**Goals:**

- Measure exact repeated script, entry, method, and control-flow shapes with
  bounded overhead and bounded-cardinality telemetry.
- Remove repeated decode, validation, branch-resolution, and dispatch work by
  caching immutable execution plans inside `neo-vm`.
- Allow a small number of explicitly modeled contract short paths to execute
  through the same gas, state, and effect interfaces as ordinary execution.
- Prove every candidate against sequential `neo-vm` before and during rollout.
- Speculate independent transactions in parallel while committing visible
  effects in canonical transaction order with deterministic retry.
- Keep caches, speculative work, and mismatch evidence bounded.

**Non-Goals:**

- Replacing `neo-vm`, reintroducing `neo-vm-rs` or `StackValue`, or converting
  stack graphs between engines.
- Caching stateful execution outputs by script, contract, method, or argument
  identity alone.
- Changing VM opcodes, gas schedules, native-contract behavior, transaction
  ordering, wire data, MPT serialization, or state roots.
- Claiming a hardware-independent 2,000 blocks/s guarantee or a 100x VM gain
  from the current MainNet profile.
- Parallel block publication or visibility out of canonical order.

## Decisions

### 1. Profile before selecting candidates

Add a disabled-by-default bounded heavy-hitter profiler at the execution host
boundary. A fingerprint includes exact script hash and byte identity, entry
instruction pointer, trigger, calling shape, network and protocol identity,
hardfork table, contract update identity when applicable, and coarse argument
types. It records calls, instructions, elapsed time, gas, faults, and selected
control-flow counters. Exact fingerprints belong in structured replay reports,
not unbounded Prometheus labels.

Alternative: hand-optimize well-known method names immediately. Rejected
because native methods already have direct dispatch and names do not establish
which work dominates or which scripts are stable.

### 2. Cache plans, not results

An immutable `ExecutionPlan` contains verified byte identity, decoded
instructions, basic-block boundaries, static branch targets, syscall metadata,
and only those call bindings whose full resolution dependencies are in the
plan key. A bounded concurrent cache stores plans by a versioned key and always
verifies exact bytes on lookup. Eviction changes performance only.

The first plan executor remains an interpreter over consensus micro-operations;
it is not a native-code JIT. Each operation uses the same fee charging,
instruction pointer, exception, reference counter, stack, diagnostic, and host
interfaces as the ordinary path.

Alternative: cache final stacks and writes. Rejected because complete state and
context dependency keys are generally as expensive and risky as execution.

### 3. Make specialized methods explicit effect programs

A contract short path is registered only for an exact script or native version,
entry, protocol range, and invocation shape. It declares every supported
argument form, storage point/range dependency, native-cache dependency, context
input, gas step, possible fault, and emitted effect. It reads and writes through
the normal execution host and effect journal; it cannot return a precomputed
stateful result.

Unknown inputs, changed script bytes, contract updates, hardfork changes,
unsupported diagnostics, dynamic calls, or undeclared host access select the
ordinary `neo-vm` path before effects become visible.

Alternative: maintain a second optimized engine. Rejected because duplicated
stack and host models create conversion cost and a second consensus surface.

### 4. Use sequential `neo-vm` as a differential oracle

Shadow mode runs the candidate and ordinary executor against separate overlays
from the same immutable snapshot. Comparison uses a canonical execution
artifact containing VM state, gas consumed, fault identity, result and
diagnostic stacks, invocation/call information, storage reads and range reads,
puts and deletes, notifications, logs, native-cache changes, and witness-visible
behavior. The ordinary result remains authoritative in shadow mode.

A mismatch records the first bounded reproducer, latches the candidate off for
the process, and fails strict replay. Production routing is candidate-specific,
version-specific, opt-in, and retains an immediate global kill switch.

### 5. Speculate transactions on versioned overlays

Transactions may execute concurrently from a pinned block-prefix snapshot.
Each job produces an execution artifact, exact point read set including absent
reads, range/prefix read observations, external context dependencies, and an
isolated write/effect set. Results are considered in canonical transaction
order. Before applying transaction N, validation compares its dependencies with
all committed prefix changes and relevant range generations.

If validation succeeds, its already computed artifact is applied in order. If
it conflicts, uses an unsupported host operation, exceeds a bound, or cannot
prove a dependency, transaction N executes sequentially against the current
prefix. Later jobs remain invisible and are revalidated against that outcome.

Alternative: static access-list scheduling. Rejected because Neo scripts can
compute keys dynamically and native contracts can perform range reads.

### 6. Preserve one effect representation

Ordinary, planned, specialized, and speculative execution all produce or apply
the existing `neo-vm` stack items and execution-layer cache/effect types. There
is no `StackItem` to `StackValue` graph conversion boundary. The comparison
layer serializes artifacts only for equality checks and durable evidence, not
for production execution handoff.

### 7. Bound all acceleration state

Configuration bounds plan bytes and entries, profiler candidates, shadow jobs,
speculative transactions, overlay bytes, range observations, mismatch samples,
and worker count. Backpressure or sequential fallback occurs at every hard
limit. Cache construction is single-flight per key, panic-isolated, and cannot
hold storage locks while compiling.

### 8. Promote from measured end-to-end evidence

Gates proceed from profiler-only, plan-cache benchmark, candidate shadow,
candidate opt-in, optimistic shadow, and optimistic opt-in. Each gate runs unit,
property, official C# fixture, adversarial conflict, staged MainNet, reopen,
and exact state-root comparisons. Reports separate useful work, compile cost,
shadow cost, conflict/retry waste, VM time, finalization, and persistence.

The performance target names CPU, memory, storage, filesystem, durability,
height range, transaction density, cache state, and percentile. Empty-block
throughput is reported separately.

## Risks / Trade-offs

- **[A plan survives a protocol or contract update]** -> Include protocol,
  hardfork, exact bytes, and update identity in keys; verify bytes and invalidate
  generations on every relevant update.
- **[A specialization omits a dependency or effect]** -> Require a declared
  effect contract, host-access auditing, shadow execution, fail-closed routing,
  and candidate-specific kill switches.
- **[A range read misses a phantom conflict]** -> Record range boundaries and
  snapshot generation/version evidence; unsupported iterators retry
  sequentially.
- **[Speculation consumes more resources than it saves]** -> Bound workers and
  bytes, track useful versus wasted work, and disable automatically only through
  deterministic operator policy rather than timing-dependent consensus logic.
- **[Profiling perturbs the hot path]** -> Keep it disabled by default, use
  sampled thread-local counters with bounded merging, and measure its overhead.
- **[Artifact comparison changes semantics]** -> Compare isolated overlays and
  keep sequential output authoritative; serialization is never on the
  production result path.
- **[Optimization distracts from persistence]** -> Preserve separate storage
  campaigns and report Amdahl limits from every replay window.

## Migration Plan

1. Land profiling and bounded metrics disabled by default.
2. Collect high-height and adversarial traces and select candidates by total
   cost, stability, and model completeness.
3. Add immutable plan generation and benchmark it without authoritative use.
4. Add one candidate specialization in shadow mode and run differential and
   staged MainNet campaigns through every applicable hardfork.
5. Enable that exact candidate only by explicit configuration after all gates
   pass; retain ordinary `neo-vm` fallback and kill switches.
6. Add optimistic execution in shadow mode, then opt-in mode after conflict,
   crash, replay, and root-parity gates pass.
7. Promote defaults only from repeatable end-to-end evidence. Rollback disables
   routing and discards rebuildable caches; no persistent state migration is
   required.

## Open Questions

- Which high-cost script and entry fingerprints dominate transaction-heavy
  MainNet ranges after persistence no longer controls wall time?
- Whether a micro-operation plan beats the existing lazy instruction cache
  enough to justify its code and audit surface.
- Which storage range-generation primitive gives exact phantom detection with
  acceptable overhead for optimistic validation.
- Whether native-cache reads need per-entry versions or a conservative global
  native-state generation in the first optimistic implementation.
- What conflict-rate and p99-latency thresholds should disable optimistic mode
  for a declared operator profile.
