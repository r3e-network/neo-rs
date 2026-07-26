# Release Guide

Steps to cut a production release for `neo-rs` (Rust Neo N3 node).

## Pre-flight
- Bump versions in `Cargo.toml` (workspace/package) and update `CHANGELOG.md` with a dated entry.
- Update compatibility statements in `README.md` and `CHANGELOG.md` for the target Neo N3 release (currently v3.10.1).
- Run: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Check the three crates the `--workspace` flags above do **not** reach. Each has
  its own manifest and lockfile, so both `cargo test --workspace` and
  `cargo update --workspace` skip them entirely:

  ```bash
  for c in fuzz neo-gui benchmarks/bench-client; do (cd "$c" && cargo check --all-targets) || break; done
  ```

  This matters most for `fuzz/`, the only one with `path` dependencies on
  workspace crates: its `version` pins sat at 0.11.1 through the 0.12.0 and
  0.13.0 bumps, and cargo could not satisfy them against the bumped path
  sources, so all three fuzz targets failed to build before fuzzing started.
  Verify with `git ls-files '*/Cargo.toml'` against `cargo metadata --no-deps`
  that this list is still complete when adding a crate.
- Optionally build binaries locally: `cargo build --release --workspace`.

## Tagging
- Tag the commit: `git tag -a vX.Y.Z -m "neo-rs vX.Y.Z"` and push: `git push --follow-tags`.
- The release workflow triggers on tags (`v*`); it builds and pushes a Docker image to GHCR if enabled.

## Docker publish (GHCR example)
- Ensure `REGISTRY` (default `ghcr.io`) and `IMAGE_NAME` (default `${{ github.repository_owner }}/neo-rs`) are correct in `.github/workflows/release.yml`.
- The workflow logs in with `GITHUB_TOKEN` and publishes two tags on release tags:
  - `ghcr.io/<owner>/neo-rs:latest`
  - `ghcr.io/<owner>/neo-rs:<tag>` (e.g., `vX.Y.Z`)
- For manual runs, use the workflow dispatch with custom `image_tag` if desired.

## Artifacts (optional)
- If you need tarballs/zip distributions, add an extra job to build and upload `neo-cli` and sample configs from `dist/`.

## Post-release
- Announce release notes (summarize CHANGELOG).
- Update deployment configs to pin the new image tag.
- Monitor rollout (height parity, peers, RPC health) and be ready to roll back to the previous tag if needed.
