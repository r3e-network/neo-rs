# State-pack recovery and publication fences

Date: 2026-07-21

Status: **accepted as correctness hardening; no throughput evidence**.

## Changes

- Canonical pack horizons now bind `frame_end` in addition to epoch and
  payload SHA-256. Prepare activation and startup recovery reject a marker
  whose placement differs from the durable frame chain.
- A failed append-shadow window now returns a mandatory degraded marker. MDBX
  commits that marker atomically with the canonical overlays, and restart
  refuses to append after the older shadow high-water until an explicit rebuild
  or reseed clears the degraded state.
- Compaction adoption reopens the prepared run and fully scrubs its records,
  payload ranges, ordering, fences, filter membership, and checksums before
  publishing a manifest. Failure leaves the prior generation authoritative.
- Degraded-marker decoding is canonical: payload bytes for absent optional
  fields must be zero, so one logical marker has one accepted byte encoding.

The append-frame payload format is unchanged by this recovery hardening. After
integration with the segment-format foundation, shadow high-water record v4
binds the segment format, segment identity, and segment-relative `frame_end`;
older marker schemas are rejected instead of being silently reinterpreted.

## Verification

- Full `neo-state-packs` tests passed: 92 passed and 2 ignored subprocess
  helpers. This includes
  wrong-frame-end activation/reopen and corrupt prepared compaction record,
  fence, internally re-checksummed false-negative filter, cross-process writer
  lease, and canonical degraded-marker regressions.
- Full `neo-storage` tests passed: 240 passed. The three MDBX shadow-hook
  outcomes all passed, including atomic degraded-marker publication.
- Full `neo-node` tests passed, including 424 main-node tests and the pack
  verifier, pack builder, database probe, rebase tool, and integration targets.
- Strict all-target/all-feature Clippy passed for `neo-state-packs`,
  `neo-storage`, and `neo-node` with warnings denied.
- Focused formatting, repository hygiene, layer-boundary tests, strict OpenSpec
  validation, and `git diff --check` passed.

## Throughput statement

This change closes corruption and continuity holes; it is not described as a
node speedup and no blocks/s delta is established. The latest separate,
StateRoot-enabled authoritative-pack reference remains 5,000 MainNet blocks in
6.450 seconds, or **775.16 blocks/s** overall and **435.02
transaction-bearing blocks/s**, from
`node-publication-parallel-3452022-3457022-20260720.md`.

The next measured candidate is same-epoch overlap of durable pack preparation
with application of canonical/metadata MDBX overlays. Its current Amdahl upper
bound is approximately 914.95 blocks/s (+18.03%), not measured evidence. It
must not overlap MDBX durable commit or weaken pack readback/seal validation.
