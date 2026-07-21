# State-Pack Format Foundation, 2026-07-21

Status: accepted correctness and format foundation; **no throughput evidence**.

## Scope

- Baseline: `b4e73e24` (`origin/main` before this change).
- Candidate: `feat/state-pack-foundation` (the commit containing this report).
- Replaced raw constructor knobs with one validated `PackStoreConfig`.
- Added caller-visible error categories for configuration, versions, limits,
  ownership, I/O, corruption, and read-worker availability.
- Added a kernel-held single-writer lease with independent-process lifetime
  coverage.
- Added authenticated, explicitly versioned segment headers and stable segment
  identities/positions.
- Bound authoritative markers, shadow markers, checkpoints, and verification
  evidence to segment format, segment identity, frame end, and payload checksum.
- Split pack-store implementation files into `format`, `lifecycle`,
  `validation`, and `tests` domains.

This format intentionally rejects the earlier unversioned `frames.pack` layout
and old checkpoint/marker schemas. Node-pack authority remains disabled by
default; an operator must rebuild or explicitly migrate experimental pack data.

## Correctness Evidence

- `cargo test -p neo-state-packs --locked`: 87 passed, 2 ignored subprocess
  workers, 0 failed.
- `cargo test -p neo-node --locked`: 488 tests passed across node binaries and
  integration targets, 0 failed.
- `cargo test -p neo-tests --test layer_boundary_tests --locked`: 30 passed.
- Strict all-target/all-feature Clippy passed for `neo-state-packs` and
  `neo-node`.
- Strict Clippy passed for the affected `append-persistence-bench` target.
- Repository hygiene passed after the module split.
- `openspec validate append-only-mpt-node-packs --strict` passed.

## Throughput Evidence

No paired StateRoot-enabled MainNet replay was run because this batch changes
format identity, recovery validation, API typing, and source organization; it
does not claim a node hot-path optimization. Therefore:

- StateRoot-enabled baseline blocks/s: not measured for this change.
- StateRoot-enabled candidate blocks/s: not measured for this change.
- Signed blocks/s delta: not established.
- Transaction-bearing blocks/s delta: not established.

No node speedup or progress toward the 2,000 blocks/s production gate is
inferred from component tests or the earlier persistence bakeoff.

## Known Repository Baseline Gaps

The workspace-wide `neo-benches --all-targets` Clippy gate is independently
blocked because `vm_execution.rs` includes a MainNet contract-map report that
is absent from `origin/main`. Repository file-size policy also reports numerous
pre-existing debt-ratchet mismatches outside this change. Neither condition was
hidden or converted into an exception here.
