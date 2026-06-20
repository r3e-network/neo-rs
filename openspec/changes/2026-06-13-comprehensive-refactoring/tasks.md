# Tasks — Comprehensive Refactoring & Protocol Completion

> Each task is a discrete, testable unit of work. Run `cargo test
> --workspace --lib` after each task to verify no regressions. All
> commands run from the workspace root.
>
> Phases are sequenced: A (dead code) → B (native-contract dedup) → C
> (3rd-party libs) → D (macro-ize RPC handlers) → E (storage + small crate
> merges). Phase F (protocol completeness) is documented separately.

---

## Phase A — Dead code elimination (1 day, highest ROI)

### A1. Delete `neo-telemetry` crate entirely

- [ ] A1.1 Verify zero production consumers: `grep -r "use neo_telemetry\|use ::neo_telemetry\|neo_telemetry::" --include="*.rs" neo-*/src` should return only self-references in `neo-telemetry/src/`
- [ ] A1.2 Remove `neo-telemetry` from `[workspace] members` in root `Cargo.toml:97`
- [ ] A1.3 Remove `neo-telemetry` from `[workspace.dependencies]` in root `Cargo.toml:184`
- [ ] A1.4 Remove `neo-telemetry` from `neo-blockchain/Cargo.toml:32` dep list
- [ ] A1.5 Remove `neo-telemetry` from `neo-node/Cargo.toml:43` dep list
- [ ] A1.6 Remove `neo-telemetry` from `tests/Cargo.toml:28` dep list
- [ ] A1.7 Update `tests/tests/layer_boundary_tests.rs:32,46` to remove `neo-telemetry` references
- [ ] A1.8 Delete `neo-telemetry/` directory
- [ ] A1.9 `cargo check --workspace` passes
- [ ] A1.10 `cargo test --workspace --lib` passes

### A2. Mark `neo-tee` and `neo-hsm` as "feature-gated, awaiting wallet integration"

- [ ] A2.1 Update `neo-tee/Cargo.toml` description: "Feature-gated TEE integration. Not yet wired to wallet/transaction-signing flows. Future-feature crate."
- [ ] A2.2 Update `neo-hsm/Cargo.toml` description: "Feature-gated HSM integration. Not yet wired to wallet/transaction-signing flows. Future-feature crate."
- [ ] A2.3 Add `#![warn(missing_docs)]` and `#![deny(unsafe_code)]` (already part of style change)
- [ ] A2.4 Update `neo-node/Cargo.toml` `tee`/`hsm` feature docs: "Not ready for production use — wallet/transaction-signing integration pending."
- [ ] A2.5 Verify `cargo check --workspace` still passes

### A3. Delete dead `prefixes` module

- [ ] A3.1 Verify zero consumers: `grep -r "neo_native_contracts::prefixes\|crate::prefixes" --include="*.rs" neo-*/src` should return zero matches
- [ ] A3.2 Remove `pub mod prefixes;` from `neo-native-contracts/src/lib.rs:45`
- [ ] A3.3 Delete `neo-native-contracts/src/prefixes.rs` (entire file)
- [ ] A3.4 `cargo check -p neo-native-contracts` passes
- [ ] A3.5 `cargo test -p neo-native-contracts --lib` passes

### A4. Delete dead `helpers` module

- [ ] A4.1 Verify zero consumers: `grep -r "NativeHelpers\|crate::helpers" --include="*.rs" neo-native-contracts/src` should return zero matches outside `helpers.rs`
- [ ] A4.2 Remove `pub mod helpers;` from `neo-native-contracts/src/lib.rs:68-70`
- [ ] A4.3 Delete `neo-native-contracts/src/helpers.rs` (entire file)
- [ ] A4.4 `cargo check -p neo-native-contracts` passes
- [ ] A4.5 `cargo test -p neo-native-contracts --lib` passes

### A5. Delete duplicate `BlockchainHandle`/`BlockchainCommand`

- [ ] A5.1 Verify the live node uses `neo-blockchain`'s canonical version (per `node-readiness-audit-2026-06-11.md:18`)
- [ ] A5.2 Remove `pub struct BlockchainHandle` and `pub struct BlockchainCommand` from `neo-runtime/src/blockchain.rs:21-86`
- [ ] A5.3 Keep `pub struct BlockchainEvent` (the one re-used)
- [ ] A5.4 Update any internal `neo-runtime` consumers to import from `neo-blockchain` instead
- [ ] A5.5 `cargo check --workspace` passes
- [ ] A5.6 `cargo test --workspace --lib` passes

