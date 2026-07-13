# Phase 1 Reproducibility Evidence

**Source commit:** `d90f758920f5817347d72399fbd3c8a6e38dd9d5`  
**Evidence captured:** `2026-07-13T17:35:00Z`  
**Verdict:** Passed for the Phase 1 reproducible protocol baseline

## Scope

This artifact proves that one clean committed source tree resolves the reviewed
VM revision, passes the locked source/fuzz/policy/repository gates, and produces
a runnable no-cache container without a sibling repository.

It does **not** prove full differential execution parity, sustained live-peer
interoperability, complete MainNet replay/state parity, or authenticated
checkpoint fast sync. Those remain later milestone release gates. The current
fast-sync package path is accelerated archive replay over HTTPS with MD5
integrity; neither property authenticates a checkpoint.

## Source Identity

| Item | Retained identity |
|---|---|
| Git commit | `d90f758920f5817347d72399fbd3c8a6e38dd9d5` |
| Main worktree before validation | Clean (`git status --porcelain=v1` empty) |
| Detached Docker worktree | Clean, detached at the same full commit |
| Root `Cargo.lock` SHA-256 | `6b5a5bc65b6e1f097cf52b3eafacf19c893b0657043aaa09a16f5a778fae229e` |
| Fuzz `Cargo.lock` SHA-256 | `f013905992355efe597d9dbb1c532c84508c98e584ca0bae46709fdf0505e11d` |
| `neo-vm-rs` package | `0.2.0` |
| `neo-vm-rs` repository | `https://github.com/r3e-network/neo-vm-rs.git` |
| `neo-vm-rs` revision | `3081e83db3716fd51dc58c0afc039290d2d07253` |
| Root/fuzz features | `default-features = false`, `std`, `interpreter` |

Both manifests and both lockfiles resolve the exact VM source above. The parsed
dependency guard rejects a different revision, source, feature set, version, or
renamed bincode consumer.

## Toolchain

| Tool | Version |
|---|---|
| rustc | `1.89.0 (29483883e 2025-08-04)` |
| Cargo | `1.89.0 (c24e10642 2025-06-23)` |
| LLVM | `20.1.7` |
| cargo-deny | `0.18.9` |
| actionlint | `1.7.10` (Go `1.24.13`) |
| Python | `3.12.3` |
| Bash | `5.2.21(1)-release` |
| Docker client/server | `29.6.1` / `29.6.1` |
| Docker buildx | `0.35.0` |
| Docker Compose | `5.3.0` |

The warnings-denied Rust 1.89 Clippy run emitted no warnings. The workspace no
longer requests the newer, unknown `clippy::manual_is_multiple_of` lint.

## Validation Results

Every command below ran from the source commit named above and exited `0`.

| Gate | Exact command | Result |
|---|---|---|
| Clean source | `test -z "$(git status --porcelain=v1)"` | Passed |
| Workspace format | `cargo +1.89.0 fmt --all -- --check` | Passed |
| Test-aware workspace check | `cargo +1.89.0 check --workspace --tests --locked` | Passed |
| Workspace tests | `cargo +1.89.0 test --workspace --locked` | Passed |
| Explicit doctests | `cargo +1.89.0 test --workspace --doc --locked` | Passed |
| Workspace Clippy | `cargo +1.89.0 clippy --workspace --all-targets --locked -- -D warnings` | Passed, no warnings |
| Fuzz format | `cargo +1.89.0 fmt --manifest-path fuzz/Cargo.toml -- --check` | Passed |
| Fuzz metadata | `cargo +1.89.0 metadata --manifest-path fuzz/Cargo.toml --locked --no-deps --format-version 1` | Passed |
| Fuzz all-target check | `cargo +1.89.0 check --manifest-path fuzz/Cargo.toml --locked --all-targets` | Passed |
| Root policy | `cargo deny check advisories licenses sources --hide-inclusion-graph` | Passed |
| Fuzz policy | `cargo deny --manifest-path fuzz/Cargo.toml check advisories licenses sources --hide-inclusion-graph` | Passed |
| Workflow lint | `actionlint -no-color` | Passed |
| Shell syntax | `find scripts -type f -name '*.sh' -print0 \| xargs -0 -n1 bash -n` | Passed |
| Repository guards | `python3 -m unittest discover -s scripts/tests -p 'test_*.py'` | Passed, 336 tests |

`cargo-deny` reported only informational unmatched-policy warnings. The root
graph did not encounter `NCSA`, while the fuzz graph did; the shared reviewed
allowance is therefore required across the two policy runs. `Unicode-DFS-2016`
is an existing unmatched allowance. The fuzz graph correctly reported that the
temporary `RUSTSEC-2025-0141` exception matched no crate because fuzz contains
no bincode. Advisories, licenses, and sources all passed for both graphs.

The repository file-size checks use exact no-growth baselines for 15 Rust
files, 4 operational Python scripts, and 4 Python test modules that already
exceed the normal 900-line review budget. New exceptions fail, growth fails,
and reductions force the exact ceiling to be ratcheted down. Phase 7 requires
this baseline to be empty before `RELEASE-01`; passing this Phase 1 gate does not
represent that debt as resolved.

## Container Proof

The detached worktree was created at the source commit, confirmed clean, and
removed after the following commands completed:

```bash
docker build --pull --no-cache --progress=plain \
  -t neo-rs:phase1-d90f758920f5 .
docker run --rm --entrypoint neo-node \
  neo-rs:phase1-d90f758920f5 --version
docker image inspect neo-rs:phase1-d90f758920f5 --format '{{.Id}}'
```

| Container item | Retained result |
|---|---|
| Build status | `0` |
| Image tag | `neo-rs:phase1-d90f758920f5` |
| Immutable image ID / manifest list | `sha256:75a20e80caacae581ddf5481fd4cf8801e46f1c090f8c25749b4c67271e1f523` |
| Image manifest | `sha256:cc73be354cebcb8d6b6811fdf9007edad2de0c61c7deb47309fbf0e1463b8208` |
| Image config | `sha256:0349890ca9fffc6b82f8336d1eb2299b66dc5d1d248bcf70ca12220820cb7c2b` |
| Builder base | `rust:1.89-bullseye@sha256:8f72d971a31b278cebdb2eb64a44c3900ff27c716a4ef6f4db05946d10c9ae4e` |
| Runtime base | `debian:bullseye-slim@sha256:f18adf4e1d04b1d8ba48025b8e35003f4c748ddd3dd8e875fe4e7d9a9c0dec84` |

The `neo-node --version` smoke command exited `0`. Complete output:

```text
neo-node 0.10.0
```

The build context was the detached `neo-rs` repository only. Compose has no
additional sibling context, and the Dockerfile does not clone a floating VM
branch. Mutable base tags and Debian package repositories mean this is a
timestamped clean-build proof, not a claim of bit-for-bit image reproduction;
the actual resolved base digests are retained above.

## Supplemental Reference Status

No live C#/NeoGo endpoint validation was included in this verdict. Endpoint
unavailability is supplemental infrastructure status and cannot be reported as
protocol success. Differential and live-network evidence must be produced by
the dedicated later phases.
