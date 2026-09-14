---
kind: dependency_management
name: Cargo Workspace with Centralized Dependency Management and cargo-deny Policy
category: dependency_management
scope:
    - '**'
source_files:
    - Cargo.toml
    - Cargo.lock
    - deny.toml
    - .cargo/config.toml
---

## What system/approach is used

The neo-rs monorepo uses **Cargo workspaces** as its sole dependency management system for Rust code. All third-party crates are declared centrally in the workspace root `Cargo.toml` under `[workspace.dependencies]`, and individual member crates reference them by name only (no version pinning at the crate level). A single `Cargo.lock` at the repository root pins every transitive dependency to an exact version plus checksum, ensuring reproducible builds across all members.

Security and licensing policy is enforced via **cargo-deny** (`deny.toml`), which audits the full dependency graph against a curated allowlist of licenses and a set of tracked security advisories that are explicitly ignored with documented reasons until upgrades land.

## Key files and packages

- `Cargo.toml` — workspace manifest declaring all 15+ member crates, shared `[workspace.dependencies]` versions, MSRV/rust-version, and release profiles.
- `Cargo.lock` — generated lockfile pinning every resolved crate (including transitive deps) with source registry and SHA256 checksums; committed to the repo.
- `deny.toml` — cargo-deny configuration: license allowlist, advisory ignore list, ban rules, and source registry restrictions.
- `.cargo/config.toml` — Cargo build config limiting parallel jobs to 2 and enabling split debuginfo for dev/test.
- Per-crate `Cargo.toml` files (e.g. `neo-core/Cargo.toml`, `neo-io/Cargo.toml`) — declare only feature flags and workspace dependency names, never versions.
- `fuzz/Cargo.toml` — separate cargo-fuzz workspace with its own `Cargo.lock`, isolated from the main workspace.

## Architecture and conventions

### Centralized version declarations
All external dependencies are pinned in one place under `[workspace.dependencies]`. Member crates depend on them by simple name (e.g. `tokio = { workspace = true }`), so updating a crate's version requires changing only the root manifest. Internal workspace crates (`neo-primitives`, `neo-config`, `neo-crypto`, `neo-storage`, `neo-io`, `neo-json`, `neo-vm`, `neo-core`, `neo-p2p`, `neo-rpc`, `neo-consensus`, `neo-tee`, `neo-hsm`, `neo-telemetry`, `neo-node`, `tests`, `benches-package`) are also centralized here with matching `0.17.0` versions.

### Lockfile-first reproducibility
`Cargo.lock` is committed and treated as the source of truth for builds. The comment at the top of the lockfile states it is "automatically @generated" and "not intended for manual editing." The workspace sets `rust-version = "1.88"` (with `msrv = "1.85"` in metadata) and a comment explains the lockfile pins `time 0.3.47` whose manifest requires rustc 1.88.

### Registry restriction
`deny.toml` restricts sources to crates.io only:
```
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
unknown-registry = "deny"
unknown-git = "deny"
```
No private registries or git-source dependencies are permitted without explicit approval through the deny policy.

### License policy
Only permissive/open-source licenses are allowed: MIT, Apache-2.0 (with LLVM exception), BSD-2/3-Clause, ISC, Unicode-3.0/DFS-2016, Zlib, CC0-1.0, MPL-2.0, OpenSSL, 0BSD, BSL-1.0, Unlicense. Private crates' licenses are ignored.

### Advisory management
Known vulnerabilities are acknowledged via explicit `ignore` entries with human-readable justifications (e.g. `RUSTSEC-2026-0190` for anyhow's `Error::downcast_mut`, `RUSTSEC-2025-0141` for unmaintained bincode, `RUSTSEC-2026-0258` for h2 empty DATA frames pulled via hyper 0.14). Each ignore includes a reason describing why the specific usage in neo-rs is not exploitable, and the comments state these are "tracked dependency-debt advisories" meant to stay green while upgrades land.

### Versioning strategy
Internal crates follow semantic versioning aligned at `0.17.0` across the entire workspace. External crates use caret ranges (`^`) for major compatibility but pin several critical ones exactly (e.g. `clap = "=4.5.53"`, `indexmap = "=2.8.0"`, `uuid = "=1.18.1"`, `tempfile = "=3.23.0"`, `proptest = "=1.7.0"`, `rayon = "=1.10.0"`, `ed25519-dalek = "=2.1.1"`).

### Feature-gated optional dependencies
Optional capabilities (TEE, HSM) are wired through workspace features rather than separate manifests, keeping the dependency graph minimal for default builds.

## Conventions and constraints

- **All dependencies must be declared in the workspace root** — member crates may not specify their own versions for workspace dependencies.
- **No vendoring** — no `vendor/` directory; all crates are fetched from crates.io at build time.
- **Lockfile is immutable by hand** — edits go through `cargo update`; the lockfile is committed to the repo.
- **No unknown registries or git sources** — `deny.toml` denies any dependency not published to crates.io.
- **Multiple versions of the same crate are warned** — `[bans].multiple-versions = "warn"` surfaces accidental duplication.
- **Wildcards are allowed** in version specs (`wildcards = "allow"`) but discouraged by convention since most deps are pinned precisely.
- **Advisory ignores require documented justification** — each entry in `deny.toml` includes a `reason` field explaining why the vulnerability does not apply to neo-rs's usage.
- **MSRV is fixed at workspace level** — `rust-version = "1.88"` in `[workspace.package]` and `msrv = "1.85"` in metadata define minimum supported compiler versions consistently across all crates.