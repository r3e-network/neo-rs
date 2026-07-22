## 1. Trace Evidence And Candidate Selection

- [x] 1.1 Add disabled-by-default bounded execution fingerprints covering exact script bytes, entry, trigger, protocol, hardfork, update identity, invocation shape, instructions, gas, faults, calls, and elapsed time
- [x] 1.2 Export bounded structured replay profiles and low-cardinality aggregate metrics without script hashes as Prometheus labels
- [x] 1.3 Add determinism, capacity, concurrency, disabled-path, and profiler-overhead tests and enforce a measured overhead budget
- [x] 1.4 Collect transaction-heavy MainNet and adversarial traces, publish the top cost-stable candidates, and record the VM and end-to-end Amdahl limits

## 2. Immutable Execution Plans

- [x] 2.1 Define a versioned plan key with exact byte verification and every network, protocol, hardfork, entry, and contract-resolution dependency
- [x] 2.2 Build immutable verified basic-block and consensus micro-operation plans without changing `neo-vm` stack, gas, exception, diagnostic, or host types
- [x] 2.3 Add a byte- and entry-bounded concurrent single-flight plan cache whose eviction and construction races cannot change results
- [x] 2.4 Add an opt-in plan executor with ordinary `neo-vm` fallback before effects on any unsupported operation, bound, error, or panic
- [x] 2.5 Differentially test planned and ordinary execution across opcodes, faults, calls, diagnostics, applicable hardfork tables, and randomized scripts
- [x] 2.6 Benchmark warm, cold, repeated, branch-heavy, syscall-heavy, and MainNet plan execution and retain the plan layer only if it materially beats the existing lazy instruction cache

## 3. Guarded Contract Specialization

- [x] 3.1 Define a candidate registry and effect contract for exact version, eligibility, arguments, context, state and range dependencies, gas, faults, and effects
- [x] 3.2 Add host-access auditing that rejects undeclared reads, ranges, native-cache access, calls, writes, notifications, logs, and witness-visible effects
- [x] 3.3 Implement one trace-selected high-cost candidate through existing `neo-vm` stack items and execution-layer state interfaces without output memoization
- [x] 3.4 Implement canonical complete execution-artifact comparison over VM state, gas, faults, stacks, calls, invocation counters, storage, native caches, notifications, logs, diagnostics, and witnesses
- [x] 3.5 Add candidate-specific shadow routing, bounded mismatch reproducers, latched disablement, global and candidate kill switches, and strict-replay failure
- [x] 3.6 Run official fixtures, adversarial state changes, contract updates, hardfork boundaries, and bounded MainNet shadow replay before any opt-in authoritative candidate

## 4. Optimistic Execution Foundations

- [x] 4.1 Add pinned block-prefix snapshots and isolated per-transaction overlays using the existing execution value and effect representation
- [x] 4.2 Capture exact present and absent point reads, writes, context dependencies, and native-cache dependencies for each speculative artifact
- [x] 4.3 Add exact range and prefix generation validation or conservatively mark unsupported iterators for sequential execution
- [x] 4.4 Apply a validated artifact exactly once with sequentially equivalent gas, faults, stacks, cache effects, notifications, logs, calls, invocations, and witness behavior
- [x] 4.5 Add property tests for missing reads, read-write and write-write conflicts, phantoms, faults, native state, dynamic calls, and unsupported-host fallback

## 5. Bounded Ordered Scheduler

- [ ] 5.1 Implement a worker-, queue-, snapshot-, overlay-byte-, artifact-, and observation-bounded speculative scheduler
- [ ] 5.2 Validate completed artifacts and expose effects strictly in canonical transaction order independently of worker completion order
- [ ] 5.3 Re-execute conflicts and uncertain artifacts sequentially against the current prefix and revalidate every later speculative result
- [ ] 5.4 Add backpressure, cancellation, panic isolation, deterministic block fallback, and shutdown behavior with no partial publication
- [ ] 5.5 Expose useful and wasted work, conflicts, retries, unsupported fallbacks, validation and commit time, idle time, and queue and byte high-water marks

## 6. Differential Promotion Gates

- [ ] 6.1 Compare sequential, planned, specialized, and optimistic artifacts on deterministic randomized corpora across every v3.10.1 hardfork table
- [ ] 6.2 Extend official C# oracle fixtures for selected candidates, state changes, conflicts, faults, and effect ordering
- [ ] 6.3 Run independent, conflict-heavy, range-heavy, storage-heavy, and worst-case legal block campaigns with strict memory and latency bounds
- [ ] 6.4 Run bounded MainNet shadow replay through applicable history and compare transaction artifacts, cache dumps, roots, reopen state, and public checkpoints
- [ ] 6.5 Record candidate-specific speedups and named end-to-end hardware, filesystem, durability, corpus, conflict-rate, cache-state, and percentile results
- [ ] 6.6 Keep every acceleration mode disabled by default until its exact gate passes, document rollback and kill switches, and reverify the production dependency graph contains only workspace `neo-vm`
