## ADDED Requirements

### Requirement: Isolated speculative execution
The system SHALL execute speculative transactions against pinned immutable
snapshots and isolated overlays. Speculative results SHALL remain invisible
until validated and applied in canonical block transaction order.

#### Scenario: A later transaction finishes first
- **WHEN** transaction N+1 finishes before transaction N
- **THEN** N+1 effects SHALL remain invisible and SHALL NOT commit before N

### Requirement: Complete dependency and effect capture
Each speculative result SHALL capture exact present and absent point reads,
range and prefix reads, contract and native-cache dependencies, protocol,
transaction and block context dependencies, and all writes and externally
observable effects needed to validate equivalence.

#### Scenario: A transaction reads an absent key
- **WHEN** an earlier canonical transaction creates that key before validation
- **THEN** the absent-read dependency SHALL conflict and the speculative result
  SHALL NOT be applied

#### Scenario: A transaction scans a storage prefix
- **WHEN** an earlier canonical transaction inserts or deletes a row in the
  observed prefix
- **THEN** range validation SHALL detect the phantom or conservatively require
  sequential re-execution

### Requirement: Deterministic ordered validation
Before applying transaction N, the system SHALL validate every captured
dependency against the canonically applied prefix. Validation decisions SHALL
depend only on deterministic state versions and artifacts, not worker timing.

#### Scenario: Speculative dependencies remain valid
- **WHEN** no applied prefix effect changes any captured dependency
- **THEN** the system SHALL apply the exact speculative artifact at position N
  with the same result as sequential execution

#### Scenario: A dependency conflicts
- **WHEN** any captured dependency changed or cannot be proven unchanged
- **THEN** transaction N SHALL execute sequentially against the current prefix
  and only that sequential result SHALL be applied

### Requirement: Sequential fallback for unsupported behavior
The system SHALL mark dynamic or host behavior whose complete dependency and
effect set cannot be captured as unsupported before speculative effects become
visible and SHALL execute it sequentially.

#### Scenario: Execution uses an unsupported iterator or interop
- **WHEN** dependency capture cannot prove its complete read domain
- **THEN** the transaction SHALL retry sequentially and canonical progress SHALL
  continue

### Requirement: Exact artifact application
Applying a validated speculative artifact SHALL reproduce sequential VM state,
gas, faults, stacks, storage and native-cache changes, notifications, logs,
calls, invocation counters, and witness-visible behavior without reordering or
duplicating an effect.

#### Scenario: A speculative transaction faults
- **WHEN** its dependencies validate at its canonical position
- **THEN** the same fault artifact and applicable committed or discarded effects
  as sequential execution SHALL be applied

### Requirement: Bounded speculation and backpressure
The system SHALL bound worker count, queued transactions, snapshots, overlay
bytes, read and range observations, artifacts, and wasted speculative work.
Reaching a hard bound SHALL pause speculation or use sequential execution.

#### Scenario: Speculative overlays reach the byte bound
- **WHEN** outstanding overlay bytes equal the configured maximum
- **THEN** the producer SHALL apply backpressure or execute sequentially without
  allocating an unbounded queue

### Requirement: Cancellation and failure containment
A worker error, panic, cancellation, timeout, or invalid artifact SHALL NOT
publish partial effects. The canonical executor SHALL retain a deterministic
sequential path for the affected transaction and remaining block.

#### Scenario: A speculative worker panics
- **WHEN** the worker terminates before returning a validated artifact
- **THEN** its overlay SHALL be discarded and the transaction SHALL execute
  sequentially without changing the canonical result

### Requirement: Optimistic differential shadowing
Before authoritative use, optimistic mode SHALL compare every accepted and
retried outcome with a complete sequential block execution from the same
starting snapshot, including final execution artifacts, cache dumps, and state
root. The sequential block result SHALL remain authoritative in shadow mode.

#### Scenario: Parallel and sequential blocks diverge
- **WHEN** any transaction artifact, final cache, or state root differs
- **THEN** the run SHALL fail with the first bounded reproducer and optimistic
  authority SHALL remain disabled

### Requirement: Observable optimistic efficiency
The system SHALL report bounded-label counts and time for useful speculation,
conflicts, sequential retries, unsupported fallbacks, worker idle time, queue
and byte high-water marks, validation, and ordered commit. Performance claims
SHALL include conflict-heavy and transaction-heavy declared corpora.

#### Scenario: Optimistic execution achieves high throughput on independent work
- **WHEN** the same implementation regresses conflict-heavy blocks or moves the
  bottleneck entirely to persistence
- **THEN** reports SHALL expose that cost and SHALL NOT present independent-work
  throughput as a worst-case guarantee
