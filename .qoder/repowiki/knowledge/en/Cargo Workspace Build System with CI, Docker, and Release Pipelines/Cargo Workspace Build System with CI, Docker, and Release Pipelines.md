---
kind: build_system
name: Cargo Workspace Build System with CI, Docker, and Release Pipelines
category: build_system
scope:
    - '**'
source_files:
    - Cargo.toml
    - .cargo/config.toml
    - Makefile
    - Dockerfile
    - scripts/docker-entrypoint.sh
    - .github/workflows/ci.yml
    - .github/workflows/release.yml
    - deny.toml
---

## What system/approach is used

The project is built as a **Rust Cargo workspace** (`Cargo.toml` at the repository root) that composes ~15 crates (neo-primitives, neo-config, neo-crypto, neo-storage, neo-io, neo-json, neo-vm, neo-core, neo-p2p, neo-rpc, neo-consensus, neo-tee, neo-hsm, neo-telemetry, neo-node, plus `tests`, `benches-package`, and `fuzz`). The workspace declares a single version (`0.17.0`) and a shared `[workspace.dependencies]` table that pins every internal crate to matching versions and centralizes third-party dependency versions. A top-level `Makefile` wraps `cargo build/test/clippy/fmt/doc/check` into developer-friendly targets, while GitHub Actions workflows drive CI, fuzzing, MSRV checks, feature-matrix builds, and release artifact publishing.

## Key files and packages

- `Cargo.toml` — workspace definition, member list, default members, workspace-wide package metadata (version `0.17.0`, edition `2024`, `rust-version = "1.88"`), shared dependencies, and custom profiles (`release`, `dev`, `test`, `bench`, `production`).
- `.cargo/config.toml` — per-repo cargo config: limits parallel jobs to 2, sets debuginfo split for faster incremental builds.
- `Makefile` — developer entrypoint exposing `build`, `build-release`, `build-all`, `test`, `test-unit`, `test-integration`, `clippy`, `check`, `doc`, `run`, `run-testnet`, `run-mainnet`, `docker`, `docker-run`, `compose-up`, `db-clean/backup/restore`, `release`, `dist`, `preflight`, and `ci` targets.
- `Dockerfile` — multi-stage image: `rust:1.88-bookworm` builder installs `llvm-14`, `clang-14`, `libsnappy-dev`, `liblz4-dev`, `libzstd-dev`, `libbz2-dev`, `libssl-dev`; copies each crate explicitly for layer caching; builds with `cargo build --release --locked -p neo-node --features full`; runtime stage uses `debian:bookworm-slim` and runs via `scripts/docker-entrypoint.sh`.
- `scripts/docker-entrypoint.sh` — selects config based on `NEO_NETWORK` (mainnet → `neo_mainnet_node.toml`, testnet → `neo_testnet_node.toml`, else production), resolves storage path, validates write permissions, parses RPC port from TOML, and execs `neo-node` with `--config`, `--storage`, optional `--backend` and `--listen-port`.
- `.github/workflows/ci.yml` — jobs: `fmt` (`cargo fmt --all --check`), `clippy` (`--workspace --all-targets --profile test -D clippy::all` with many allowed lints), `test` (`cargo nextest run --workspace --no-fail-fast` + doctests), `test-runtime` (feature-gated `runtime` and consensus integration tests), `fuzz-smoke` (cargo-fuzz, 60s per target), `windows-msvc` (`cargo build --release --locked -p neo-node`), `node-full` (`--features full` check+test), `msrv` (toolchain `1.88`, `cargo check --locked --workspace`), `features` matrix (`tee`, `hsm`, `full,tee,hsm`), `deny` (license/bans/sources/advisories via `cargo-deny-action`), `protocol-consistency-goldens` (Python scripts against live seeds).
- `.github/workflows/release.yml` — triggers on `v*` tags or `workflow_dispatch`; uses `docker/setup-buildx-action` and `docker/build-push-action` to push multi-platform images to `ghcr.io/r3e-network/neo-rs` with tag derived from git ref.
- `deny.toml` — cargo-deny policy file referenced by CI.
- `neo-node/Cargo.toml` (referenced throughout) — the application crate whose binary is `neo-node`.

## Architecture and conventions

