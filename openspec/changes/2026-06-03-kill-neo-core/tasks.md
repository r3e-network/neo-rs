# Kill `neo-core` — Tasks ✅ ALL COMPLETE

This change was executed in two stages. The first stage (commit `90e7468`) scaffolded the four new crates plus nine placeholder future-extraction crates. The second stage (this commit) wires the four new crates into `neo-core` (deps, re-exports, internal references, external consumer updates, pre-existing macro fix), finalises the OpenSpec, and lands the documentation updates.

## 1. Track A — `neo-error` ← `CoreError` ✅ COMPLETE

- [x] 1.1 Move `neo-core/src/error.rs` (593 lines) → `neo-error/src/error.rs`.
- [x] 1.2 Rewrite `use` statements in the moved file (only `neo_primitives::PrimitiveError`, `thiserror`).
- [x] 1.3 Add cross-crate `From` impls needed by other tracks: `From<neo_io::IoError>`, `From<neo_script_builder::ScriptBuilderError>`, `From<neo_script_builder::RedeemScriptError>`, `From<neo_storage::StorageError>`, `From<neo_storage::KeyBuilderError>`, `From<neo_vm::VmError>`. Documented as TODO to relocate each into the source crate per polkadot-sdk convention.
- [x] 1.4 Update `neo-error/Cargo.toml` deps to include `thiserror`, `neo-primitives`, `neo-io`, `neo-storage`, `neo-vm`, `neo-script-builder`, `neo-script-builder`.
- [x] 1.5 Add `neo-error` to `workspace.members` + `workspace.dependencies`.
- [x] 1.6 **Verify:** `cargo check -p neo-error` ✅ green. `cargo test -p neo-error` ✅ 7 unit + 1 doc passing.
- [x] 1.7 Update 89 internal `use crate::error::*` references in `neo-core` to `use neo_error::*`.
- [x] 1.8 Replace `pub mod error;` in `neo-core/src/lib.rs` with `pub use neo_error::{CoreError, CoreResult, Result, ToNativeError};`.
- [x] 1.9 Add `neo-error` as dep of `neo-core`, `neo-rpc`, `neo-chain`, `neo-tx-builder`, `neo-ledger-types`.
- [x] 1.10 Delete `neo-core/src/error.rs` (canonical home is now `neo-error/src/error.rs`).

## 2. Track B — `neo-ledger-types` ← `Witness` ✅ COMPLETE

- [x] 2.1 Move `neo-core/src/witness.rs` (522 lines) → `neo-ledger-types/src/witness.rs`.
- [x] 2.2 Rewrite `use` statements: `neo_error::{CoreError, CoreResult}`, `neo_io::{serializable::helper::get_var_size_bytes, Serializable}`, `neo_primitives::UInt160`, `neo_crypto::Crypto`, `neo_vm_rs::OpCode`, `base64`, `serde::{Deserialize, Serialize}`, `std::sync::OnceLock`, `std::{convert::TryInto, fmt}`. Remove `crate::neo_io::*` and `crate::UInt160` references.
- [x] 2.3 Add `pub mod witness;` to `neo-ledger-types/src/lib.rs` and a comprehensive crate-level doc explaining what belongs / doesn't belong.
- [x] 2.4 Update `neo-ledger-types/Cargo.toml`: added `neo-error`, `neo-crypto`, `neo-primitives`, `neo-io`, `neo-vm-rs`, `neo-script-builder`, `base64`, `hex`, `serde`, `serde_json`. Re-export `impl_default_via_new` / `impl_error_from` / `impl_from_bytes` / `impl_hash_for_fields` / `impl_ord_by_fields` from `neo-io`.
- [x] 2.5 Add `neo-ledger-types` to `workspace.members` + `workspace.dependencies`.
- [x] 2.6 **Verify:** `cargo check -p neo-ledger-types` ✅ green. `cargo test -p neo-ledger-types` ✅ 8 unit + 1 doc passing.
- [x] 2.7 Bulk-migrate external `neo_core::Witness` / `neo_core::witness::` references in `neo-rpc`, `neo-consensus`, `neo-p2p`, `neo-node` to `neo_ledger_types::Witness` / `neo_ledger_types::witness::`.
- [x] 2.8 `neo-rpc` and `neo-p2p` build clean (no pre-existing errors masked).
- [x] 2.9 Delete `neo-core/src/witness.rs` (canonical home is now `neo-ledger-types/src/witness.rs`).

## 3. Track C — `neo-chain` ← `block validation` ✅ COMPLETE

