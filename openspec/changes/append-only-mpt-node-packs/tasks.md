## 1. Reproducible Persistence Bakeoff

- [x] 1.1 Encode deterministic prefilled workload campaigns from the measured high-height key size, value distribution, version-hit ratio, tombstone ratio, and commit cadence
- [x] 1.2 Benchmark durable current-MDBX campaigns with logical/physical bytes, sync latency, CPU, RSS, and point/sorted-batch read percentiles
- [x] 1.3 Implement a benchmark-only append-frame plus sorted-run index prototype and run the identical sustained campaigns
- [x] 1.4 Benchmark a ParityDB hash-index column on the identical durability, filesystem, workload, lookup, reopen, and compaction conditions
- [x] 1.5 Record the backend decision and reject candidates that do not reduce physical write amplification by at least one order of magnitude without unacceptable lookup or recovery cost

## 2. Node-Pack Format And Index

- [x] 2.1 Create the `neo-state-packs` crate with bounded configuration, typed errors, single-writer lease, segment identity, and explicit format versioning
- [x] 2.2 Implement deterministic frame encode/decode for put and tombstone rows with roots, block range, lengths, ordering, checksums, and complete footer
- [x] 2.3 Add segment rotation, positioned reads, append and `sync_data`, directory durability, and hard frame/segment limits
- [x] 2.4 Implement high-water recovery, torn-tail truncation, orphan suffix handling, and fatal committed-frame corruption checks
- [ ] 2.5 Implement rebuildable immutable sorted index runs, verified filters, newest-version point reads, and ordered batch reads
- [ ] 2.6 Implement immutable manifest generations that authenticate selected segment/frame extents and run identities, snapshot segment leases, deferred reclaim, and bounded recent-run memory
- [x] 2.7 Implement leveled index compaction with newest-epoch/reference-count semantics and safe tombstone retention
- [x] 2.8 Add scrub/probe APIs and bounded-label append, sync, lookup, rebuild, compaction, debt, stall, and amplification metrics

## 3. Shadow Publication And Recovery

- [x] 3.1 Split prepared StateService overlays into exact node operations and non-node metadata without changing MPT serialization or root calculation
- [x] 3.2 Add a versioned MDBX pack high-water record and cold-first coordinated commit that atomically publishes Ledger, root metadata, and the marker
- [x] 3.3 Add disabled and shadow configuration modes, node composition, startup validation, and explicit on-disk identity checks
- [ ] 3.4 Dual-write shadow frames while MDBX remains authoritative and compare exact node values, reference counts, scans, proofs, roots, and reopen state at checkpoints
- [ ] 3.5 Add deterministic crash injection before/after frame write, sync, MDBX marker commit, index activation, compaction publication, and cleanup
- [ ] 3.6 Implement and test canonical unwind, pinned-reader behavior, suffix retirement, and replacement-branch lookup isolation
- [ ] 3.7 Replay staged MainNet clones in shadow mode and record the first mismatch or exact raw-namespace/root/recovery parity evidence

## 4. Opt-In Authoritative Node Packs

- [x] 4.1 Route authoritative `0xf0` point and sorted-batch reads through pinned pack snapshots while retaining StateService root metadata in MDBX
- [x] 4.2 Add explicit migration/checkpoint requirements and reject unsafe mode changes or silent fallback to an incomplete MDBX namespace
- [ ] 4.3 Verify archive and pruned retention policies, tombstone compaction, historical proof availability, and bounded disk growth independently
- [ ] 4.4 Run sustained high-height and adversarial transaction/storage-heavy replay with exact roots, crash recovery, compaction debt, RSS, and latency percentiles
- [ ] 4.5 Promote or reject authoritative packs using the declared correctness gates and a named hardware/filesystem 1,500-2,000 blocks/s performance profile

## 5. Ordered Persistence Pipeline

- [ ] 5.1 Add a byte- and epoch-bounded execution, MPT-finalization, and pack-publication pipeline after sequential authoritative gates pass
- [ ] 5.2 Enforce strict commit order, invisible later epochs, bounded backpressure, cancellation, and deterministic sequential retry on any stage failure
- [ ] 5.3 Expose useful/wasted overlap, queue depth, stall source, and per-stage wall-time metrics without high-cardinality labels
- [ ] 5.4 Compare pipelined and sequential store dumps, artifacts, roots, reopen, unwind, and crash outcomes on conflict-free and worst-case corpora