### A6. (Subsumed by A3) Delete unused `NEP17_PREFIX_*` second declarations

- A3 deletes the entire `prefixes.rs` file, so the duplicates at `prefixes.rs:36-37` are removed automatically.

---

## Phase B — Native contract style consistency + dedup (1 week)

### B1. Hoist `assert_committee` into shared module

- [ ] B1.1 Create `neo-native-contracts/src/committee.rs` with the `assert_committee(engine, method)` function (moved from `policy_contract.rs:380-389`)
- [ ] B1.2 Add `pub mod committee;` to `neo-native-contracts/src/lib.rs`
- [ ] B1.3 Replace 7 inline copies:
  - `role_management.rs:350-356` → `crate::committee::assert_committee`
  - `treasury.rs:139-144` → `crate::committee::assert_committee`
  - `notary.rs:801-810` → `crate::committee::assert_committee`
  - `oracle_contract.rs:672-679` → `crate::committee::assert_committee`
  - `contract_management.rs:1209-1218` → `crate::committee::assert_committee`
  - `neo_token.rs:1749-1758` → `crate::committee::assert_committee`
  - `neo_token.rs:1776-1783` → `crate::committee::assert_committee`
- [ ] B1.4 Update `policy_contract.rs:380` to use the hoisted version
- [ ] B1.5 `cargo test -p neo-native-contracts --lib` passes (221 tests)
- [ ] B1.6 `cargo test -p neo-native-contracts --test native_manifest_pinning` passes (16 tests)

### B2. Hoist arg-parsing helpers into `args.rs`

- [ ] B2.1 Create `neo-native-contracts/src/args.rs` with:
  - `pub fn parse_account(args: &[StackItem], method: &str) -> CoreResult<UInt160>` (from `notary.rs:241`)
  - `pub fn hash160_arg(args: &[StackItem], index: usize, method: &str) -> CoreResult<UInt160>` (from `policy_contract.rs:693`)
  - `pub fn setter_int_arg(args: &[StackItem], method: &str) -> CoreResult<i64>` (from `policy_contract.rs:683`)
  - `pub fn attribute_type_arg(args: &[StackItem], method: &str) -> CoreResult<u8>` (from `policy_contract.rs:706`)
- [ ] B2.2 Add `pub(crate) mod args;` to `neo-native-contracts/src/lib.rs`
- [ ] B2.3 Replace inline copies in:
  - `gas_token.rs:600, :608, :614` (3× `hash160_arg`)
  - `neo_token.rs:1691, :2007, :2045, :2062` (4× `hash160_arg`)
  - `policy_contract.rs:1335, :1352` (2× `hash160_arg`)
  - `oracle_contract.rs:149, :162` (2× decode of `UInt160` field)
  - `notary.rs:170, :661` (2× `parse_account`)
  - `ledger_contract.rs:578, :604, :627, :669` (4× `UInt256::from_bytes` parse)
- [ ] B2.4 `cargo test -p neo-native-contracts --lib` passes

### B3. Implement or delete `PolicyContract::get_*_snapshot` stubs

**Decision point: implement or delete.**

- [ ] B3.1 Read the call sites:
  - `neo-rpc/src/server/wallet_compat.rs:108, :164`
  - `neo-rpc/src/server/rpc_server_wallet/mod.rs:871`
  - `neo-oracle-service/src/service/transactions/response.rs:50, :135, :156`
- [ ] B3.2 (Option A) **Implement**: use PolicyContract's typed storage-key helpers to read the live on-chain value. Drop the `_snapshot` underscore prefix.
- [ ] B3.3 (Option B) **Delete the stubs and migrate callers** to call the contract methods directly via `ApplicationEngine`.
- [ ] B3.4 Document the chosen approach in code comments
- [ ] B3.5 `cargo test --workspace --lib` passes

### B4. Remove duplicate `DEFAULT_*` constants

- [ ] B4.1 Remove `impl PolicyContract { pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30; pub const DEFAULT_FEE_PER_BYTE: u32 = 1000; pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760; }` at `policy_contract.rs:65-69`
- [ ] B4.2 Verify top-level `pub const DEFAULT_*` at lines 47-52 are still in scope
- [ ] B4.3 `cargo test -p neo-native-contracts --lib` passes

