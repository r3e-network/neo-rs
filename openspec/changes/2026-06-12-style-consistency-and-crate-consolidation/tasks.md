# Tasks — Style Consistency & Crate Consolidation

> Each task is a discrete, testable unit of work. Run `cargo test
> --workspace --lib` after each task to verify no regressions. All
> commands run from the workspace root.

## 1. Dead directory cleanup

- [x] 1.1 Delete `neo-data-cache/` directory (empty `src/`, not in workspace)
- [x] 1.2 Delete `neo-redeem-script/` directory (empty `src/`, not in workspace)
- [x] 1.3 Delete `neo-events/` directory (empty `src/`, not in workspace)
- [x] 1.4 Verify `cargo check --workspace` still passes

## 2. Stale version fix

- [x] 2.1 Update `fuzz/Cargo.toml`: change `version = "0.7.1"` to `version = "0.7.2"` for all 3 internal deps (`neo-io`, `neo-primitives`, `neo-p2p`)
- [x] 2.2 Verify `cargo check --workspace` still passes

## 3. Package metadata completeness

- [x] 3.1 Add missing metadata fields to `neo-vm/Cargo.toml`: `homepage`, `repository`, `documentation`, `keywords`, `categories`, `readme` (match the pattern from `neo-primitives/Cargo.toml`)
- [x] 3.2 Verify `cargo check -p neo-vm` still passes

## 4. Dependency style uniformity

- [x] 4.1 `neo-vm/Cargo.toml`: convert 4 internal deps from `path =` to `workspace = true` (`neo-io`, `neo-primitives`, `neo-crypto`, `neo-error`)
- [x] 4.2 `neo-io/Cargo.toml`: convert 1 internal dep from `path =` to `workspace = true` (`neo-primitives`)
- [x] 4.3 `neo-tokens-tracker/Cargo.toml`: convert 1 internal dep from `path =` to `workspace = true` (`neo-storage`)
- [x] 4.4 `neo-oracle-service/Cargo.toml`: convert 2 internal deps from `path =` to `workspace = true` (`neo-storage`, `neo-crypto`)
- [x] 4.5 `neo-rpc/Cargo.toml`: convert 3 optional internal deps from `path =` to `workspace = true` (`neo-application-logs`, `neo-tokens-tracker`, `neo-oracle-service`)
- [x] 4.6 `neo-node/Cargo.toml`: convert 8 internal deps from `path =` to `workspace = true` (`neo-tee`, `neo-hsm`, `neo-application-logs`, `neo-tokens-tracker`, `neo-oracle-service`, `neo-consensus`, `neo-p2p`, `neo-crypto`, `neo-rpc`)
- [x] 4.7 Verify all internal deps in workspace root `Cargo.toml` `[workspace.dependencies]` include any missing crates
- [x] 4.8 Verify `cargo check --workspace` still passes

## 5. Macro deduplication

- [x] 5.1 Remove `impl_default_via_new!` macro definition from `neo-primitives/src/lib.rs` (lines ~127-136). Keep the `pub use` re-export if needed for backward compat.
- [x] 5.2 Remove `impl_default_via_new!` macro definition from `neo-storage/src/lib.rs` (lines ~64-73)
- [x] 5.3 Update all call sites in `neo-payloads/` to use `neo_io::impl_default_via_new!` instead of `neo_primitives::impl_default_via_new!` (files: `witness_condition.rs`, `transaction_attributes.rs`, `signer.rs`)
- [x] 5.4 Update all call sites in `neo-execution/` to use `neo_io::impl_default_via_new!` (file: `execution_context_state.rs`)
- [x] 5.5 Update all call sites in `neo-crypto/` to use `neo_io::impl_default_via_new!` (file: `hash.rs`)
- [x] 5.6 Update all call sites in `neo-storage/` to use `neo_io::impl_default_via_new!` (files: `storage_item.rs`, `memory_store.rs`, `memory_store_provider.rs`)
- [x] 5.7 Verify `cargo test --workspace --lib` passes (all 17+ call sites work)

