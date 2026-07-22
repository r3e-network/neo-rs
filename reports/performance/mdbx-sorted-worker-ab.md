# MDBX Sorted Worker A/B

Captured 2026-07-16 on the deferred full-state path at heights 811,001-812,000
with eight opt-in MDBX readers. The parallel sorted worker now dispatches each
contiguous chunk to the bounded ordered cursor routine instead of the generic
per-key seek routine.

The run reached height 812,000 with zero MPT failures and matched the official
root `0xacd96890371c7c1df925cb172d2df7b5e87731b26f6bff7fa9a8284ed7598fac`.
The lookup stage measured 2.366 seconds for 65,875 backing misses and the
transaction-bearing rate was 669.1 blocks/s. The bounded ordered cursor still
falls back to independent seeks for this highly sparse content-addressed key
set, so this correction is a correctness-preserving dispatch fix rather than a
measured throughput gain for this particular window.

Structured log: `fullstate-profile-h811-812-sorted-cursor-node.log` (SHA-256
`589d8981a6a3099a25e73727699ebcc2861141686b443a746df7f01a35dd3bea`).
