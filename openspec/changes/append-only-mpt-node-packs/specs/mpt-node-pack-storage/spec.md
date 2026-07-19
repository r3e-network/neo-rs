## ADDED Requirements

### Requirement: Exact versioned MPT node storage
The system SHALL store every node-pack put and tombstone as an ordered version
of the existing `0xf0` MPT node namespace and SHALL preserve put values
byte-for-byte, including mutable reference-count bytes.

#### Scenario: A node hash receives a new reference count
- **WHEN** a later committed epoch puts a different serialized value for an existing node hash
- **THEN** every new snapshot SHALL return the later exact value while snapshots pinned before that epoch retain their prior view

#### Scenario: A node is deleted
- **WHEN** a committed pruning or unwind epoch writes a tombstone for a node hash
- **THEN** new snapshots SHALL report the node absent and compaction SHALL retain the tombstone while any older visible run contains that hash

### Requirement: Deterministic checksummed frame format
The system SHALL encode node operations in versioned bounded frames with block
range, previous and resulting roots, sorted row metadata, payload lengths,
checksums, and an unambiguous complete-frame footer.

#### Scenario: A frame is reopened
- **WHEN** a complete frame is read after process restart
- **THEN** its header, row index, payload, footer, roots, lengths, ordering, and checksums SHALL validate before any row is exposed

#### Scenario: A write tears before its footer
- **WHEN** startup encounters an incomplete frame suffix
- **THEN** recovery SHALL reject that suffix as uncommitted and SHALL preserve every preceding complete committed frame

### Requirement: Cold-first canonical publication
The system SHALL sync node-pack bytes before atomically committing the canonical
Ledger overlay, StateService root metadata, and pack high-water mark in MDBX.
Visible in-memory height and root SHALL advance only after that MDBX commit.

#### Scenario: Pack sync fails
- **WHEN** appending or syncing a prepared node-pack frame fails
- **THEN** the canonical MDBX transaction SHALL not commit and visible Ledger and StateService heights SHALL remain unchanged

#### Scenario: MDBX commit fails after pack sync
- **WHEN** the pack frame is durable but the canonical MDBX transaction fails
- **THEN** the frame SHALL remain unreachable orphan data and restart SHALL expose the preceding committed root

#### Scenario: Canonical commit succeeds
- **WHEN** the pack frame is durable and MDBX commits its matching marker
- **THEN** the resulting root SHALL become visible with every referenced node durably readable

### Requirement: Authoritative recovery and corruption handling
Startup SHALL reconcile packs and derived indexes to the MDBX high-water mark,
truncate or ignore uncommitted suffixes, rebuild missing derived indexes, and
fail closed if committed authoritative bytes are absent or corrupt.

#### Scenario: Crash after canonical marker before index publication
- **WHEN** the committed frame is valid but its derived index run is missing
- **THEN** startup SHALL rebuild the index from frame metadata before reporting the StateService ready

#### Scenario: Committed frame is missing
- **WHEN** the MDBX marker names a segment or frame that cannot be validated
- **THEN** startup SHALL fail with a corruption error and SHALL not fall back to an older or incomplete state implicitly

#### Scenario: Compaction output is incomplete
- **WHEN** a crash leaves an incomplete index-compaction output
- **THEN** startup SHALL discard that output and retain the complete source runs

### Requirement: Correct indexed lookup
The system SHALL provide point and sorted-batch reads with newest-committed-
version semantics across recent and compacted runs. Filters SHALL only remove
impossible runs and SHALL never synthesize an authoritative value.

#### Scenario: A hash has versions in several runs
- **WHEN** a snapshot looks up a hash present in multiple committed epochs
- **THEN** it SHALL receive the newest version visible to its pinned manifest or the newest visible tombstone

#### Scenario: A filter reports no membership
- **WHEN** a run's verified Bloom or xor filter proves a hash absent
- **THEN** lookup MAY skip that run but SHALL preserve the same result as a complete run search

#### Scenario: A sorted batch spans index levels
- **WHEN** MPT finalization requests sorted hashes that map to several runs and levels
- **THEN** the batch result order and values SHALL exactly match independent point reads from the same snapshot

### Requirement: Snapshot isolation and safe reclamation
Each read snapshot SHALL pin one immutable index manifest and lease every
referenced segment. Compaction, unwind, and cleanup SHALL not change its results
or remove its bytes before release.

#### Scenario: Compaction publishes during a read
- **WHEN** a new compacted manifest becomes current while an older snapshot is active
- **THEN** the older snapshot SHALL continue using its pinned runs and return unchanged values

#### Scenario: An obsolete segment is still leased
- **WHEN** cleanup finds a segment referenced by any active snapshot
- **THEN** physical deletion SHALL be deferred until the last lease is released

### Requirement: Deterministic unwind and branch replacement
Canonical unwind SHALL atomically move Ledger/root metadata and the pack
high-water mark to a prior committed epoch before later pack generations become
reclaimable. Replacement history SHALL not expose rows from the abandoned
branch.

#### Scenario: Node unwinds to a prior height
- **WHEN** the canonical store selects an earlier committed root
- **THEN** new snapshots SHALL resolve only versions visible at that epoch even if later segment bytes remain on disk

#### Scenario: A replacement branch is appended
- **WHEN** new epochs follow an unwind
- **THEN** lookup ordering SHALL prefer the replacement branch and SHALL never merge abandoned later versions into it