## 6. Lint directive standardization

### 6a. Add `#![deny(unsafe_code)]` + `#![warn(missing_docs)]` to crates with no directives

- [x] 6a.1 `neo-storage/src/lib.rs`
- [x] 6a.2 `neo-config/src/lib.rs`
- [x] 6a.3 `neo-script-builder/src/lib.rs` (will be merged into neo-vm later; do this first for consistency)
- [x] 6a.4 `neo-rpc/src/lib.rs`
- [x] 6a.5 `neo-p2p/src/lib.rs`
- [x] 6a.6 `neo-telemetry/src/lib.rs`
- [x] 6a.7 `neo-application-logs/src/lib.rs` (will be merged into neo-rpc later; do this first)
- [x] 6a.8 `neo-tokens-tracker/src/lib.rs`
- [x] 6a.9 `neo-oracle-service/src/lib.rs`
- [x] 6a.10 `neo-tee/src/lib.rs`
- [x] 6a.11 `neo-hsm/src/lib.rs`

### 6b. Fix crates with partial directives

- [x] 6b.1 `neo-primitives/src/lib.rs`: add `#![deny(unsafe_code)]` (already has `warn(missing_docs)`)
- [x] 6b.2 `neo-io/src/lib.rs`: add `#![deny(unsafe_code)]` (already has `warn(missing_docs)`)
- [x] 6b.3 `neo-crypto/src/lib.rs`: add `#![deny(unsafe_code)]` (already has `warn(missing_docs)`)
- [x] 6b.4 `neo-consensus/src/lib.rs`: add `#![deny(unsafe_code)]` (already has `warn(missing_docs)`)
- [x] 6b.5 `neo-node/src/main.rs`: add `#![deny(unsafe_code)]` (already has `warn(missing_docs)`)

### 6c. Fix crates that suppress warnings

- [x] 6c.1 `neo-execution/src/lib.rs`: change `#![allow(missing_docs)]` to `#![warn(missing_docs)]`. Add `#![deny(unsafe_code)]`. Keep `#![allow(dead_code)]` only if justified with a comment.
- [x] 6c.2 `neo-native-contracts/src/lib.rs`: change `#![allow(missing_docs)]` to `#![warn(missing_docs)]`. Add `#![deny(unsafe_code)]`. Keep `#![allow(dead_code)]` and `#![allow(unused_imports)]` only if justified.
- [x] 6c.3 `neo-wallets/src/lib.rs`: change `#![allow(missing_docs)]` to `#![warn(missing_docs)]`. Change `#![allow(unsafe_code)]` to `#![deny(unsafe_code)]` (the `transmute_copy` usage should be replaced with safe code or documented with `// SAFETY:`).
- [x] 6c.4 `neo-vm/src/lib.rs`: remove contradictory `#![allow(missing_docs)]` at line 136. Keep only the `#![warn(missing_docs)]` at line 6. Add `#![deny(unsafe_code)`.

### 6d. Verify

- [ ] 6d.1 Run `cargo clippy --workspace` and fix or add targeted `#[allow(...)]` with comments for any new warnings
- [ ] 6d.2 Verify `cargo test --workspace --lib` passes

## 7. Error handling convention

- [x] 7.1 Add a `CONVENTIONS.md` or section in existing docs documenting the error handling strategy:
  - Cross-crate: `CoreError`/`CoreResult` from `neo_error`
  - Crate-internal: per-crate `thiserror` types
  - Application boundary: `anyhow` only in `neo-node`
  - Import style: `use neo_error::{CoreError, CoreResult}` (no aliasing)
- [x] 7.2 Normalize `CoreError` import aliases:
  - `neo-execution/src/application_engine/mod.rs`: change `CoreError as Error` to `CoreError`
  - `neo-execution/src/native_contract.rs`: change `CoreResult as Result` to `CoreResult`
  - `neo-execution/src/native_contract_cache.rs`: change `CoreError as Error` to `CoreError`
  - `neo-manifest/src/manifest/contract_manifest.rs`: change `CoreError as Error, CoreResult as Result` to `CoreError, CoreResult`
  - `neo-manifest/src/manifest/contract_group.rs`: change `CoreError as Error, CoreResult as Result` to `CoreError, CoreResult`
  - `neo-manifest/src/manifest/contract_permission.rs`: change `CoreError as Error` to `CoreError`
