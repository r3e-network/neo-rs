## ADDED Requirements

### Requirement: Bounded execution fingerprinting
The system SHALL profile repeated execution shapes only when explicitly enabled
and SHALL bound profiler memory, merge work, report size, and metric cardinality.
Fingerprints SHALL distinguish exact script bytes, entry point, trigger,
protocol and hardfork identity, applicable contract update identity, and
invocation shape.

#### Scenario: The same contract hash has updated script bytes
- **WHEN** an updated contract executes under the same contract hash
- **THEN** the profiler and every specialization cache SHALL treat it as a
  different execution identity

#### Scenario: The profiler reaches its candidate bound
- **WHEN** a new fingerprint arrives after the configured bound is reached
- **THEN** the profiler SHALL evict, aggregate, or sample it without allocating
  unbounded state or changing execution behavior

### Requirement: Immutable protocol-versioned execution plans
The system SHALL cache only immutable execution plans whose keys cover exact
script bytes and every protocol or resolution dependency. A cache hit SHALL
verify byte identity, and cache eviction or construction races SHALL affect
performance only.

#### Scenario: A hardfork changes instruction behavior
- **WHEN** the same script executes under a different applicable hardfork table
- **THEN** the system SHALL build or select a distinct plan and SHALL NOT reuse
  the incompatible plan

#### Scenario: Concurrent callers miss the same plan
- **WHEN** several executions request one absent plan concurrently
- **THEN** plan construction SHALL be bounded and race-free and every caller
  SHALL observe semantics identical to ordinary `neo-vm`

### Requirement: No stateful output memoization
An authoritative optimized path SHALL NOT reuse a final stack, gas result,
fault, storage delta, notification, call result, witness result, or other
stateful execution output solely from script, contract, method, or argument
identity. Specialized paths SHALL obtain state and context through the normal
execution host for each invocation.

#### Scenario: Identical arguments execute against changed storage
- **WHEN** a method is invoked with identical arguments after a relevant state
  value changes
- **THEN** the optimized path SHALL observe the new value and produce the same
  result and effects as ordinary sequential `neo-vm`

### Requirement: Explicit specialization eligibility
Each specialized method SHALL declare its exact script or native version,
protocol range, supported entry and argument forms, context inputs, state and
range dependencies, gas steps, faults, and possible effects. Any undeclared or
unsupported condition SHALL select ordinary `neo-vm` before optimized effects
become visible.

#### Scenario: A specialization encounters an unsupported argument form
- **WHEN** the invocation does not match every declared eligibility condition
- **THEN** execution SHALL fall back to ordinary `neo-vm` with no partial
  specialized effect

#### Scenario: A specialized path attempts undeclared host access
- **WHEN** host-access auditing observes a dependency or effect outside the
  specialization contract
- **THEN** strict shadow replay SHALL fail and authoritative routing for that
  candidate SHALL remain disabled

### Requirement: Complete execution semantic equivalence
Planned and specialized execution SHALL preserve VM state, instruction and
exception behavior, gas, faults, stacks, reference identity, calls, invocation
counters, storage reads and writes, notifications, logs, native-cache changes,
diagnostics, and witness-visible behavior exactly.

#### Scenario: Optimized execution faults after emitting effects
- **WHEN** the equivalent ordinary execution reaches `FAULT`
- **THEN** the optimized execution SHALL expose and discard effects exactly as
  ordinary `neo-vm` does for the applicable protocol version

### Requirement: Differential shadow authority
Shadow mode SHALL execute optimized and ordinary paths from the same immutable
snapshot in isolated overlays and SHALL compare canonical complete execution
artifacts. Ordinary sequential `neo-vm` SHALL remain authoritative in shadow
mode.

#### Scenario: Shadow artifacts match
- **WHEN** every compared field and isolated effect matches
- **THEN** the observation MAY count toward candidate promotion evidence but
  SHALL NOT make the optimized result authoritative in shadow mode

#### Scenario: Shadow artifacts diverge
- **WHEN** any compared semantic or effect differs
- **THEN** the system SHALL record a bounded first reproducer, latch the
  candidate off, and fail strict replay

### Requirement: Bounded caches and fail-closed controls
The system SHALL enforce byte and entry bounds for plans, profiles, shadow
artifacts, and mismatch evidence. It SHALL provide global and candidate-specific
kill switches and SHALL fall back sequentially on construction error, panic,
resource exhaustion, or uncertain eligibility.

#### Scenario: Plan construction exceeds its bound
- **WHEN** a plan exceeds a configured size or complexity limit
- **THEN** the system SHALL discard it and execute through ordinary `neo-vm`
  without changing consensus output

### Requirement: Evidence-based candidate promotion
A specialization SHALL remain disabled by default until official differential
fixtures, adversarial tests, applicable hardfork ranges, bounded MainNet shadow
replay, reopen, and exact state-root gates pass for its exact versioned identity.
Performance reports SHALL separate optimized VM time from finalization and
persistence time.

#### Scenario: A candidate is faster only in a microbenchmark
- **WHEN** end-to-end declared-corpus replay does not show a material gain or
  does not pass every correctness gate
- **THEN** the candidate SHALL NOT be promoted as a production default

### Requirement: Single VM value model
All execution paths SHALL use workspace `neo-vm` stack items and execution-layer
state and effect types directly. The production graph SHALL NOT add
`neo-vm-rs`, `StackValue`, or a stack-graph conversion boundary.

#### Scenario: A specialized result returns to block persistence
- **WHEN** an eligible specialized invocation completes
- **THEN** its result and effects SHALL already use the same representation as
  ordinary `neo-vm` and SHALL require no graph conversion
