# Optimistic Signature Verification: No Throughput Evidence

Date: 2026-07-20

## Scope

The bounded `SignatureVerificationPool` overlaps NeoVM verification of header
consensus witnesses with header intake. Inventory batches keep an ordered
look-ahead window bounded by `workers + queue_capacity`, so multiple header
witnesses can verify while the canonical lane executes and persists the current
block. A receipt is bound to the exact header bytes, parent context, chain
identity, witness digest, and applicable cache revision. The ordered import path
checks that receipt at the canonical persistence fence before `HeaderCache`,
Ledger, MPT, StateService, events, relay, or durable publication can advance.
Any invalid receipt, stale context, worker failure, queue shutdown, or broken
parent chain falls back to the synchronous NeoVM verifier and discards later
speculative tickets.

The transaction signature helper remains an independent mempool admission and
audit API. It is not used as a new historical block rejection rule: the
existing protocol regression deliberately accepts a verified-sync block with
an invalid standard transaction signature, matching the current Neo N3 import
contract until a C# differential fixture proves otherwise.

The pool is disabled by default. It overlaps work but never publishes
unverified block state.

## Performance verdict

**No end-to-end node throughput delta was established.** No identical
StateRoot-enabled MainNet A/B was run with only the header look-ahead window
changed. The targeted regression proves ordered publication and deep invalid
prefix handling, but unit-test timing is not valid blocks-per-second evidence.
The runtime log record reports `blocks_per_second`, queue counters,
`signature_prefetched_headers`, and `signature_max_pending` whenever the
opt-in inventory path processes a batch.

The authoritative high-height StateRoot-enabled reference remains the existing
`reports/performance/node-publication-parallel-3452022-3457022-20260720.md`
campaign; it must not be attributed to this signature pool. The fixed counters
(`submitted`, `valid`, `invalid`, worker failures, and queue rejections) are
available through `SignatureVerificationPool::metrics_snapshot()` and are
emitted with the opt-in `neo::performance` inventory record. The existing
StateRoot-enabled high-height reference is 775.16 blocks/s, but it predates
this window and must not be attributed to it.

## Acceptance status

This is a correctness-safe, opt-in latency-overlap foundation, not an accepted
node throughput optimization. A paired StateRoot-enabled replay with identical
height range, checkpoint, binary profile, hardware, filesystem, durability, and
cache condition is required before claiming any blocks/s improvement or making
the pool default.
