# Optimistic Signature Verification: No Throughput Evidence

Date: 2026-07-20

## Scope

The bounded `SignatureVerificationPool` overlaps NeoVM verification of header
consensus witnesses with header intake. A header receipt is bound to the exact
header bytes, parent context, chain identity, witness digest, and applicable
cache revision. The ordered header path rechecks that receipt before
`HeaderCache` publication can advance; worker failures and stale receipts use
the canonical synchronous verifier. The transaction verifier remains an
independent admission/audit API; canonical block import does not add a new
transaction-signature acceptance rule beyond Neo N3 v3.10.1.

The pool is disabled by default. It overlaps work but never publishes
unverified block state.

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
emitted with the opt-in `neo::performance` header batch record.

## Acceptance status

This is a correctness-safe, opt-in latency-overlap foundation, not an accepted
node throughput optimization. A paired StateRoot-enabled replay with identical
height range, checkpoint, binary profile, hardware, filesystem, durability, and
cache condition is required before claiming any blocks/s improvement or making
the pool default.
