# State-pack frame v2

Date: 2026-07-22

Status: **accepted format and recovery correctness work; no throughput evidence**.

## Scope

- Baseline: `c4642e6c` (`origin/main` before this change).
- Candidate: `feat/state-pack-frame-v2-recovered` (the commit containing this
  report).
- Replaced the experimental frame format with current-only `N3PACK02` frames:
  a fixed header, fixed-width canonical row metadata, packed values, and a
  complete footer.
- Bound every frame to its block range, previous StateRoot, resulting
  StateRoot, exact section lengths, domain-separated section digests, epoch,
  and complete-frame digest.
- Made tombstones distinct from empty puts and enforced strict key/sequence
  order plus an exact sequence permutation.
- Moved frame construction behind `PackStore`, with conservative limits before
  allocation and exact limits before durable writes.
- Added current-only shadow, authority, and checkpoint markers carrying the
  full frame context and digest. Removed legacy checkpoint adoption paths.
- Centralized exact StateService current/historical root decoding in
  `neo-state-service` and validated frame history from one frozen snapshot.
- Authenticated every committed frame and canonical row before recovery may
  clean temporary artifacts, truncate orphan bytes, or rebuild derived runs.
  A checksum-consistent but structurally invalid frame now fails with all store
  bytes and names unchanged.
- Kept metadata-only blocks frame-free while explicitly consuming both halves
  of the coordinated StateService overlay.

## Correctness evidence

- `cargo test -p neo-state-packs --locked`: 104 passed, 2 ignored subprocess
  workers, 0 failed.
- `cargo test -p neo-state-service --locked`: 135 passed, 0 failed.
- `cargo test -p neo-node --all-targets --all-features --locked`: 430 main-node
  tests plus 59 tool/integration tests passed, 0 failed.
- `cargo test -p neo-tests --test layer_boundary_tests --locked`: 31 passed.
- `cargo test -p neo-benches --bin append-persistence-bench --locked`: binary
  compiled and its empty test harness completed successfully.
- Strict all-target/all-feature Clippy passed for `neo-state-packs`,
  `neo-state-service`, and `neo-node`.
- Targeted formatting for every touched crate, repository hygiene (11 tests),
  strict OpenSpec validation, stale frame-API search, and `git diff --check`
  passed.

The workspace-wide format check remains blocked only by two clean, inherited
`neo-mempool` re-export lists outside this change. They were not mixed into this
commit.

## Throughput evidence

No paired StateRoot-enabled MainNet replay was run. This change replaces a
persistent format and hardens startup/recovery behavior; it does not establish
a node hot-path improvement. Therefore:

- StateRoot-enabled baseline blocks/s: not measured for this change.
- StateRoot-enabled candidate blocks/s: not measured for this change.
- Blocks/s delta: not established.
- Transaction-bearing blocks/s delta: not established.

This report is not a speedup claim and does not advance the 2,000 blocks/s
production gate by itself.

## Remaining promotion gates

The node-pack authority remains disabled by default and is not yet promoted as
a production storage authority. In particular, the unfinished manifest task
must cryptographically bind each selected segment/frame extent to its derived
run identity; independently valid stale runs must never be accepted for a
different frame history. Segment rotation, complete multi-segment recovery,
unwind/replacement history, sustained MainNet replay, and paired performance
evidence also remain open OpenSpec tasks.
