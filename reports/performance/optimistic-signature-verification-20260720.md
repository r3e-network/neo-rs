# Optimistic Signature Verification: No Throughput Evidence

Date: 2026-07-20

## Scope

The bounded `SignatureVerificationPool` overlaps NeoVM verification of header
consensus witnesses with header intake. A receipt is bound to the exact
unsigned header hash, parent context, chain identity, witness digest, and the
applicable cache revision. The ordered import path rechecks that receipt before
`HeaderCache`, Ledger, MPT, StateService, events, relay, or durable publication
can advance. Invalid receipts stop at the valid prefix; worker failures use the
canonical synchronous verifier.

The pool is disabled by default. It does not assume transaction signatures are
valid and does not publish unverified block state.

## Performance verdict

**No end-to-end node throughput delta was established.** The current MainNet
replay/import path does not repeat transaction signature verification for
historical blocks, and no identical StateRoot-enabled MainNet A/B was run with
only this pool changed. Header-only unit tests and a synthetic queue test are
not valid blocks-per-second evidence under the project acceptance rules.

The authoritative high-height StateRoot-enabled reference remains the existing
`reports/performance/node-publication-parallel-3452022-3457022-20260720.md`
campaign; it must not be attributed to this signature pool. The fixed counters
(`submitted`, `valid`, `invalid`, worker failures, and queue rejections) are
available through `SignatureVerificationPool::metrics_snapshot()` and are
emitted with the opt-in performance log records once the pending ChainSpec
composition migration wires the pool into the node.

## Acceptance status

This is a correctness-safe, opt-in latency-overlap foundation, not an accepted
node throughput optimization. A paired StateRoot-enabled replay with identical
height range, checkpoint, binary profile, hardware, filesystem, durability, and
cache condition is required before claiming any blocks/s improvement or making
the pool default.