### B5. Hoist test helpers into `tests/common/mod.rs`

- [ ] B5.1 Create `neo-native-contracts/tests/common/mod.rs` with:
  - `pub fn deploy_native(cache: &DataCache, state: ContractState) -> Result<...>` (consolidated from 4× copies in `neo_token.rs:2808`, `treasury.rs:294`, `notary.rs:1136`, `policy_contract.rs:2385`)
  - `pub fn deploy_contract(cache: &DataCache, ...) -> ...` (from `oracle_contract.rs:1296`)
  - `pub fn hex(s: &str) -> Vec<u8>` (from 5× copies in `gas_token.rs:856`, `treasury.rs:247`, `policy_contract.rs:2337`, `neo_token.rs:2260`, `crypto_lib.rs:583`)
  - `pub fn hex_to_bytes(s: &str) -> Vec<u8>` (from `role_management.rs:430`)
  - `pub fn sample_committee() -> Vec<EcPoint>` (from 4× copies in `gas_token.rs:864`, `treasury.rs:254`, `policy_contract.rs:2344`, `neo_token.rs:2289`)
  - `pub const CM_PREFIX_CONTRACT: u8 = 8;`
  - `pub const NEO_PREFIX_COMMITTEE: u8 = 14;`
  - `pub const POLICY_PREFIX_ATTRIBUTE_FEE: u8 = 20;`
  - `pub fn deployable_manifest(name: &str) -> ContractManifest` (from 2× copies in `contract_management.rs:1793, :2300`)
  - `pub fn committee_address() -> UInt160` (from 3× copies in `policy_contract.rs:2380`, `treasury.rs:289`, plus the `compute_committee_address` production helper in `neo_token.rs:1125`)
- [ ] B5.2 Update each contract's test module to use `crate::tests::common::*` (or similar)
- [ ] B5.3 `cargo test -p neo-native-contracts --lib` passes (221 tests, same count)

### B6. Lift storage-key builders

- [ ] B6.1 Add `pub fn prefixed_storage_key(contract_id: i32, prefix: u8, suffix: &[u8]) -> StorageKey` to `neo-native-contracts/src/lib.rs` or a new `keys.rs`
- [ ] B6.2 Replace inlined helpers in:
  - `gas_token.rs:27-31` (`gas_account_key`)
  - `oracle_contract.rs:67-85` (`request_id_key`, `request_key`, `id_list_key`)
  - `notary.rs:69-73` (`deposit_key`)
  - `policy_contract.rs:256-261, :394-398, :466-471` (3 helpers)
  - `role_management.rs:129-134` (`designation_key`)
  - `ledger_contract.rs:260-302` (5 helpers)
  - `contract_management.rs:150-172` (3 helpers)
  - `neo_token.rs:962-974` (`candidate_key`, `neo_account_key`)
- [ ] B6.3 `cargo test -p neo-native-contracts --lib` passes

---

## Phase C — Third-party library consolidation (2 weeks)

### C1. Migrate RPC server from `warp` to `jsonrpsee`

- [ ] C1.1 Update `neo-rpc/Cargo.toml`:
  - Remove `warp = { version = "0.3", optional = true, features = ["compression-gzip"] }` (line 53)
  - Remove `"dep:warp"` from the `server` feature flag (line 120)
  - Confirm `jsonrpsee = { version = "0.24.10", optional = true, default-features = false, features = ["server"] }` is enabled
- [ ] C1.2 Promote `neo-rpc/src/server/jsonrpsee_adapter.rs` to the primary path
- [ ] C1.3 Delete the warp-based glue:
  - `neo-rpc/src/server/rpc_server.rs:255-460` (~205 LoC)
  - `neo-rpc/src/server/routes/mod.rs` (245 LoC)
  - `neo-rpc/src/server/routes/cors.rs` (entire file)
  - `neo-rpc/src/server/routes/handlers.rs` (384 LoC)
  - `neo-rpc/src/server/routes/tests.rs` (~350 LoC)
