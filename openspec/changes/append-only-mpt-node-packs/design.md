## Context

The coordinated StateService path currently writes canonical Ledger rows and
all MPT rows into two MDBX tables in one transaction. MPT node keys are
`0xf0 || node_hash`; their distribution is effectively random. A measured
10,000-block transaction-heavy window produced 922,549 MDBX mutations and
205 MB of values, then spent 53.35 seconds in MDBX versus 7.15 seconds in VM
execution. Live disk samples show repeated 34,000-44,000 write/s bursts made
almost entirely of 4 KiB pages.

The MPT node hash excludes the mutable reference count. A node hash is therefore
content addressed, but the exact serialized value associated with that hash can
change. Pruning and unwind also require deletes. The new store must provide
newest-committed-version semantics, not a write-once blob map.

The existing `neo-static-files` archive demonstrates useful framing, lease,
positioned-I/O, and tail-recovery patterns. Its MDBX row-location sidecar cannot
be reused for node hashes because one random index write per node would recreate
the measured bottleneck.

## Goals / Non-Goals

**Goals:**

- Persist exact existing MPT node bytes through sequential, checksummed writes.
- Keep MDBX as the single canonical commit decision for Ledger height, state
  root metadata, and the committed pack high-water mark.
- Recover deterministically from every crash boundary without exposing a root
  whose nodes are not durable.
- Provide point and sorted-batch lookup with immutable derived index runs,
  snapshot isolation, bounded memory, and measurable compaction debt.
- Roll out through benchmark, shadow, opt-in authoritative, and production
  stages with exact state-root and raw-namespace parity gates.
- Establish a persistence shape capable of a sustained 1,500-2,000 blocks/s
  hardware-specific target before adding execution/persistence pipelining.

**Non-Goals:**

- Changing Neo MPT serialization, hashing, reference counting, state roots,
  wire formats, or VM behavior.
- Replacing canonical Ledger MDBX or changing its schema in the first release.
- Treating non-durable MDBX flags, tmpfs, pruning, or checkpoint sync as archive
  throughput proof.
- Implementing optimistic transaction execution before storage publication is
  no longer the controlling bottleneck.
- Guaranteeing a hardware-independent blocks/s rate for arbitrary legal blocks.

## Decisions

### 1. Isolate the on-disk format in `neo-state-packs`

Create a service-owned crate that stores versioned raw key/value operations and
does not depend on MPT mutation logic. `neo-state-service` remains responsible
for splitting prepared overlays, validating roots, and publishing its in-memory
generation. This keeps format/recovery tests independent and avoids coupling
pack parsing to the VM or native contracts.

Alternative: place the format in `neo-state-service`. Rejected because recovery,
scrubbing, indexing, and leases form an independent persistence subsystem.

### 2. Store one ordered operation stream per commit epoch

Each frame contains a versioned header, epoch and block range, previous and
resulting roots, row count, exact payload lengths, sorted row metadata, payload
and index checksums, and a complete-frame footer. Rows contain the 32-byte node
hash, put/tombstone operation, and exact existing serialized bytes. The `0xf0`
namespace byte is implicit. Frames may rotate across bounded segment files but
one frame is never split.

Repeated hashes are legal. Lookup and compaction use newest committed epoch,
then newest row within that epoch. Tombstones remain until every older version
they mask has left the readable generation. Reference-count bytes are never
reconstructed by the pack layer.

Alternative: immutable blob files keyed only by hash. Rejected because node
reference counts and pruning deletes require versioned values.

### 3. Use immutable sorted index runs, not a mutable per-row database

Every frame carries enough sorted hash/offset metadata to rebuild its index.
Recent committed frames expose memory-mapped sorted runs with Bloom or xor
filters. Background leveled compaction merge-walks index entries and publishes
an immutable manifest; payload frames are not rewritten during ordinary index
compaction. Point reads search runs newest first. Batch reads sort once and
merge-walk relevant runs.

A snapshot pins one manifest generation and leases all referenced segments.
Index files are derived and replaceable. Missing or corrupt derived files are
rebuilt from committed frames before the store becomes ready.

Alternatives: an MDBX hash-to-offset row per node, which recreates random
writes; a single in-memory hash map, which is unbounded and expensive to rebuild.

### 4. Publish cold data before the MDBX commit marker

Authoritative publication order is:

1. Prepare canonical and MPT overlays without changing visible state.
2. Append and `sync_data` the node-pack frame.
3. Commit canonical Ledger rows, non-node StateService metadata, and a pack
   high-water record in one durable MDBX transaction.
4. Activate the prepared index run and visible in-memory StateService root.

The high-water record includes format version, epoch, segment identity, byte
offset, frame checksum, block range, and resulting root. A frame before the
marker is orphaned durable data; a marker can never precede its durable frame.
MDBX remains the sole decision of whether an epoch is canonical.

Alternative: commit MDBX first and append later. Rejected because a crash can
publish an unrecoverable state root. A distributed two-phase commit is
unnecessary when cold-first ordering gives one authoritative marker.