### Requirement: Bounded resources and compaction debt
The node SHALL enforce configured bounds for frame size, segment size, recent
runs, memory, pending bytes, and compaction debt. Reaching a hard bound SHALL
apply backpressure rather than lose committed versions or grow without limit.

#### Scenario: Compaction falls behind
- **WHEN** pending runs or bytes reach their configured hard limit
- **THEN** the persistence producer SHALL wait or reduce its batch while canonical order and all snapshot-visible versions remain intact

#### Scenario: One epoch exceeds a frame target
- **WHEN** a prepared epoch is larger than the normal frame target but within the hard maximum
- **THEN** the system SHALL store it in a bounded dedicated frame or reject it before canonical publication without partially committing

### Requirement: Observable persistence performance
The node SHALL expose bounded-label metrics for logical and physical bytes,
append, sync, index, lookup, rebuild, compaction, stalls, queue depth, and shadow
parity outcomes. Performance claims SHALL name the corpus, hardware, filesystem,
durability mode, storage mode, and percentile.

#### Scenario: A replay window completes
- **WHEN** a bounded MainNet replay reaches its verified target
- **THEN** its report SHALL separate VM, MPT finalization, pack append/sync, MDBX marker commit, lookup, compaction, and backpressure time

#### Scenario: A throughput gate is evaluated
- **WHEN** automation evaluates the 1,500-2,000 blocks/s target
- **THEN** it SHALL use transaction-bearing and adversarial declared corpora rather than infer success from empty-block or tmpfs rates

### Requirement: Paired throughput evidence for applied optimizations
Every change accepted and applied as a node-throughput optimization SHALL
publish a paired baseline/candidate report. The pair SHALL use the same exact
corpus and height range, immutable starting checkpoint, hardware, filesystem,
cache condition, durability mode, storage mode, and configuration except for
the declared optimization. The report SHALL identify both revisions and
binaries and the corpus/checkpoint digest.

The report SHALL include baseline and candidate overall blocks/s and
transaction-bearing blocks/s, the block counts and elapsed-time denominators
used for both rates, and the signed percent delta for each rate. Both sides
SHALL use identical end-to-end timing boundaries covering execution, state
finalization, and durable canonical publication and SHALL pass the applicable
root, reopen, and durability checks.

#### Scenario: An optimization is accepted on the node path
- **WHEN** a candidate is retained and described as a node-throughput optimization
- **THEN** its acceptance report SHALL show the paired baseline and candidate overall blocks/s, transaction-bearing blocks/s, both signed percent deltas, and all required reproducibility and correctness fields

#### Scenario: A change has no paired end-to-end replay
- **WHEN** a correctness, memory, recovery, format, or component-level change lacks the required paired replay
- **THEN** its report SHALL label it `no throughput evidence`, SHALL state that no node blocks/s delta was established, and SHALL NOT describe it as a node speedup

#### Scenario: Compared runs use non-identical workload evidence
- **WHEN** results come from a microbenchmark, an empty-block-only or tmpfs run, or different or adjacent MainNet ranges
- **THEN** those results MAY be reported as diagnostic or path-specific evidence but SHALL NOT establish a causal node-throughput improvement or satisfy the production throughput gate

### Requirement: Shadow parity before authority
Node-pack authority SHALL remain disabled by default until shadow mode has
compared exact state roots, node bytes, reference counts, proofs, scans, reopen,
unwind, and crash outcomes against the MDBX implementation.

#### Scenario: Shadow comparison matches
- **WHEN** both backends reopen at a declared checkpoint and all comparisons match
- **THEN** the run MAY count as promotion evidence but MDBX SHALL remain authoritative in shadow mode

#### Scenario: Shadow comparison diverges
- **WHEN** any root, node value, proof, scan, height, or failure outcome differs
- **THEN** the run SHALL fail, node-pack authority SHALL remain disabled, and the mismatch SHALL be reported with its first reproducible key or height

### Requirement: Explicit storage modes
The node SHALL distinguish disabled, shadow, and authoritative node-pack modes,
and SHALL distinguish archive, pruned, and checkpoint-sync retention products.
It SHALL not silently reinterpret or downgrade an existing database.

#### Scenario: Operator enables authoritative packs on an MDBX-only database
- **WHEN** no verified pack checkpoint or completed migration exists
- **THEN** startup SHALL reject the mode change with a recovery-safe migration requirement

#### Scenario: Operator requests archive validation
- **WHEN** a replay is declared an archive/full-state proof
- **THEN** pruning or checkpoint-only data SHALL not satisfy its historical namespace and proof requirements

### Requirement: Ordered bounded persistence pipeline
The system SHALL commit and expose persistence epochs strictly in block order
with a deterministic sequential fallback. After sequential authoritative packs
pass promotion gates, it MAY overlap bounded execution, MPT finalization, and
persistence work.

#### Scenario: A later epoch finishes execution first
- **WHEN** epoch N+1 is prepared while epoch N is still being persisted
- **THEN** N+1 SHALL remain invisible and uncommitted until N publishes successfully

#### Scenario: Pipeline reaches its byte bound
- **WHEN** speculative overlays reach the configured epoch or byte limit
- **THEN** execution SHALL apply backpressure and SHALL not allocate an unbounded queue
