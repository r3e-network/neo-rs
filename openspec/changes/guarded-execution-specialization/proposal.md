## Why

MainNet profiling shows repeated application scripts and a small native-contract
surface, but consensus execution cannot safely cache effects using only a
contract or script identity because results depend on protocol version,
transaction context, invocation state, and storage. We need evidence-driven
execution acceleration that keeps sequential `neo-vm` as the semantic oracle
and proves every optimized path against exact artifacts and state roots.

## What Changes

- Add bounded execution fingerprints and frequency/cost profiles keyed by exact
  script bytes, entry point, trigger, protocol/hardfork identity, and invocation
  shape.
- Add immutable, protocol-versioned compiled execution plans that eliminate
  repeated decode, dispatch, and method-resolution work without memoizing
  stateful outputs.
- Permit narrowly scoped native or script short paths only when their complete
  dependency and effect contract is explicit and differential shadow execution
  proves gas, faults, stacks, storage, notifications, calls, and witnesses.
- Add bounded optimistic transaction execution with isolated overlays, exact
  read/write sets, ordered validation and commit, and deterministic sequential
  retry on conflicts or unsupported behavior.
- Add kill switches, bounded caches, promotion gates, mismatch evidence, and
  declared-corpus benchmarks. No optimized path is authoritative by default.
- Keep workspace `neo-vm` as the sole VM implementation and fallback; this does
  not introduce `neo-vm-rs`, `StackValue`, or graph-conversion boundaries.

## Capabilities

### New Capabilities

- `guarded-execution-specialization`: Trace-guided compiled plans, exact
  specialization eligibility, differential shadowing, cache bounds, protocol
  invalidation, and promotion requirements.
- `optimistic-transaction-execution`: Speculative overlay isolation, conflict
  validation, ordered commit, sequential fallback, resource bounds, and parity
  gates for parallel transaction execution.

### Modified Capabilities

None.

## Impact

The change affects `neo-vm`, `neo-execution`, `neo-native-contracts`, block
persistence orchestration, execution metrics, replay tooling, and benchmarks.
It adds no wire-format, RPC, MPT-serialization, state-root, gas-schedule, or
contract-behavior changes. Production routing remains sequential until bounded
MainNet shadow runs and adversarial differential suites pass.