- [ ] C1.4 Update `neo-rpc/src/server/ws/handler.rs:10` to use `jsonrpsee::server::Server::ws()` API instead of `warp::ws::Message`
- [ ] C1.5 Verify all 49 RPC handlers still work through jsonrpsee
- [ ] C1.6 Add an integration test that spins up the RPC server via jsonrpsee and calls `getblockcount`/`getversion` (smoke test)
- [ ] C1.7 `cargo check -p neo-rpc` passes
- [ ] C1.8 `cargo test -p neo-rpc --lib` passes (6 tests)
- [ ] C1.9 `cargo test --workspace --lib` passes

### C2. Drop custom `Sha256Hasher` wrapper

- [ ] C2.1 In `neo-crypto/src/hash.rs:38-65`, replace `pub struct Sha256Hasher { inner: Sha256 }` with `pub type Sha256Hasher = sha2::Sha256;`
- [ ] C2.2 Delete `new()`, `update()`, `finalize()` methods (use the `sha2::Sha256` API directly)
- [ ] C2.3 `cargo test -p neo-crypto --lib` passes (141 tests)

### C3. Migrate `BigDecimal` → `bigdecimal` crate wrapper

- [ ] C3.1 Add `bigdecimal = { version = "0.4", features = ["serde"] }` to `[workspace.dependencies]` in root `Cargo.toml`
- [ ] C3.2 Add `bigdecimal = { workspace = true }` to `neo-primitives/Cargo.toml`
- [ ] C3.3 Refactor `neo-primitives/src/big_decimal.rs` (448 LoC) to a thin newtype:
  ```rust
  pub struct BigDecimal { inner: bigdecimal::BigDecimal, scale: u8 }
  ```
  that delegates arithmetic to the inner type while preserving Neo's 8-decimal fixed-point semantics
- [ ] C3.4 Keep the public API (`parse`, `Display`, `FromStr`, arithmetic operators) unchanged
- [ ] C3.5 `cargo test -p neo-primitives --lib` passes (222 tests)

### C4. Replace `Result<_, String>` with `thiserror` enums

- [ ] C4.1 Add `pub type Result<T> = core::result::Result<T, ModuleError>;` and `pub enum ModuleError { ... }` to:
  - `neo-rpc/src/client/utility.rs` (and sub-modules)
  - `neo-rpc/src/client/utility/parsing.rs`
  - `neo-rpc/src/client/models/*.rs` (37 files)
  - `neo-serialization/src/binary_serializer.rs` (11 sites)
  - `neo-serialization/src/json_serializer.rs` (12 sites)
  - `neo-wallets/src/wallet_helper.rs` (7 sites)
  - `neo-wallets/src/bip39.rs`
  - `neo-config/src/protocol.rs` (7 sites)
- [ ] C4.2 Replace `Result<T, String>` with the new types throughout each file
- [ ] C4.3 Use `#[derive(thiserror::Error, Debug)]` for each enum
- [ ] C4.4 Add `From<serde_json::Error>`, `From<std::io::Error>`, etc. `From` impls as needed
- [ ] C4.5 `cargo test --workspace --lib` passes (no regressions)

---

## Phase D — Macro-ize RPC handler boilerplate (1 week)

### D1. Introduce `rpc_method!` macro

- [ ] D1.1 In `neo-rpc/src/server/rpc_handler_macros.rs` (already has `rpc_handlers!`), add a `rpc_method!` declarative macro:
  ```rust
  rpc_method! {
      name = "getblockcount",
      handler = get_block_count,
      params = [],
      return = serde_json::Value,
  }
  ```
- [ ] D1.2 Implement auto-generated parameter extraction, error wrapping, and JSON serialization
- [ ] D1.3 Convert the 49 RPC handlers (in `rpc_server_blockchain/mod.rs`, `rpc_server_wallet/mod.rs`, `rpc_server_node/mod.rs`, `rpc_server_tokens_tracker/mod.rs`, `rpc_server_state.rs`, `smart_contract/invocation.rs`) to use the macro
- [ ] D1.4 `cargo test -p neo-rpc --lib` passes (6 tests)
- [ ] D1.5 `cargo test --workspace --lib` passes

### D2. Migrate RPC models to `#[derive(serde::Deserialize)]`

