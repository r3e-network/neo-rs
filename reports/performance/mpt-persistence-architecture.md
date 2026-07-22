# Full-State MPT Persistence Architecture

Captured 2026-07-16 from the production full-state replay evidence through
height 1,827,000. This report is an architecture recommendation, not a claim
that the 2,000 blocks/s target has been met. The durable replay database was
not opened or modified for this investigation.

## Decision

Stop spending the primary optimization budget on VM dispatch, object reuse,
MDBX sync flags, or larger MDBX transactions. Keep those as secondary tuning
surfaces. The next persistence prototype should be a log-structured MPT node
store with these properties:

1. MPT node bytes are staged into checksummed, append-only segment frames.
2. The segment is synced before the canonical Ledger transaction is allowed to
   publish the corresponding state root.
3. MDBX remains the atomic publication point for canonical Ledger bytes, root
   metadata, and the committed node-pack high-water mark.
4. The hash-to-segment index is derived and rebuildable. It must be an
   immutable sorted-run/LSM index, not one random MDBX update per MPT node.
5. Recent committed runs remain memory indexed while background compaction
   builds larger immutable levels.

This is the smallest architectural change that attacks the measured write
shape while preserving the existing execution, MPT serialization, state-root,
and canonical commit authority.

## Measured Constraint

The production `1,817,001..1,827,000` window recorded:

| Measure | Value |
| --- | ---: |
| Imported blocks | 10,000 |
| Import wall time | 44.089 s |
| Overall throughput | 226.81 blocks/s |
| Transaction-bearing blocks | 2,718 |
| Transactions | 4,324 |
| Transaction execution | 2.510 s |
| Finalization/store commit | 40.550 s |
| MDBX commit path | 35.015 s |
| MDBX cursor writes | 10.026 s |
| MDBX entries | 616,621 |
| Puts / deletes | 615,992 / 629 |
| Key bytes | 19.84 MB |
| Value bytes | 132.42 MB |
| Deferred backing hits / misses | 3,967 / 569,535 |
| Exact reopened reference root | yes |

The earlier syscall sample observed approximately 1.14 million `pwrite64`
calls plus 28,862 `pwritev` calls and nine `fdatasync` calls over a 10,000
block window. Absolute syscall time was distorted by `strace`, but the call
shape is decisive.

The adjacent non-`strace` `iostat` capture is stronger amplification evidence.
Its six five-second active intervals reported about 3.99 GiB of aggregate
device writes while the node reported 22.86 MB of keys plus 136.56 MB of
values. The observed device/logical ratio is about 26x. The device counters are
host-wide rather than process-attributed, so this is an upper-bound estimate,
but the 4.28-4.50 KiB average write size and 23,000-36,000 writes/s match the
MDBX page-write syscall profile.

At 2,000 blocks/s, 10,000 blocks have a 5 second wall-time budget. The latest
window contains only about 26.5 MB/s of MPT value payload at that rate, but it
requires about 123,000 logical MPT entries/s. Sequential payload bandwidth is
therefore plausible. Random copy-on-write B-tree page traffic is not.

The keys are `0xf0 || SHA256(node_payload)`, so newly produced node keys are
uniformly distributed over an increasingly large table. Sorting the overlay
removes caller disorder but cannot turn those insert positions into an append
workload. The latest window was 99.90% puts and only 0.10% deletes. It is a
near-ideal append-log workload being forced through a mutable B-tree.

## Current Write Path

The relevant production path is:

```text
block/native execution
  -> DataCache projected StateService changes
  -> MptStore::prepare_block_changes_batch
  -> Trie deferred full-state finalization
  -> MptWriteBatch hash overlay
  -> PreparedMptCommit ordered overlay
  -> Hooks::commit_canonical
  -> MdbxStore::commit_coordinated_overlays
  -> cursor UPSERT/DELETE for every node
  -> MDBX copy-on-write page commit and durable fence
```

The important source boundaries are:

- `neo-state-service/src/storage/mpt_store.rs`: prepares and publishes the
  full-state overlay.
- `neo-state-service/src/storage/mpt_store/write_batch.rs`: frozen backing
  reads, negative provenance, and overlay staging.
- `neo-trie/src/mpt/cache/operations.rs`: preserves exact serialized node payloads
  and reference counts during deferred finalization.
- `neo-node/src/node/context/plugins.rs`: coordinates StateService and Ledger
  publication.
- `neo-storage/src/mdbx/store.rs`: visits the ordered overlays, writes every
  entry through an MDBX cursor, and commits the transaction.

The MPT code is already eliminating repeated ancestors, batching backing
reads, reusing path scratch space, and collapsing each commit to its surviving
hash overlay. The remaining cost occurs after those optimizations.

## Why Local MDBX Tuning Is Insufficient

Measured candidates already reject the obvious local changes:

| Candidate | Result |
| --- | --- |
| `no-meta-sync` | No gain |
| `safe-no-sync` | Faster but not durable; still below the target |
| WriteMap | Higher cost per overlay entry |
| coalescing | Slower |
| no-meminit | Slower |
| one unbounded transaction | Cursor work became nonlinear |
| 32,768 projected-change budget | Slower than 16,384 |
| merge cursor | Correct but slower for sparse hash keys |
| cross-batch decoded-node cache | Almost no reuse and slower |
| bounded MPT hot cache | Zero memory hits in the measured run |
| pruning clone | No demonstrated high-height gain |

MDBX dirty-page reserve and spill limits remain valid diagnostic A/B knobs,
but they cannot remove the fundamental random copy-on-write leaf updates. The
host's automatic dirty-page allowance is also much larger than one measured
commit's logical value volume, so repeated spill is not the leading
explanation without new counter evidence.

## Proposed Node-Pack Store

### Physical format

Create a StateService-owned segmented store, preferably as a new
`neo-state-packs` crate that reuses the proven framing, lease, positioned I/O,
checksumming, and tail-recovery concepts in `neo-static-files`.

Each commit epoch writes one or more frames:

```text
frame header
  format version
  epoch and block range
  previous and resulting state root
  row count and payload lengths
  index and payload checksums

sorted rows
  node hash [32]
  operation: put or tombstone
  serialized-node length
  exact existing Neo MPT serialized bytes

frame footer
  header checksum and complete-frame marker
```

The `0xf0` namespace byte is implicit in a node pack. Non-node StateService
records, including local roots and the current-root index, remain in MDBX.
Node serialization must remain byte-for-byte unchanged. Compression should be
optional and benchmarked; random child hashes make aggressive compression a
poor assumption on the hot path. Full-state replay is overwhelmingly put-only,
but the format still needs tombstones so pruning mode, unwind, and fault tests
do not rely on an archive-only assumption.

The node hash excludes its reference count. Repeated puts for one hash must
therefore retain the newest exact serialized value, including the reference
count, while the content payload remains hash-verifiable. Index compaction is
newest-epoch-wins and must retain a newest tombstone until all older indexed
versions it masks have left the readable generation.

The existing `neo-static-files` row sidecar must not be reused unchanged. It
stores one MDBX row-location update per archived row, which would recreate the
same random-write bottleneck for MPT hashes.

### Derived index

Use a conventional leveled index:

- Each synced frame already contains a sorted hash/offset index.
- A small recent-run map or memory-mapped sorted array covers un-compacted
  frames.
- Every run has a Bloom or xor filter, minimum/maximum hash, checksum, and
  committed epoch range.
- Point lookup checks the mutable execution overlay, recent runs newest first,
  and then compacted levels.
- Batch lookup sorts hashes once and merge-walks candidate runs.
- Background compaction merges immutable indexes and does not rewrite node
  payload frames.
- A read snapshot pins one immutable index manifest and the segment leases it
  references, so compaction and unwind cannot change a live root view.
- Index files are derived. Missing or corrupt indexes rebuild from committed
  frame headers and row indexes.

An initial prototype should benchmark this design against a ParityDB hash-index
column. ParityDB is a relevant Polkadot-derived candidate for content-addressed
state, but adopting it only for StateService creates a second durability
domain. It still needs the publication protocol below. Replacing all canonical
storage with another engine is a separate migration and should not be coupled
to the first performance proof.

### Atomic publication

The existing cold-first static Ledger protocol supplies the correct ordering:

```text
1. Prepare canonical overlay and exact MPT node overlay in memory.
2. Append node-pack frames and call sync_data.
3. In one durable MDBX transaction, commit:
   - canonical Ledger overlay,
   - StateService root metadata,
   - committed pack segment/offset/checksum high-water mark.
4. Advance the in-memory visible StateService root.
5. Publish or compact the derived hash index asynchronously.
```

Nodes appended before the MDBX commit are not reachable from a committed root
and are harmless. A committed root is never published before its node bytes
are durable. MDBX remains the single commit decision.

### Recovery matrix

| Crash point | Required recovery |
| --- | --- |
| Before frame sync | Discard or truncate incomplete footer |
| After frame sync, before MDBX commit | Ignore/truncate suffix above committed high-water mark |
| During MDBX commit | MDBX exposes either the old or new root/high-water mark |
| After MDBX commit, before index publication | Rebuild the missing index run from committed frame bytes |
| During index compaction | Ignore incomplete output; source runs remain authoritative |

Startup must verify that the committed frame checksum and resulting root in
the MDBX manifest match the frame. A missing committed frame is fatal. An
unpublished complete suffix is recoverable garbage, never implicit canonical
state. Canonical unwind first moves the MDBX high-water mark to the selected
committed epoch, then retires later index generations and pack suffixes only
after pinned readers release them. A replacement branch appends only after the
old suffix is logically hidden or physically truncated under the single-writer
lease.