### Workspace layout
- Crates are grouped by layer in the workspace comment block: Foundation Layer (primitives, config, crypto, storage, io, json) → Core Layer (vm, core, p2p, rpc, consensus) → Infrastructure (tee, hsm, telemetry) → Application (`neo-node`) → Tests/Benchmarks/Fuzz. This ordering mirrors the dependency graph and is enforced by the workspace member list.
- All internal crates share the same version (`0.17.0`) declared once in `[workspace.package]` and re-exported through `[workspace.dependencies]`, so cross-crate version drift is impossible without editing the workspace manifest.
- Default members exclude heavy optional crates (`neo-tee`, `neo-hsm`, `tests`, `benches-package`, `fuzz`); they are pulled in only when needed via features or explicit `-p` flags.

### Profiles and optimization
- `profile.release`: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`, `incremental = false` — tuned for smallest, fastest binaries.
- `profile.production` inherits `release` and adds `debug = false`.
- `profile.dev`: no LTO, debug info enabled, overflow checks on.
- `profile.bench`: optimized, no debug, fat LTO.

### Feature gating
- Optional capabilities (`tee`, `hsm`, `full`, `runtime`, `server`) are gated behind Cargo features. CI explicitly compiles each combination (`neo-node --features tee`, `hsm`, `full,tee,hsm`) to ensure they remain compilable. The Docker image requires `--features full` because its shipped configs expect RocksDB.

### Cross-compilation / platforms
- CI builds on `ubuntu-latest` and `windows-latest` (MSVC). The release workflow uses `docker/setup-qemu-action` + `buildx` but currently publishes only `linux/amd64`. No cross-toolchain configuration beyond standard Rust toolchains is present.

### Versioning strategy
- Single monorepo version `0.17.0` in `[workspace.package]` applies to all crates. There is no per-crate versioning; bumping the workspace version is the canonical way to advance the project.
- The `rust-version` field is set to `1.88` (enforced by the `msrv` CI job using `dtolnay/rust-toolchain@master` with toolchain `1.88`).

### Testing & quality gates
- Formatting: `cargo fmt --all --check` (CI `fmt` job).
- Linting: `cargo clippy --workspace --all-targets --profile test` with a curated allowlist of lints (e.g., `large_enum_variant`, `module_inception`, `type_complexity`, `too_many_arguments`).
- Unit/integration tests: `cargo nextest run --workspace --no-fail-fast` plus doctests.
- Fuzzing: `cargo fuzz` targets for transaction/script/message parsing, bounded to 60s per target with RSS limits.
- Dependency policy: `cargo deny check licenses bans sources advisories`.
- Protocol consistency: Python scripts verify state roots and presets against live C# seed nodes.

### Docker & deployment
- Multi-stage image separates build deps from runtime deps. Runtime image exposes ports `20332/20333` (testnet), `103332/10333` (mainnet), `30332/30333` (private network) and includes a healthcheck that calls `getversion` on the detected RPC port.
- Environment variables control behavior: `NEO_NETWORK`, `NEO_CONFIG`, `NEO_STORAGE`, `NEO_BACKEND`, `NEO_PLUGINS_DIR`, `NEO_LISTEN_PORT`, `NEO_RPC_PORT`, `RUST_LOG`.
- `docker-compose.yml` (referenced by `make compose-*`) provides a stack including a monitoring profile with Grafana.

### Conventions and constraints
- Builds must use `--locked` in CI and Docker to pin the exact dependency tree (`Cargo.lock` is committed).
- The workspace enforces `edition = "2024"` and `resolver = "2"` globally.
- Debug symbols are split (`split-debuginfo = "unpacked"`) in dev/test profiles to speed up incremental rebuilds.
- Parallel compilation is capped at 2 jobs via `.cargo/config.toml` to reduce memory pressure on CI runners.
- The `neo-node` binary is the sole published artifact; there is no separate CLI crate — CLI functionality was merged into `neo-node` (noted in workspace comments).
- Database lifecycle is managed via Makefile targets (`db-clean`, `db-backup`, `db-restore`, `backup-rocksdb`) rather than ad-hoc scripts.
- Configuration validation is exposed as a CLI mode (`--check-config`, `--check-storage`, `--check-all`) invoked through `make check-config`, `make check-storage`, `make check-all`, and `make preflight`.