- [ ] D2.1 For each of the 37 model files in `neo-rpc/src/client/models/` (`rpc_account.rs`, `rpc_block.rs`, `rpc_block_header.rs`, `rpc_contract_state.rs`, `rpc_nep17_balances.rs`, `rpc_nep17_token_info.rs`, `rpc_nep17_transfers.rs`, `rpc_nep11_balances.rs`, `rpc_nep11_transfers.rs`, `rpc_application_log.rs`, `rpc_mempool_accepted.rs`, `rpc_mempool_unverified.rs`, etc.):
  - Add `#[derive(serde::Deserialize, serde::Serialize)]`
  - Replace hand-written `from_json`/`to_json` impls with `serde_json::from_value::<Model>(value)?` at dispatch sites
- [ ] D2.2 `cargo test -p neo-rpc --lib` passes

---

## Phase E — Storage key builder consolidation + small crate merges (3 days)

### E1. Add `key_builder!` macro

- [ ] E1.1 In `neo-storage/src/key_builder.rs` (currently 11 LoC), add:
  ```rust
  #[macro_export]
  macro_rules! key_builder {
      ($id:expr, $prefix:expr) => { StorageKey::create($id, $prefix) };
      ($id:expr, $prefix:expr, $($field:tt)+) => { ... }; // builder pattern
  }
  ```
- [ ] E1.2 Replace 28+ manual `key.push(PREFIX_*)` calls in:
  - `neo-blockchain/src/ledger_records.rs:69, :77, :85, :94`
  - `neo-native-contracts/src/role_management.rs:131`
  - `neo-native-contracts/src/contract_management.rs:153, :161, :169`
  - `neo-native-contracts/src/ledger_contract.rs:272, :280, :290, :299`
  - `neo-rpc/src/application_logs/service.rs:113, :125`
  - `neo-rpc/src/server/rpc_server_state.rs:308`
  - `neo-oracle-service/src/service/tests/response_tx.rs:38, :58`
  - `neo-tokens-tracker/tests/tokens_tracker_nep17_csharp_parity.rs:35`
  - `neo-rpc/src/server/test_support.rs:194, :202, :351, :367, :393, :404`
- [ ] E1.3 `cargo test --workspace --lib` passes

### E2. Add `strip_hex_prefix` helper

- [ ] E2.1 In `neo-primitives/src/uint_hex.rs`, expose `pub fn strip_hex_prefix(s: &str) -> &str { s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s) }` (handles both lowercase and uppercase)
- [ ] E2.2 Replace inline `strip_0x` definitions in:
  - `neo-p2p/src/witness_rule/helpers.rs:9-10`
  - `neo-primitives/src/uint_hex.rs:5-7` (inline; just re-export)
  - `neo-rpc/src/client/utility.rs:127, :153` (inline)
  - `neo-rpc/src/client/utility/witness_rule.rs:149` (inline)
  - `neo-oracle-service/src/neofs/json/helpers.rs:35` (inline)
- [ ] E2.3 `cargo test --workspace --lib` passes

### E3. Merge `neo-tokens-tracker` → `neo-rpc::plugins::tokens_tracker`

- [ ] E3.1 Copy `neo-tokens-tracker/src/` contents into `neo-rpc/src/plugins/tokens_tracker/`
- [ ] E3.2 Add `pub mod plugins { pub mod tokens_tracker; }` (feature-gated) to `neo-rpc/src/lib.rs`
- [ ] E3.3 Add `neo-tokens-tracker`'s unique dependencies to `neo-rpc/Cargo.toml`
- [ ] E3.4 Update `neo-rpc/src/server/rpc_server_tokens_tracker/` to import from `neo_rpc::plugins::tokens_tracker`
- [ ] E3.5 Move `neo-tokens-tracker/tests/tokens_tracker_nep17_csharp_parity.rs` to `neo-rpc/tests/`
- [ ] E3.6 Update `neo-node/Cargo.toml:89` to remove `neo-tokens-tracker` optional dep
- [ ] E3.7 Remove `neo-tokens-tracker` from workspace members in root `Cargo.toml`
- [ ] E3.8 Remove `neo-tokens-tracker` from `[workspace.dependencies]`
- [ ] E3.9 Delete `neo-tokens-tracker/` directory
- [ ] E3.10 `cargo check --workspace` passes
- [ ] E3.11 `cargo test --workspace --lib` passes

### E4. Extract `neo-rpc-types` leaf crate

