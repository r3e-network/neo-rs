## Why

Durable full-state MainNet replay is limited by random copy-on-write MDBX
updates for content-addressed MPT nodes, not by block decoding or VM execution.
Measured transaction-heavy windows spend 53 seconds publishing state for every
7 seconds of VM work and exhibit roughly 26x device-to-logical write
amplification, making the 1,500-2,000 blocks/s target unreachable through local
cache or cursor tuning.

## What Changes

- Add a crash-recoverable append-only node-pack format for exact Neo MPT node
  bytes and immutable sorted hash indexes.
- Keep canonical Ledger data, StateService root metadata, and the committed
  node-pack high-water mark in one authoritative MDBX transaction.
- Add cold-first publication, torn-tail recovery, checksum verification,
  deterministic index rebuild, unwind, and corruption handling.
- Add a shadow-write rollout that compares every reachable node and proof with
  the existing MDBX namespace before node packs can become authoritative.
- Add sustained persistence benchmarks, physical-write metrics, compaction
  debt metrics, and named hardware/filesystem performance gates.
- Add a bounded execution/finalization/persistence pipeline only after the
  sequential storage format proves exact state-root and crash parity.
- Preserve MDBX as the production default until every promotion gate passes;
  no unsafe sync mode is promoted.

## Capabilities

### New Capabilities

- `mpt-node-pack-storage`: Durable append-only storage, lookup indexing,
  publication, recovery, compaction, observability, and rollout requirements
  for content-addressed StateService MPT nodes.

### Modified Capabilities

None.

## Impact

The change affects `neo-state-service`, `neo-storage`, node composition and
recovery, StateService configuration and metrics, the database probe, replay
tooling, and storage benchmarks. It introduces a new service-owned persistence
domain and on-disk format but does not change Neo wire data, VM semantics, MPT
serialization, state-root calculation, or the canonical Ledger schema. Archive,
pruned, and checkpoint-sync storage modes remain explicit and distinct.