- [x] 7.3 Verify `cargo test --workspace --lib` passes

## 8. Merge neo-script-builder into neo-vm

- [x] 8.1 Copy `neo-script-builder/src/lib.rs` contents into `neo-vm/src/script_builder.rs`
- [x] 8.2 Copy `neo-script-builder/src/redeem_script.rs` (if exists) into `neo-vm/src/script_builder/redeem_script.rs`
- [x] 8.3 Add `pub mod script_builder;` to `neo-vm/src/lib.rs`
- [x] 8.4 Add `neo-script-builder`'s unique dependencies to `neo-vm/Cargo.toml` (if any not already present)
- [x] 8.5 Update all 12 consumers to import from `neo_vm::script_builder` instead of `neo_script_builder`:
  - `neo-execution`
  - `neo-native-contracts`
  - `neo-consensus`
  - `neo-blockchain`
  - `neo-payloads`
  - `neo-wallets`
  - `neo-rpc`
  - `neo-mempool`
  - `neo-application-logs` (will be merged later)
  - `neo-oracle-service`
  - `neo-tokens-tracker`
  - `neo-node`
- [x] 8.6 Remove `neo-script-builder` from workspace members in root `Cargo.toml`
- [x] 8.7 Remove `neo-script-builder` from `[workspace.dependencies]` in root `Cargo.toml`
- [x] 8.8 Remove `neo-script-builder` dependency from all consumer `Cargo.toml` files
- [x] 8.9 Delete `neo-script-builder/` directory
- [x] 8.10 Verify `cargo check --workspace` passes
- [x] 8.11 Verify `cargo test --workspace --lib` passes

## 9. Merge neo-application-logs into neo-rpc

- [x] 9.1 Copy `neo-application-logs/src/` contents into `neo-rpc/src/application_logs/`
- [x] 9.2 Add `pub mod application_logs;` to `neo-rpc/src/lib.rs` (behind feature gate if needed)
- [x] 9.3 Add `neo-application-logs`'s unique dependencies to `neo-rpc/Cargo.toml` (if any not already present)
- [x] 9.4 Update `neo-node/Cargo.toml`: remove `neo-application-logs` optional dependency, ensure `neo-rpc` feature enables the module
- [x] 9.5 Update `neo-node/src/node.rs` (or wherever application-logs is used): import from `neo_rpc::application_logs` instead of `neo_application_logs`
- [x] 9.6 Remove `neo-application-logs` from workspace members in root `Cargo.toml`
- [x] 9.7 Remove `neo-application-logs` from `[workspace.dependencies]` in root `Cargo.toml`
- [x] 9.8 Remove `neo-application-logs` dependency from all consumer `Cargo.toml` files
- [x] 9.9 Delete `neo-application-logs/` directory
- [x] 9.10 Verify `cargo check --workspace` passes
- [x] 9.11 Verify `cargo test --workspace --lib` passes

## 10. Final verification

- [x] 10.1 `cargo check --workspace` — green, 0 errors
- [x] 10.2 `cargo clippy --workspace` — clean or only pre-existing warnings
- [x] 10.3 `cargo test --workspace --lib --no-fail-fast` — all tests pass (same count as before)
- [x] 10.4 `cargo test -p neo-tests --no-fail-fast` — all integration tests pass
- [x] 10.5 Verify workspace member count is 30 (down from 32): 28 library crates + 1 binary + 1 test crate + 1 bench crate
- [x] 10.6 Verify no dead directories remain (`neo-data-cache`, `neo-redeem-script`, `neo-events` gone)
- [x] 10.7 Verify no stale version references (`fuzz/Cargo.toml` at `0.7.2`)