- [ ] E4.1 Create `neo-rpc-types/` directory with `Cargo.toml`, `src/lib.rs`
- [ ] E4.2 Move `neo-rpc/src/client/models/*.rs` (37 files, 5,921 LoC) into `neo-rpc-types/src/models/`
- [ ] E4.3 Move `neo-rpc/src/parameter_converter/` into `neo-rpc-types/src/parameter_converter/`
- [ ] E4.4 Move `neo-rpc/src/error_code.rs` into `neo-rpc-types/src/error_code.rs`
- [ ] E4.5 Move `neo-rpc/src/client/rpc_error.rs` (the `RpcError` type) into `neo-rpc-types/src/rpc_error.rs`
- [ ] E4.6 Update all imports throughout `neo-rpc/src/` to use `neo_rpc_types::*`
- [ ] E4.7 Add `neo-rpc-types` to `[workspace.members]` and `[workspace.dependencies]`
- [ ] E4.8 `cargo check --workspace` passes
- [ ] E4.9 `cargo test --workspace --lib` passes

### E5. Feature-gate `neo-native-contracts` external deps

- [ ] E5.1 In `neo-native-contracts/Cargo.toml`, change:
  ```toml
  [features]
  default = []
  oracle = ["dep:url", "dep:reqwest"]
  cryptolib = ["dep:blst", "dep:bip39"]
  ```
- [ ] E5.2 Mark `url`, `reqwest`, `blst`, `bip39` as `optional = true`
- [ ] E5.3 Update `neo-native-contracts/src/oracle_contract.rs` `pub use` of `reqwest::Client` to be feature-gated
- [ ] E5.4 Update `neo-native-contracts/src/crypto_lib.rs` `pub use` of `blst`/`bip39` to be feature-gated
- [ ] E5.5 Update consumers (`neo-rpc`, `neo-oracle-service`, `neo-wallets`, etc.) to enable `oracle` and/or `cryptolib` features as needed
- [ ] E5.6 `cargo check --workspace --all-features` passes
- [ ] E5.7 `cargo build -p neo-rpc` succeeds without pulling `reqwest`/`blst`/`bip39` (verify via `cargo tree -p neo-rpc`)
- [ ] E5.8 `cargo test -p neo-native-contracts --all-features --lib` passes

---

## Phase F — Protocol completeness (separate proposal)

These tasks are documented in `PROTOCOL_VERIFICATION_REPORT.md` §12 and are **out of scope** for this change. They warrant a dedicated proposal:

- F1. JSON-driven `RpcTestCases.json` harness (46 cases). **Effort: 2 weeks.**
- F2. 4-validator dBFT end-to-end round producing a real `Block`. **Effort: 1 week.**
- F3. C# native-contract state-root replay vectors. **Effort: 1–2 weeks.**
- F4. Transaction-bearing mainnet block C# vector. **Effort: 2–3 days.**
- F5. Live `Version`/`Verack` handshake against a real C# peer. **Effort: 2–3 days.**
- F6. C# MPT root vectors. **Effort: 1 week.**
- F7. BLS12-381 draft-04 / RFC 6979 / BIP-32 test vectors. **Effort: 1 week.**
- F8. Per-method RPC handler tests. **Effort: 1–2 weeks.**

---

## Final verification

- [ ] V.1 `cargo check --workspace` — green, 0 errors
- [ ] V.2 `cargo clippy --workspace` — clean or only pre-existing warnings
- [ ] V.3 `cargo test --workspace --lib --no-fail-fast` — all tests pass (same count or more)
- [ ] V.4 `cargo test -p neo-tests --no-fail-fast` — all integration tests pass
- [ ] V.5 Verify workspace member count is 31 (down from 32): `neo-telemetry` deleted, `neo-tokens-tracker` merged, `neo-rpc-types` extracted
- [ ] V.6 Verify no dead code remains: `neo-native-contracts::prefixes`, `neo-native-contracts::helpers`, duplicate `BlockchainHandle`
- [ ] V.7 Verify no `Result<_, String>` patterns remain in `neo-rpc`, `neo-serialization`, `neo-wallets`, `neo-config` (per Phase C4)
- [ ] V.8 Verify `warp` is no longer in `Cargo.lock` after Phase C1
- [ ] V.9 Verify `neo-telemetry`, `prometheus`, `sysinfo`, `tracing-appender` are no longer in `Cargo.lock`
- [ ] V.10 Update `PROTOCOL_VERIFICATION_REPORT.md` to reflect the new state