## Execution And Persistence Pipeline

Once sequential node persistence is proven, overlap it with useful CPU work
using a bounded three-stage pipeline:

```text
execute/project epoch N+1
  || finalize/hash epoch N
  || append/sync/publish epoch N-1
```

Execution may advance against immutable logical overlays before the previous
epoch reaches disk, but visible canonical height must advance only in commit
order. Keep two or three epochs maximum and apply memory backpressure. A crash
replays from the last committed high-water mark.

This pipeline cannot make the current 35 second write stage disappear; it is a
multiplier after the physical write shape is fixed. Today only about 3.3
seconds of import work can overlap a 40.5 second finalization stage.

## Optimistic Parallel Transaction Execution

Transaction execution is now a secondary but real budget item. A safe design
executes transactions in one block speculatively against the post-`OnPersist`
snapshot, records complete read/write sets, and commits results in canonical
transaction order.

Validation must include:

- exact storage-key reads and writes;
- range/prefix reads with phantom detection;
- native-contract reads and versioned context;
- contract/script loads and dynamic-call policy reads;
- fee and signer-dependent state;
- emitted artifacts, gas, and fault state.

If an earlier committed transaction changes any recorded dependency, rerun the
candidate sequentially against the updated overlay. The sequential path is the
deterministic fallback and oracle. Native `OnPersist` and `PostPersist` remain
ordered bookends. This work needs differential block fixtures and conflict-
heavy adversarial tests before it can enter production.

Optimistic execution cannot explain or solve the current durable bottleneck:
the latest window spent about 2.51 seconds in VM execution and 35.01 seconds
inside MDBX commit work.

## Storage Modes

Do not conflate three different operator products:

1. `pruned`: current authenticated state plus a configured recent window.
2. `archive`: all historical authenticated MPT nodes and roots.
3. `checkpoint fast sync`: cryptographically verified state package plus the
   headers/proofs required to establish its root, followed by normal replay.

Pruning and checkpoint sync can reduce work but do not satisfy an archive
claim. The node must report the selected mode and its proof/retention boundary.
This mirrors Reth's explicit pruning/static-file profiles and Polkadot's state
pruning/warp-sync separation.

## Delivery Plan

### Phase A: Reproducible backend benchmark

- Extract a representative anonymized MPT overlay shape from metrics: 33-byte
  hash keys, measured value-size distribution, hit/miss ratio, and commit
  cadence.
- Benchmark current MDBX, append-only frames plus sorted-run index, and
  ParityDB on the same filesystem.
- Record logical bytes, physical bytes, write syscalls, sync time, CPU, RSS,
  lookup latency, and compaction debt.
- Require sustained throughput, not an empty fresh-database microbenchmark.

### Phase B: Shadow node packs

- Dual-write packs while MDBX remains authoritative.
- Reopen packs and compare every reachable node byte at checkpoint roots.
- Compare full StateService raw namespace, latest reference counts, proofs,
  and state scans.
- Inject frame truncation, checksum corruption, failed sync, and failed
  canonical publication.

### Phase C: Opt-in authoritative packs

- Make the pack store authoritative for the `0xf0` node namespace only.
- Keep root metadata and canonical Ledger publication in MDBX.
- Add bounded recent-run memory and background index compaction.
- Replay bounded MainNet clones and require exact roots after reopen.

### Phase D: Pipelining and optimistic execution

- Add bounded execution/finalization/persistence epochs.
- Then add read/write-set speculative transaction execution with deterministic
  fallback.
- Measure both conflict-free and adversarial conflict-heavy blocks.

## Promotion Gates

No backend or parallel path should become the production default until it
passes all of these gates:

- exact official state roots at every declared checkpoint;
- exact reachable node bytes and proof responses;
- full raw namespace/reference-count parity where archive mode promises it;
- canonical and StateService heights match after every reopen;
- deterministic crash tests at every publication boundary;
- corruption detection and bounded index rebuild;
- no unbounded memory growth or compaction backlog;
- sustained performance on a named hardware/filesystem profile;
- latency percentiles for full blocks, not only average empty-block rate.

The phrase "2,000 blocks/s in the worst case" cannot be a hardware-independent
correctness promise. Neo blocks have bounded but variable transaction, script,
storage, and witness work. The release gate must define the CPU, memory, disk,
block corpus, storage mode, durability mode, and percentile. The current MainNet
window is a valid regression corpus; an adversarial maximum-work corpus is a
separate required gate.

## Immediate Next Action

Build Phase A before another production-path cache or MDBX cursor variant. The
prototype should reuse the existing static-file frame/recovery concepts but
must use a sorted-run index rather than the current MDBX row sidecar. A result
that cannot reduce physical write amplification by at least one order of
magnitude is not capable of closing the measured 8x to 10x durable gap and
should not proceed to MainNet replay.