- [x] 3.1 Move `neo-core/src/validation.rs` (638 lines) → `neo-chain/src/block_validation.rs`.
- [x] 3.2 Rewrite `use` statements. **Decoupled from `Header` and `Transaction` types entirely**: `validate_witness_scripts(witness: &Witness)` now takes a `&Witness` reference, and `validate_merkle_root` / `validate_no_duplicate_transactions` now take `&[UInt256]` instead of `&[Transaction]`. Result: `neo-chain` has **zero** dependency on `neo-core`.
- [x] 3.3 Add `pub mod block_validation;` to `neo-chain/src/lib.rs` with comprehensive crate-level doc explaining what belongs / doesn't belong.
- [x] 3.4 Update `neo-chain/Cargo.toml` deps: `neo-crypto`, `neo-error`, `neo-primitives`, `neo-time`, `neo-ledger-types`, `thiserror`. No `neo-core` dep.
- [x] 3.5 Add `neo-chain` to `workspace.members` + `workspace.dependencies`.
- [x] 3.6 **Verify:** `cargo check -p neo-chain` ✅ green. `cargo test -p neo-chain` ✅ 22 unit passing.
- [x] 3.7 Add `neo-chain` as dep of `neo-core` and re-export the module: `pub use neo_chain::block_validation;` in `neo-core/src/lib.rs`.
- [x] 3.8 Delete `neo-core/src/validation.rs` (canonical home is now `neo-chain/src/block_validation.rs`).

## 4. Track D — `neo-time` (new) ✅ COMPLETE

- [x] 4.1 Create `neo-time/Cargo.toml` with `chrono` + `parking_lot` deps only. No `neo-*` deps.
- [x] 4.2 Move `neo-core/src/time_provider.rs` (124 lines) → `neo-time/src/lib.rs`. Add comprehensive crate-level doc explaining why it's a foundation crate.
- [x] 4.3 Add `neo-time` to `workspace.members` + `workspace.dependencies`.
- [x] 4.4 `cargo check -p neo-time` ✅ green. `cargo test -p neo-time` ✅ 1 unit + 1 doc passing.
- [x] 4.5 Update `neo-node/src/consensus.rs` to use `neo_time::TimeProvider` instead of `neo_core::time_provider::TimeProvider`. Add `neo-time` as a dep of `neo-node`.
- [x] 4.6 Add `neo-time` as dep of `neo-core`. Replace `pub mod time_provider;` in `neo-core/src/lib.rs` with `pub use neo_time::{TimeProvider, TimeSource};`.
- [x] 4.7 Delete `neo-core/src/time_provider.rs` (canonical home is now `neo-time/src/lib.rs`).

## 5. Cleanup ✅ COMPLETE

- [x] 5.1 Add stub `src/lib.rs` to `neo-client-api`, `neo-transaction-pool`, `neo-pipeline`, `neo-provider` (each with a one-paragraph crate-level doc explaining the reserved future role).
- [x] 5.2 Replace "Placeholder" stubs in `neo-codecs/src/lib.rs` and `neo-native-contracts/src/lib.rs` with proper crate-level docs.
- [x] 5.3 Fix pre-existing macro bug in `impl_native_contract!` and `neo_native_contract_methods!`: replace leftover `$neo_error::` placeholders with `::neo_error::` (the half-finished refactor had abandoned these as broken).
- [x] 5.4 Move orphan-rule-violating `impl From<KeyBuilderError> for CoreError` from `neo-core/src/smart_contract/key_builder.rs` to `neo-error` (where it now belongs).
- [x] 5.5 Bulk-migrate all 89 internal `crate::error::*` and 1 `crate::time_provider::*` references in `neo-core` to their new home crates.
- [x] 5.6 Bulk-migrate all external `neo_core::Witness`, `neo_core::error::*`, `neo_core::time_provider::*` references across the workspace.
- [x] 5.7 Update `neo-tx-builder` to use `neo_error` and `neo_ledger_types`; update its `Cargo.toml` accordingly.
- [x] 5.8 Add `neo-error`, `neo-time`, `neo-ledger-types`, `neo-chain` as deps of `neo-core` (with the new top-of-deps comment explaining the extraction).
- [x] 5.9 Verify `cargo check --workspace` ✅ green (0 errors).
- [x] 5.10 Verify `cargo test --workspace --lib` ✅ **2048 passed, 0 failed, 8 ignored across 27 test suites**.

## 6. Documentation ✅ COMPLETE

- [x] 6.1 `ARCHITECTURE.md` — updated Foundation / Protocol / Service / Application tables. The Foundation diagram now shows `neo-error` and `neo-time` alongside the existing foundation crates. The Layer 1 table was renamed from "Core Layer" to "Protocol Layer" and lists `neo-ledger-types` (was: `neo-core`). A new Layer 2 ("Service Layer") table lists `neo-chain` and the existing service crates. A final paragraph explains `neo-core`'s new role as a thin compatibility facade.
- [x] 6.2 `CHANGELOG.md` — added `[Unreleased]` section with breaking-changes migration table (old import path → new import path), list of internal cleanups, and verification results.
- [x] 6.3 `openspec/changes/2026-06-03-kill-neo-core/proposal.md` — refreshed with the final state.
- [x] 6.4 `openspec/changes/2026-06-03-kill-neo-core/tasks.md` — this file.