### 5. Keep shadow and authoritative modes explicit

`shadow` dual-writes packs while MDBX remains authoritative for every MPT row.
At configured checkpoints it reopens both stores and compares lookup results,
reachable node bytes, reference counts, proofs, scans, roots, and failure
outcomes. A mismatch disables packs and fails the replay proof.

`authoritative` stores `0xf0` node operations in packs while root/index records
remain in MDBX. It is opt-in until crash injection and sustained replay gates
pass. `disabled` is the unchanged MDBX-only production fallback. Mode changes
require an explicit migration/checkpoint and cannot silently reinterpret data.

### 6. Recover from the committed high-water mark

On startup, validate the MDBX marker against the referenced frame and root.
An incomplete tail or complete suffix above the marker is truncated or ignored.
A missing/corrupt committed frame is fatal. Missing derived indexes are rebuilt.
Incomplete compaction outputs are discarded while source runs remain valid.

Canonical unwind first atomically moves Ledger/root metadata and the high-water
mark to a prior committed epoch. Later runs become invisible immediately and
are physically reclaimed only after pinned snapshots release them. Replacement
branches append after the old suffix is hidden or truncated under one writer
lease.

### 7. Bound compaction, memory, and pipeline debt

Configuration specifies maximum frame/segment sizes, recent runs, index levels,
memory, pending bytes, and compaction debt. Exceeding a hard bound applies
backpressure; it never drops versions needed by a pinned snapshot. Metrics
expose logical/physical bytes, append and sync latency, lookups by level,
filter effectiveness, rebuild time, debt, stalls, and shadow mismatches.

Only after authoritative sequential packs pass all gates may a bounded
three-stage pipeline overlap execution/project epoch N+1, MPT finalization N,
and append/publication N-1. Visible height still advances strictly in order and
the queue is capped by bytes and epochs.

### 8. Benchmark the exact measured shape and a backend bakeoff

Phase A uses deterministic campaigns matching high-height key size, value-size
distribution, hit/miss ratio, puts/tombstones, batch cadence, and a prefilled
working set. Compare current MDBX, node packs, and a ParityDB hash-index column
on the same named filesystem. Report logical and physical bytes, sync latency,
CPU, RSS, lookup percentiles, rebuild cost, and compaction debt. Fresh-empty and
tmpfs results are diagnostic only.

## Risks / Trade-offs

- **[Cross-store publication is implemented incorrectly]** -> Keep a single
  durable MDBX marker, cold-first sync, crash injection at every boundary, and
  fail closed on a missing committed frame.
- **[Derived indexes return a stale reference count]** -> Version every row,
  define newest-epoch-wins ordering, and compare full raw node values in shadow
  mode.
- **[Tombstones are compacted too early]** -> Retain them until all masked
  versions leave the readable manifest and test pruning/unwind histories.
- **[Compaction starves catch-up or grows without bound]** -> Separate append
  from compaction, expose debt, apply bounded backpressure, and define a hard
  promotion limit.
- **[Snapshot readers race segment deletion]** -> Pin immutable manifests and
  use segment leases before reclaim.
- **[Shadow mode doubles write cost]** -> Keep it opt-in and bounded to
  verification campaigns; it is correctness evidence, not a speed benchmark.
- **[Compression costs more than it saves]** -> Default the first benchmark to
  uncompressed frames and promote a codec only from measured end-to-end gains.
- **[A fast microbenchmark is mistaken for node throughput]** -> Require
  sustained high-height MainNet replay, reopen, crash, and state-root proof on
  declared hardware before promotion.

## Migration Plan

1. Add deterministic backend campaigns and format/recovery unit tests without
   changing node composition.
2. Add shadow frames and marker metadata; MDBX remains fully authoritative.
3. Run bounded MainNet clones, crash injection, corruption, index rebuild, and
   namespace/proof comparisons.
4. Add opt-in authoritative packs for the node namespace and retain immediate
   rollback to the last shadow-verified MDBX checkpoint.
5. Prove sustained replay, bounded compaction, and restart/unwind behavior.
6. Consider a production default only after multiple independent hosts pass.
7. Add bounded persistence pipelining as a separate measured promotion.

Rollback before authoritative mode is deletion of unreferenced pack files and
marker metadata. Rollback from authoritative mode requires a verified MDBX
checkpoint or deterministic materialization of the committed pack generation;
the node must not silently fall back to an incomplete MDBX node namespace.

## Open Questions

- Whether the first production index should be custom sorted runs or ParityDB
  after the Phase A bakeoff.
- Whether payload compression wins on real serialized-node distributions.
- The frame epoch size that balances sync fences, recovery scope, memory, and
  lookup-run count on NVMe and slower durable disks.
- Whether archive and pruned modes share one format with different retention or
  use separately tuned compaction policies.
- Which hardware/corpus/percentile definition becomes the release performance
  gate in addition to the current high-height MainNet windows.
