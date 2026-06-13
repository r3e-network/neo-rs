# Cross-Crate Consistency Audit — Standards & Fix Plan

## 1. Consistency scorecard

| Dimension | Overall verdict | # high | # medium | # low |
|---|---|---|---|---|
| Lint directives / unsafe-code handling | mostly | 4 | 2 | 2 |
| Error handling (CoreError / thiserror / no `Result<_,String>`) | mostly | 6 | 11 | 3 |
| Cargo.toml manifest consistency | mostly | 3 | 4 | 18 |
| Shared-macro usage / boilerplate dedup | consistent | 0 | 1 | 7 |
| Module & file organization | consistent | 0 | 3 | 11 |
| Documentation consistency | mostly | 0 | 0 | 12 |
| Naming conventions & API idioms | mostly | 0 | 5 | 9 |
| Test organization & style | mostly | 0 | 3 | 3 |

Net: the workspace is genuinely consistent on the structural axes (module layout, shared macros, doc presence) and the deviations cluster in two real backlogs — the `Result<_,String>` typed-error migration (~334 sites) and the unenforced-because-hand-duplicated lint policy.

---

## 2. Canonical standard (additions to CONVENTIONS.md)

### 2.1 Lint policy — centralize via `[workspace.lints]`

Root `Cargo.toml` gains:

```toml
[workspace.lints.rust]
unsafe_code = "deny"
missing_docs = "warn"
```

Every crate's `Cargo.toml` gains:

```toml
[lints]
workspace = true
```

…and the two hand-duplicated header lines (`#![deny(unsafe_code)]` / `#![warn(missing_docs)]`) are **deleted** from each `lib.rs`/`main.rs`. `#![doc(html_root_url = …)]` is NOT a lint and stays in `lib.rs`.

**The two unsafe crates (neo-vm, neo-execution)** keep `[lints] workspace = true` (so the `deny` still applies) and opt out **per-site**:

```rust
// SAFETY: <invariant>
#[allow(unsafe_code)]
unsafe { … }
```

Forbidden everywhere: blanket `#![allow(dead_code)]`, `#![allow(unused_imports)]`, `#![allow(deprecated)]`, crate-level `#![allow(unsafe_code)]`, and the inert `#![warn(rustdoc::missing_crate_level_docs)]`. Dead code is deleted or carries a per-item `#[allow(dead_code)]` + reason; unused imports are scoped with `#[cfg(test)]` or a per-`use` allow.

### 2.2 Manifest shape

`[package]` inherits **all** shared keys via `.workspace = true`: `version`, `edition`, `rust-version`, `authors`, `homepage`, `repository`, `documentation`, `license`, `keywords`, `categories`, `readme`. The **only** per-crate `[package]` literal is `description`. This binds workspace members `tests` and `benches-package` too (they are real members — no hardcoded `version`/`edition`). Internal deps: always `{ workspace = true }`, never `path =`. External deps already in `[workspace.dependencies]` must reference `{ workspace = true }`; a per-crate version literal is allowed **only** for a single-consumer dep that is not in the workspace table. Dependency value style: inline-table `foo = { workspace = true }` (drop dotted-key `foo.workspace = true`). Section order: `[package]` → `[dependencies]` → `[dev-dependencies]` → `[build-dependencies]` → `[features]` → target tables (`[[bin]]`/`[[test]]`/`[[bench]]`). Drop redundant `features = ["derive"]` on serde (workspace serde already enables it).

### 2.3 Error policy (codifies existing CONVENTIONS §Error Handling)

