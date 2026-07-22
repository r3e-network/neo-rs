# State-Pack Manifest V3 Correctness Gate

## Outcome

Manifest v3 binds every published index generation to the exact authenticated
frame history and complete physical run identities. A coherent manifest and
run set copied from a different same-height store can no longer authorize
foreign offsets against the local pack.

This is a correctness and recovery-format migration, not a throughput
optimization. No before/after MainNet campaign was run and no BPS improvement
is claimed. The latest independently measured StateRoot-enabled result remains
**346.63 blocks/s** from
`optimistic-signature-verification-20260721.md`; it is not attributable to this
change and remains **5.77x below** the 2,000 blocks/s requirement.

## Format and recovery contract

- Manifest format v3 is current-only; numeric v1/v2/unknown manifests fail
  closed instead of entering an in-process compatibility path. Checkpoint and
  authority artifacts embedding manifest format v1/v2 require a rebuild from
  authoritative MDBX/source data with the current tooling.
- Domain-separated SHA-256 binds the manifest header, segment extents, rolling
  canonical frame receipts, and run sections.
- Each run identity covers its format, epoch range, record count, records
  offset, file size, records digest, structure digest, and canonical name.
- Standalone recovery authenticates the manifest-selected frame prefix before
  cleanup, truncation, or rebuild. A history mismatch is fatal and leaves every
  artifact unchanged.
- An external durable commit horizon may rebuild corrupt derived manifests and
  runs only after authenticating its exact selected frame prefix.
- Unknown manifest versions remain typed fatal errors in both recovery modes.
- Manifest decoding validates the fixed header and count-derived exact file
  geometry before allocating or reading the variable body; allocation and
  encoding arithmetic are fallible and checked. Extent and run counts are
  capped at the implementation hard limit of 4,096 before length arithmetic.

## Verification

- `neo-state-packs`: 110 passed, 2 ignored.
- `neo-state-service`: 135 passed.
- `neo-pack-build`: 7 passed.
- `neo-pack-verify`: 5 passed.
- Layer-boundary integration tests: 31 passed.
- Strict all-target/all-feature `neo-state-packs` Clippy: passed.
- `neo-node` and `neo-state-service` compile checks: passed.
- Strict OpenSpec validation, targeted formatting, and `git diff --check`:
  passed.

The repository file-size ratchet still has two unrelated pre-existing failures:
`scripts/run-bounded-mainnet-replay.py` is 41 lines above its recorded ceiling,
and `scripts/tests/test_run_bounded_replay.py` is 52 lines above its recorded
ceiling. Neither file is modified by this change.

## Remaining work

OpenSpec task 2.6 remains open. Segment rotation, segment-scoped snapshot
leases, deferred reclamation, and configured recent-run/index-level
backpressure are not completed by manifest v3.
