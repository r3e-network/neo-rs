# Rust Toolchain, MSRV, Lints and Compiler Performance Policy

## Overview

neo-rs builds with exactly one pinned stable Rust toolchain. That pin is the
single source of truth for local builds, CI and the Docker image, and it is also
the declared MSRV (`rust-version`). This document records the pin, how it is
enforced, which newer language/library features the codebase adopts, the lint
policy, and the compiler-level performance knobs that have been evaluated.

| Item | Value | Where |
|---|---|---|
| Pinned toolchain | `1.99.1` | `rust-toolchain.toml` |
| MSRV (`rust-version`) | `1.99` | `[workspace.package]` in `Cargo.toml` |
| Edition | `2024` | `[workspace.package]` in `Cargo.toml` |
| Windows host | `x86_64-pc-windows-gnullvm` (LLVM-MinGW) or `x86_64-pc-windows-msvc` | local choice |
| Fuzzing toolchain | `nightly` | `fuzz/rust-toolchain.toml` |
| Docker builder | `rust:1.99.1-bookworm` | `Dockerfile` |

## Why MSRV equals the pinned toolchain

neo-rs ships a node binary; the crates are not consumed as general-purpose
libraries. Keeping MSRV below the pin would require a second CI toolchain and
would forbid the 1.96–1.99 APIs listed below. The dependency tree is not the
constraint: the highest `rust-version` in `Cargo.lock` is 1.88 (`time`).

Raising the pin is a deliberate change: bump `rust-toolchain.toml`,
`rust-version`, the `Dockerfile` base image and the table above in one commit,
then run the verification steps at the end of this document.

## Usage

```bash
# rustup reads rust-toolchain.toml and installs 1.99.1 + rustfmt + clippy on first use.
# `rustc -vV` inside the repo must report 1.99.1. A standalone Rust install earlier
# on PATH (this machine has one at "C:\Program Files\Rust stable LLVM 1.95") ignores
# the pin; use %USERPROFILE%\.cargo\bin\cargo.exe in that case.
cargo build --workspace

# Fuzzing: fuzz/rust-toolchain.toml selects nightly automatically.
cd fuzz && cargo fuzz run fuzz_transaction_parse --release

# Forward-compatibility check against whatever "stable" is today.
cargo +stable check --workspace --all-targets --locked

# Deny every warning (Cargo >= 1.97) without invalidating the build cache.
CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --all-targets
```

CI installs the pinned toolchain via `dtolnay/rust-toolchain@master` with
`toolchain: "1.99.1"`; `rust-toolchain.toml` would override any other choice
anyway. The `stable-forward` CI lane deliberately runs `cargo +stable` so a new
stable release that breaks the build is noticed before the pin is moved.

## Lint policy

All workspace members inherit `[workspace.lints]` through
`[lints] workspace = true`. Before this policy existed, the workspace lint table
was defined but no crate opted in, so it had no effect.

* The lint table encodes what CI enforces. The allow-list that used to live on
  the CI `cargo clippy` command line (`type_complexity`, `too_many_arguments`,
  …) now lives in `Cargo.toml`, so local runs and CI agree.
* `clippy::pedantic` is not enabled as a group. It produced thousands of
  warnings with low signal for this codebase; individual pedantic lints that
  encode adopted idioms (for example `manual_let_else`) are enabled one by one.
* CI denies warnings with `CARGO_BUILD_WARNINGS=deny` (Cargo 1.97+) instead of
  `RUSTFLAGS=-Dwarnings`, which keeps the build cache valid.

## Adopted language and library features

| Since | Feature | Adoption rule |
|---|---|---|
| 1.65 | `let … else` | Required for early-return destructuring (`clippy::manual_let_else`). |
| 1.88 (edition 2024) | let chains (`if let A = x && let B = y`) | Preferred over nested `if let` (`clippy::collapsible_if`). |
| 1.96 | `assert_matches!` / `debug_assert_matches!` | Preferred over `assert!(matches!(..))`. Not in the prelude; the codebase invokes it as `std::assert_matches!(..)`. The matched value must implement `Debug`. |
| 1.99 | `Atomic*::update` | Replaces the deprecated `fetch_update` when the closure is infallible (`try_update` otherwise). |
| 1.96 | `core::range::Range` (`Copy`) | Allowed for stored spans; public APIs keep `impl RangeBounds`. |
| 1.98 | `{integer}::format_into(&mut NumBuffer)` | Use when profiling shows integer-to-text on a hot path. Today none qualifies: RPC/JSON numbers go through `serde_json` (`itoa`/`ryu`). |
| 1.99 | `Box::into_non_null`, `Vec::into_parts` | Preferred over raw-pointer round trips in `unsafe` code. `ArenaMemoryPool` stores `NonNull` from `Box::into_non_null`. |

Explicitly **not** adopted:

* `f32/f64::algebraic_*` (1.98). Results are non-deterministic by design, which
  is incompatible with consensus-critical code. No consensus path uses floats.
* Replacing intentionally permanent `Box::leak` calls. The 1.99 guidance is
  against leak-then-free round trips; `neo-rpc/src/server/jsonrpsee_adapter.rs`
  leaks method names for the process lifetime and never frees them.

## Compiler-level performance

Release builds already use `lto = "fat"`, `codegen-units = 1`,
`panic = "abort"`, and `neo-node` uses mimalloc. The table below lists the
remaining knobs and their status. Measured numbers live in
[`performance-baselines.md`](performance-baselines.md#toolchain-and-target-cpu).

| Knob | Status | Notes |
|---|---|---|
| Toolchain 1.95 → 1.99 | Adopted | Same host, geomean −2.7%. VM stack `push/pop` −51%. |
| v0 symbol mangling (1.97 default) | Adopted automatically | Readable generic symbols in pprof/flamegraphs. |
| `-C target-cpu=x86-64-v3` | Opt-in only | Same host, geomean +0.8% (slower). Binary fails on CPUs without AVX2/BMI2. Never a default. |
| PGO | Opt-in script | MSVC host, geomean −4.5% vs 1.95 and about −1.6% beyond plain 1.99. In-sample: the benches were the training run. |
| Build parallelism | Default (all cores) | `.cargo/config.toml` no longer caps `jobs`; use `CARGO_BUILD_JOBS=N` on memory-constrained machines. |

### Opt-in CPU-specific build

```bash
RUSTFLAGS="-C target-cpu=x86-64-v3" cargo build --profile production -p neo-node --features full
```

Integer and hashing code is bit-for-bit identical across `target-cpu`
settings; only instruction selection changes. Only use this when the deployment
fleet is known to support x86-64-v3.

### Profile-guided optimisation

```bash
scripts/pgo-build.sh            # instrument -> train on benches -> merge -> optimised build
PGO_TRAIN_CMD="..." scripts/pgo-build.sh   # custom training workload, e.g. a mainnet replay
```

Requires the `llvm-tools` rustup component (the script installs it). The
optimised binary is written to `target/pgo/production/neo-node`.

On Windows, run it with the MSVC host (`RUSTUP_TOOLCHAIN=1.99.1-x86_64-pc-windows-msvc`).
On `x86_64-pc-windows-gnullvm` the instrumented binaries crash during training
(exit 139 in the `hashing` suite, observed with 1.99.1). Linux is unaffected.

## Verification

A toolchain change is complete when all of the following pass on the new pin:

1. `cargo fmt --all --check`
2. `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --all-targets --locked`
3. `cargo test --workspace --locked`
4. `cargo check --locked -p neo-node --features full,tee,hsm`
5. `cargo +stable check --workspace --all-targets --locked` (forward compatibility)