Cross-crate fallible APIs return `neo_error::CoreResult<T>` spelled in full — never aliased to `Result`/`Error`, and **no generic `Result` alias may be defined or re-exported** (remove `neo-error`'s `pub type Result`, `neo-vm`'s dead `pub type Result`). Crate-internal domain errors are a `#[derive(Debug, thiserror::Error)] pub enum XxxError` with `#[error("…")]` on every variant — never hand-rolled `Display`/`std::error::Error`. `Result<_, String>` is **forbidden** in library code: map to a `CoreError` variant or a crate `thiserror` variant (preserve C#-parity fault-message text where present). Only `neo-node` may use `anyhow` (this makes neo-rpc's feature-gated anyhow a violation). `neo-config` is the reference migration (`ProtocolConfigError`).

### 2.4 Shared macros

When `Default` is literally `Self::new()` (no-arg, non-generic) and the crate depends on neo-io, use `neo_io::impl_default_via_new!(X)`. Plain field-by-field `Hash` → `neo_io::impl_hash_for_fields!`; plain field-comparison `Ord` → `neo_io::impl_ord_by_fields!`; plain field-list wire codec → `impl_serializable!`. A type may NOT carry both a macro-generated trait `Default` and a hand-written inherent `default()` (the JumpTable smell) — pick one path. Macros are inapplicable to generic/bounded types, derived-value ordering, pointer-identity hashing, and consensus-critical custom codecs — those stay hand-written.

### 2.5 Module / file organization

One independent public type per self-named file. `lib.rs`/`mod.rs` are pure re-export hubs with **zero** definitions and **explicit** re-exports (`pub use sub::Type;`, never `pub use sub::*;` — including `#[cfg(test)]` test modules). Tight coupled clusters (channel-actor `Handle`+`Service`+`Command`+`State`, reth service-trait families, C#-parity nested sub-payloads) may co-locate.

### 2.6 Documentation voice

Crate doc opens with `//! # neo-<crate-name>` (code-name form, not Title-Case), then a one-line summary. Lint directives precede the `//!` block (moot once `[workspace.lints]` lands). Public-item docs use third-person present (`Creates`, `Returns`, `Decodes`). `# Errors`/`# Panics`/`# Safety` where applicable; `// SAFETY:` on every unsafe site. C#-parity prose is intentional.

### 2.7 Naming idioms

Constructors: `new()` infallible, `try_new()` fallible, `with_*()` variant, `from_*()` conversion; `create_*` **only** for C#-parity factory mirrors (`Contract.Create*`, `StorageKey.CreateSearchPrefix`). Getters: bare `field()` (no `get_`). Setters: `set_field()`. Conversions: `as_*` borrow / `to_*` owned-Vec / `into_*` consuming / `to_array` reserved for `[u8; N]`. Extension traits: `*Ext`. Predicates: `is_/has_/can_`. Config: `*Config` = single block, `*Settings` = aggregate/C#-parity mirror.

### 2.8 Test organization

Unit tests inline as `#[cfg(test)] mod tests { … }`; large suites may use a sibling `tests.rs`. A crate `tests/` dir is integration-only, **one binary per `.rs` file, no `mod.rs`/`#[path]` aggregator** re-declaring auto-discovered siblings (avoids double-compile/run). Test fns use descriptive snake_case behavior names without a `test_` prefix. `#[tokio::test]` by default; `flavor = "multi_thread"` only for real server/IO runtimes. C#-parity vectors named `*_matches_csharp` / `*_pins_csharp_metadata`, files `*_pinning.rs`.

---

## 3. Fix plan — batched for safe application

Mechanical-safe, high-value first; needs-care and the big migrations later.

| # | Batch | Dimension | Crates | fix_kind | ~edits | Verify |
|---|---|---|---|---|---|---|
| B1 | Drop inert `#![warn(rustdoc::missing_crate_level_docs)]` | lint | neo-vm:135, neo-consensus:222 | mechanical-safe | 2 | `cargo check --workspace` |
| B2 | Delete dead error aliases | error | neo-vm error.rs:643 (also B11 for neo-error) | mechanical-safe | 1 | `cargo check -p neo-vm` |
| B3 | Complete `[package]` metadata inheritance | manifest | neo-execution, neo-native-contracts, neo-system, tests, benches-package | mechanical-safe | ~14 | `cargo metadata` / `cargo check --workspace` |
| B4 | Redundant version pins → `{ workspace = true }` | manifest | neo-state-service (lru), neo-execution (num-integer), neo-rpc (anyhow, subtle) | mechanical-safe (anyhow=needs-care, §4) | 4 | `cargo check --workspace`, diff `Cargo.lock` (no change) |
| B5 | Dev-dep workspace refs + tempfile drift | manifest | neo-tee (tokio, tempfile 3.10→=3.23.0) | mechanical-safe | 2 | `cargo check -p neo-tee --tests` |
| B6 | Normalize dotted-key → inline-table, drop redundant serde `derive`, fix `[features]` order | manifest | neo-node, neo-mempool, neo-state-service, neo-io, neo-tee, neo-crypto, neo-network, neo-consensus | mechanical-safe | ~25 | `cargo check --workspace` |
| B7 | Glob re-exports → explicit | module org | neo-primitives lib.rs:85, blockchain/mod.rs:7 | mechanical-safe (lib.rs:85 needs-care, §4) | 2 | `cargo check -p neo-primitives` |
| B8 | Split grab-bag files into one-type-per-file | module org | neo-config (genesis.rs→5), neo-crypto (encoding.rs→3) | mechanical-safe | ~8 files | `cargo check -p neo-config -p neo-crypto` |
| B9 | Crate-doc H1 + voice polish | docs | all (H1 casing 10+4 crates; lint/`//!` order 10 crates; `Create`→`Creates`) | mechanical-safe | ~140 prose | `cargo doc --no-deps` |
| B10 | Restore `#![deny(unsafe_code)]` on unsafe crates + per-site allows | lint | neo-vm (21 sites), neo-execution (1 site, state.rs:203) | needs-care | ~24 | `cargo check -p neo-vm -p neo-execution` — a missed site fails the build |
| B11 | Centralize lints into `[workspace.lints]` + per-crate `[lints]` | lint | root + all 25 | needs-care | ~75 | `cargo check --workspace`; the two unsafe crates inherit the deny + keep per-site allows (do AFTER B10) |
| B12 | Triage blanket `#![allow(dead_code)]` / `#![allow(unused_imports)]` | lint | neo-execution (103 dead warns), neo-native-contracts, neo-wallets | needs-care — **warning flood** | per-crate | remove allow → `cargo check -p <crate>` → triage → narrow per-item allows |
| B13 | JumpTable double-Default reconcile | macro | neo-vm jump_table/mod.rs:46/72 | needs-care — preserve cached fast path | ~3 | `cargo check -p neo-vm`, clippy |
| B14 | Plain field-hash → `impl_hash_for_fields!` | macro | neo-storage (StorageKey), neo-crypto (ECPoint) | needs-care — Hash bit-identical | 2 | `cargo test -p neo-storage -p neo-crypto` (HashMap/HashSet behavior) |
| B15 | Hand-rolled `Display`/`Error` → thiserror | error | neo-rpc (RpcError struct, ClientRpcError) | needs-care — Display feeds wire responses | 2 | `cargo test -p neo-rpc --features server` |
| B16 | `Result<_,String>` → typed: leaf-parser crates | error | neo-p2p (6), neo-crypto (1), neo-system (1), neo-payloads (4) | mechanical-safe | ~12 | `cargo check --workspace` |
| B17 | `Result<_,String>` → typed: small-surface crates | error | neo-serialization (20+aliases), neo-vm (9), neo-wallets (8), neo-primitives (6), neo-blockchain (5), neo-state-service (2), neo-manifest (23) | needs-care | ~73 | per-crate `cargo test` |
| B18 | `Result<_,String>` → typed: large surfaces | error | neo-oracle-service (17), neo-execution (95) | needs-care — **preserve C# fault text** | ~112 | `cargo test -p neo-execution`; parity fault-message check |
| B19 | `Result<_,String>` → typed: neo-rpc | error | neo-rpc (139, incl. client `from_json` family) | needs-care — ripples into jsonrpsee response mapping | ~139 | `cargo test -p neo-rpc --features server` |
| B20 | Remove `tests/` aggregators causing double-run | test org | neo-io (tests/mod.rs), neo-serialization (tests/json_mod.rs) | needs-care | 2 files | `cargo test -p neo-io -p neo-serialization` (no coverage loss) |
| B21 | Naming: drop redundant `get_hash`, rename non-parity `create_*`/`get_*`/`to_array` | naming | neo-payloads, neo-p2p, neo-storage, neo-vm, neo-consensus, neo-wallets | needs-care — public rename, update call sites | ~25 | `cargo check --workspace` |
| B22 | Extension-trait suffix unify on `*Ext` | naming | neo-io (5 `*Extensions`) | needs-care — trait rename, all `use` sites | ~5 | `cargo check --workspace` |
| B23 | Test-fn `test_` prefix strip (opportunistic, per-crate) | test org | neo-primitives, neo-crypto, neo-storage, neo-io, neo-tee, neo-consensus (~880) | mechanical-safe — defer/opportunistic | ~880 | `cargo test -p <crate>` |

**Consensus / risk callouts:** B18 (neo-execution VM fault text) and B17 (neo-state-service `StateRootComputer`, neo-blockchain relay-rejection strings) touch consensus-relevant message text — preserve wording. B12 surfaces a 103-warning flood in neo-execution alone (plus unknown counts in the other two) — never remove blind. B13/B14 touch the deliberate two-tier VM fast path and HashMap key behavior. B11 will hard-break neo-vm/neo-execution if applied before B10.

---

## 4. Keep-with-reason

- **neo-vm / neo-execution crate-level unsafe** — justified (`get_unchecked` hot paths, raw `InteropHost`/interop-host pointer); keep the `deny` + per-site `#[allow(unsafe_code)]`+SAFETY, never a blanket opt-out.
- **neo-config `HardforkManager` hand-written Default** — converting to the macro would force a `neo-config → neo-io` dependency, inverting layering. Keep.
- **`OrderedDictionary<K,V>` Default** — `impl_default_via_new!` cannot express generics/bounds. Keep.
- **nep_11/nep_17 key `Ord` impls, Transaction/Script `Hash`** — order/hash a derived value (`token_integer()`) or pointer/cached-hash identity, not raw fields; the macros would produce wrong behavior. Keep.
- **29 hand-written `Serializable` impls** — consensus-critical wire types (Header/Block/Transaction/witnesses) with cached hashes, conditional witness arrays, parity-sensitive var-int counts. Keep.
- **C#-parity factory names** — `Contract.Create*` (neo-execution), `StorageKey.CreateSearchPrefix`, `ContractPermissionDescriptor.Create*` (neo-manifest), `GetSpan` macro alias. Keep; document parity intent.
- **`*Config` vs `*Settings` mix** — partly C#-parity (`ProtocolSettings`, `OracleServiceSettings`); RpcServerConfig(block)/RpcServerSettings(aggregate) is a real two-concept split. Keep; document the convention rather than churn-rename.
- **Cohesive multi-type files** — neo-consensus `recovery.rs` (nested dBFT sub-payloads), neo-payloads `event_handlers.rs` (documented plugin-callback consolidation), neo-network `remote_node.rs`/`task_manager.rs` (channel-actor clusters), neo-runtime `services.rs`/`outcome.rs` (reth service-trait family). Keep.
- **Test-only / parity targeted allows** — neo-system `back_compat.rs` `#![allow(missing_docs)]` (re-export shim), neo-consensus `#[allow(missing_docs)]` on `protocol_enum!` wire-enum variants. Keep.
- **neo-rpc external single-consumer pins** (jsonrpsee/rustls/TLS), neo-oracle-service grpc/build-deps, neo-wallets NEP-6 crypto cluster, neo-serialization zstd, neo-crypto hmac — single-consumer local pins are defensible. Keep unless a second consumer appears.
- **neo-rpc external `tests.rs` sibling modules** — legitimate split of large server suites; both forms idiomatic. Keep.
- **fuzz `path =` deps + 0.1.0 version** — deliberately workspace-excluded; cannot inherit. Not a violation.

---

## 5. Recommended execution order

1. **B1** — drop inert rustdoc directives (2 edits, zero risk).
2. **B2** — delete neo-vm dead `Result` alias (1 edit).
3. **B3** — complete `[package]` metadata inheritance (metadata-only).
4. **B4** — redundant version pins → workspace (drift hazard; lockfile unchanged).
5. **B5** — neo-tee dev-deps + tempfile drift fix.
6. **B6** — dotted-key→inline-table + serde-derive + `[features]` order (cosmetic manifest sweep).
7. **B7** — neo-primitives glob re-exports → explicit.
8. **B8** — split genesis.rs / encoding.rs into one-type-per-file.
9. **B10** — restore `#![deny(unsafe_code)]` on neo-vm/neo-execution with per-site allows (prerequisite for centralization).
10. **B11** — centralize lints into `[workspace.lints]` + `[lints] workspace = true` (after B10 so the unsafe crates don't break).
11. **B12** — triage blanket dead_code/unused_imports allows (neo-execution → native-contracts → wallets; warning-flood, dedicated cleanup).
12. **B13** — JumpTable double-Default reconcile.
13. **B14** — StorageKey/ECPoint field-hash → macro.
14. **B15** — neo-rpc hand-rolled error types → thiserror.
15. **B20** — remove neo-io/neo-serialization test aggregators (kill double-run).
16. **B16** — typed-error migration: leaf-parser crates (mechanical wave).
17. **B17** — typed-error migration: small/medium surfaces.
18. **B18** — typed-error migration: neo-oracle-service + neo-execution (preserve C# fault text).
19. **B19** — typed-error migration: neo-rpc (largest, ripples into jsonrpsee).
20. **B21** — naming renames (drop `get_hash`, non-parity `create_*`/`get_*`/`to_array`).
21. **B22** — extension-trait suffix unify on `*Ext`.
22. **B9** — doc H1/voice polish (low-risk, batch anytime; placed late as pure cosmetics).
23. **B23** — `test_`-prefix strip, opportunistically per-crate (defer; ~880 cosmetic renames).

Batches 1–8 are mechanical-safe and can land in a single low-risk session. 9–10 (lint restoration + centralization) are the highest-value structural fix. 16–19 are the real backlog (the tracked `Result<_,String>` task #15, ~334 sites) and should land per-crate with that crate's test gate green before moving on.