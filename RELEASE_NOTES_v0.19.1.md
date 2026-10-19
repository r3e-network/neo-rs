# Neo-RS v0.19.1 Release Notes

**Release date:** 2026-10-20  
**Protocol compatibility:** Neo N3 v3.10.1  
**MSRV:** Rust 1.88  
**License:** MIT

This is a packaging and CI hotfix for v0.19.0. Runtime behavior is unchanged;
all v0.19.0 validation evidence applies.

## Fixes

- Docker image build: copy `tools/` into the builder stage so the workspace
  members `tools/validation-diff` and `tools/state-root-diff` resolve. The
  v0.19.0 Docker build failed before compiling `neo-node`.
- TEE feature build: re-export `EnclaveInitError` from `neo_tee` so
  `neo-node --features tee` compiles.
- Dependency policy: add `license = "MIT"` to both tool crates, unblocking
  `cargo deny check licenses bans sources`.
- Clippy on current stable: remove redundant `#[must_use]` attributes flagged
  by `clippy::double_must_use`.

## Verification

```text
Workspace tests:        4,517 passed, 0 failed
Formatting:             cargo fmt --all --check
Clippy (CI args):       clean
TEE feature:            cargo check -p neo-node --features tee
Full feature:           cargo check -p neo-node --features full --locked
Release binary:         neo-node 0.19.1, cargo build --release --locked
Replay spot check:      blocks 1,346,000..=1,347,000, 0 mismatches
```

## Use v0.19.1 instead of v0.19.0

The v0.19.0 GitHub release is marked pre-release and its Docker workflow
failed at the image build step. Use v0.19.1 for the published container.

## Known limitations

See `SECURITY-AUDIT-v0.19.0.md` and `RELEASE_NOTES_v0.19.0.md`. This remains a
testnet/private-network release candidate, not a mainnet-certified node.
