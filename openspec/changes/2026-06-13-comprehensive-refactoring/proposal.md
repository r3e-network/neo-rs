# Comprehensive Refactoring & Protocol Completion

## Why

The neo-rs codebase is **protocol-complete** for Neo N3 v3.10.1 (all 11 native contracts, 8 hardforks, 109 VM opcodes, 18+ P2P message commands, all 5 transaction attributes, 6 dBFT message types, NEP-2/6/32/39 wallets, StateService plugin). However, deep review of all 32 workspace members revealed **3 categories of structural debt** that must be resolved to make this a professional blockchain node:

### 1. Dead code & no-consumers crates (the worst form of inconsistency)
- **`neo-telemetry`** (1,980 LoC, 10 files): **ZERO production consumers**. `neo-blockchain`, `neo-node`, and `tests` declare it as a Cargo dependency but no `.rs` file in the workspace imports `neo_telemetry::*`. The `init_node_logging`/`init_for_node` functions are never called.
- **`neo-tee`** (5,036 LoC, 16 files): **ZERO production consumers**. Only `neo-node`'s feature-gated `tee`/`tee-sgx` flags declare it; no code path instantiates `TeeWallet`/`TeeMempool`/`TeeEnclave`.
- **`neo-hsm`** (1,729 LoC, 15 files): **ZERO production consumers**. Same pattern as `neo-tee`. The `HsmSigner` trait is defined but never invoked from `neo-wallets`/`neo-rpc`/transaction-signing paths.
- **`neo-native-contracts::prefixes`** (entire file): 31 constants declared `pub` but **never imported anywhere**. Every contract redefines its own private `PREFIX_*` constants.
- **`neo-native-contracts::helpers`** (entire module): `NativeHelpers` defined but **never referenced anywhere**.
- **`neo-runtime::blockchain::BlockchainHandle`/`BlockchainCommand`**: duplicated types; the live node uses `neo-blockchain`'s canonical version (`node-readiness-audit-2026-06-11.md:18`).

### 2. Style inconsistencies across native contracts
The 11 native contracts share a near-perfect canonical skeleton (`#[derive(Debug, Default, Clone, Copy)]` unit struct, `ID`/`NAME`/`new()`/`hash()`/`script_hash()`, `NativeContract` impl, identical `invoke` signature, snake_case hooks). However:

- **`policy_contract.rs:88-114` has STUB METHODS that ignore their `snapshot` argument** and return hardcoded constants — used by `neo-rpc`'s fee estimation on the hot path of `sendrawtransaction`.
- **`policy_contract.rs:47-69` declares `DEFAULT_EXEC_FEE_FACTOR`/`DEFAULT_FEE_PER_BYTE`/`DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT` TWICE** (top-level + inside `impl PolicyContract`).
- **Test helpers duplicated 4-6× across contracts**: `deploy_native` (4×), `hex` (5×), `sample_committee` (4×), `CM_PREFIX_CONTRACT` (6×), `NEO_PREFIX_COMMITTEE` (4×), `committee_address` (3×), `deployable_manifest` (2×).

### 3. Custom implementations that should use mature third-party libraries
- **CRITICAL: `warp = "0.3"` (abandoned since 2020) is still the primary RPC server framework**. `jsonrpsee = "0.24.10"` (mature, used by Polkadot/Ankr) is already integrated as a parallel adapter. The 611-LoC `neo-rpc/src/server/rpc_server.rs` uses `warp::Filter` + manual hyper plumbing that `jsonrpsee` provides for free. ~1,200 LoC of glue code can be deleted.
- **Custom `Sha256Hasher` wrapper** (60 LoC) over `sha2::Sha256` adds no value beyond delegation.
- **Custom `BigDecimal` (448 LoC)** could use the `bigdecimal` crate (~1M downloads) with a thin Neo-specific newtype wrapper.
- **`Result<_, String>` patterns everywhere** (100+ in `neo-rpc/src/client/utility/`, `neo-serialization/`, `neo-wallets/`) leak stringified errors that can never be pattern-matched. The workspace already depends on `thiserror = "2.0"`.

### 4. Duplicate logic / missed Rust idioms
- **`strip_0x` helper** duplicated 6+ times across crates.
- **Manual `key.push(PREFIX_*)`** in 28+ sites, despite `StorageKey::create_*` helpers existing in `neo-storage/src/types/storage_key.rs`.
- **49 RPC handler functions** share the identical signature `fn name(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException>`; only the parameter extraction differs.
- **30+ hand-written `from_json` parsers** in `neo-rpc/src/client/models/` that should use `#[derive(serde::Deserialize)]`.
- **`assert_committee` check** (8-line boilerplate) duplicated 7× across native contracts.
- **`parse_account`/`hash160_arg`/`setter_int_arg`** arg-parsing helpers duplicated 14× across native contracts.

### Remaining protocol gaps (per `PROTOCOL_VERIFICATION_REPORT.md` §12)
| Gap | Effort |
|---|---|
| `RpcTestCases.json` harness (46 cases unconsumed) | 2 weeks |
| 4-validator dBFT end-to-end round → real `Block` | 1 week |
| C# native-contract state-root replay vectors | 1–2 weeks |
| Transaction-bearing mainnet block C# vector (only block 1,000 is covered) | 2–3 days |
| Live `Version`/`Verack` handshake against a real C# peer | 2–3 days |
| C# MPT root vectors | 1 week |
| BLS12-381 draft-04 / RFC 6979 / BIP-32 test vectors | 1 week |
| Per-method RPC handler tests | 1–2 weeks |

**Total estimated effort: 6–10 weeks to 100% C# wire/protocol parity; 3–6 months to production-ready.**

## What Changes

This change is structured as **5 phases** that must be applied roughly in order. Each phase is independently shippable.

### Phase A — Dead code elimination (1 day, highest ROI)
- A1. **Delete `neo-telemetry` crate entirely.** Remove from workspace members and deps in `neo-blockchain`/`neo-node`/`tests` (zero consumers).
- A2. **Mark `neo-tee` and `neo-hsm` as "feature-gated, awaiting wallet integration"** in their README + `Cargo.toml` descriptions. Do not delete (they're future-feature). Update the `neo-node` `tee`/`hsm` feature docs to clarify they're not production-ready.
- A3. **Delete `neo-native-contracts::prefixes` module** (entire file, dead code).
- A4. **Delete `neo-native-contracts::helpers` module** (entire module, dead code).
- A5. **Delete duplicate `BlockchainHandle`/`BlockchainCommand`** in `neo-runtime/src/blockchain.rs` (86 LoC).
- A6. **Delete unused `NEP17_PREFIX_TOTAL_SUPPLY`/`NEP17_PREFIX_ACCOUNT` second declarations** in `prefixes.rs` (already deleted by A3).

### Phase B — Native contract style consistency + dedup (1 week)
- B1. **Hoist `assert_committee`** from `policy_contract.rs:380-389` into a new `neo-native-contracts/src/committee.rs` module. Replace 7 inline copies in `role_management.rs`, `treasury.rs`, `notary.rs`, `oracle_contract.rs`, `contract_management.rs`, `neo_token.rs` (2×).
- B2. **Hoist `parse_account`/`hash160_arg`/`setter_int_arg`/`attribute_type_arg`** into `neo-native-contracts/src/args.rs`. Replace ~14 inline copies in `gas_token.rs`, `neo_token.rs`, `policy_contract.rs`, `oracle_contract.rs`, `ledger_contract.rs`, `contract_management.rs`.
- B3. **Implement or delete the `PolicyContract::get_*_snapshot` stubs** (`policy_contract.rs:88-114`). They silently ignore the snapshot argument and return hardcoded defaults, which leaks stale values into `neo-rpc`'s fee estimation.
- B4. **Remove duplicate `DEFAULT_*` constants** in `policy_contract.rs:65-69` (already exist top-level at lines 47-52).
- B5. **Hoist test helpers into `neo-native-contracts/tests/common/mod.rs`**: `deploy_native` (4×), `hex`/`hex_to_bytes` (5×), `sample_committee` (4×), `CM_PREFIX_CONTRACT` (6×), `NEO_PREFIX_COMMITTEE` (4×), `POLICY_PREFIX_ATTRIBUTE_FEE` (3×), `deployable_manifest` (2×), `committee_address` (3×), `on_persist_engine`/`post_persist_engine` (3×).
- B6. **Lift storage-key builders** to a common `prefixed_storage_key(contract_id, prefix, suffix)` helper to replace ~25 inlined definitions.

### Phase C — Third-party library consolidation (2 weeks)
- C1. **Migrate RPC server from `warp` to `jsonrpsee`** (CRITICAL). Delete the `warp`-based path entirely:
  - Remove `warp = "0.3"` from `neo-rpc/Cargo.toml` (line 53, 120).
  - Delete `neo-rpc/src/server/rpc_server.rs:255-460` (~205 LoC of TCP/HTTP glue).
  - Delete `neo-rpc/src/server/routes/{mod.rs, cors.rs, handlers.rs}` (~830 LoC).
  - Update `neo-rpc/src/server/ws/handler.rs:10` to use `jsonrpsee::server::Server::ws()` API.
  - Promote `neo-rpc/src/server/jsonrpsee_adapter.rs` (already exists, 116 LoC) to the primary path.
  - Replace manual `warp::Filter` plumbing with `jsonrpsee::server::Server::builder().build(...).await?` + HTTP/WebSocket transports already provided by jsonrpsee.
- C2. **Drop custom `Sha256Hasher` wrapper** (`neo-crypto/src/hash.rs:38-65`): re-export `sha2::Sha256` as `Sha256Hasher` or just use `sha2::Sha256` directly throughout.
- C3. **Migrate `BigDecimal` → `bigdecimal` crate** wrapper. The Neo semantics (8-decimal fixed-point) differ; introduce a thin `BigDecimal` newtype that delegates to `bigdecimal::BigDecimal` for arithmetic.
- C4. **Replace `Result<_, String>` with proper `thiserror` enums** in:
  - `neo-rpc/src/client/utility/` (100+ sites)
  - `neo-rpc/src/client/models/` (37 files)
  - `neo-rpc/src/client/utility/parsing.rs`
  - `neo-serialization/src/{binary,json}_serializer.rs` (23 sites)
  - `neo-wallets/src/{wallet_helper.rs, bip39.rs}` (7+)
  - `neo-config/src/protocol.rs` (7 sites)
  - Use `pub type Result<T> = core::result::Result<T, ModuleError>;` pattern per module.

### Phase D — Macro-ize RPC handler boilerplate (1 week)
- D1. **Introduce `rpc_method!` macro** (in `neo-rpc/src/server/rpc_handler_macros.rs`) that declaratively maps `(name, [param_ty; N]) -> Result<T, RpcException>` to a JSON-RPC handler with auto-generated parameter extraction, error wrapping, and JSON serialization. Inspired by `jsonrpsee::proc_macros::rpc`. Replace 49 hand-written handler functions.
- D2. **Migrate `neo-rpc/src/client/models/` (37 files, 5,921 LoC) to `#[derive(serde::Deserialize)]`**. Replace hand-written `from_json`/`to_json` impls with `serde_json::from_value::<Model>(value)?` at dispatch sites.

### Phase E — Storage key builder consolidation + small crate merges (3 days)
- E1. **Add `key_builder!` macro / `StorageKeyBuilder` struct** to `neo-storage/src/key_builder.rs` (currently 11 LoC, unused). Use throughout to replace 28+ manual `key.push(PREFIX_*)` calls.
- E2. **Add `strip_hex_prefix` helper to `neo-primitives`** and use throughout (currently duplicated 6+ times across `neo-p2p/src/witness_rule/helpers.rs`, `neo-primitives/src/uint_hex.rs`, `neo-rpc/src/client/utility.rs`, `neo-rpc/src/client/utility/witness_rule.rs`, `neo-oracle-service/src/neofs/json/helpers.rs`).
- E3. **Merge `neo-tokens-tracker` → `neo-rpc::plugins::tokens_tracker`** (feature-gated). Only `neo-rpc` consumes it; the 15 internal deps in `neo-tokens-tracker/Cargo.toml` are mostly redundant. Updates: 6 import sites in `neo-rpc`, 1 test file.
- E4. **Extract `neo-rpc-types` leaf crate** from `neo-rpc/src/client/models/` (5,921 LoC of pure DTOs) + `parameter_converter/` + `error_code.rs` + `RpcError`. Required pre-requisite for the planned `neo-rpc-server`/`neo-rpc-client` split.
- E5. **Feature-gate heavy external deps** in `neo-native-contracts/Cargo.toml`: `reqwest`/`url` (Oracle only), `blst`/`bip39` (CryptoLib only). Saves ~30–60s on `cargo build -p neo-rpc` for consumers that don't need Oracle/CryptoLib.

### Phase F — Protocol completeness (separate, large effort)
This phase is documented but NOT included in this change's tasks (it warrants its own proposal):
- F1. JSON-driven `RpcTestCases.json` harness (46 cases).
- F2. 4-validator dBFT end-to-end round producing a real `Block`.
- F3. C# native-contract state-root replay vectors.
- F4. Transaction-bearing mainnet block C# vector.
- F5. Live `Version`/`Verack` handshake against a real C# peer.
- F6. C# MPT root vectors.
- F7. BLS12-381 draft-04 / RFC 6979 / BIP-32 test vectors.

## Impact

**Codebase:**
- ~80 files modified across 12 crates.
- ~2,800 LoC reduction (mostly from `warp`→`jsonrpsee`, dead code, and duplicate helpers).
- ~3,000 LoC of duplicated boilerplate replaced with macro-based helpers.
- Workspace member count drops from 32 to 31 (after deleting `neo-telemetry`).

**APIs:** No protocol behavior changes. Public API surface changes:
- `neo_telemetry` crate removed (zero consumers).
- `neo_application_logs` (already merged) and `neo-script_builder` (already merged) handled by prior change.
- `warp` is no longer a dependency; `jsonrpsee` becomes the only RPC transport.
- 49 RPC handler signatures may simplify once `rpc_method!` macro is applied.

**Dependencies:**
- **Removed:** `warp = "0.3"`, `prometheus`, `sysinfo`, `tracing-appender` (after `neo-telemetry` deletion).
- **Added:** None mandatory. `bigdecimal` is an opt-in addition for Phase C3.

**Testing:** All existing tests must continue to pass. New tests:
- One integration test for each deduplicated helper (`assert_committee`, `parse_account`, `strip_hex_prefix`).
- One integration test for the `key_builder!` macro.
- Smoke tests for the new `jsonrpsee`-backed RPC server.

**Documentation:**
- New `CONVENTIONS.md` section on native-contract style (already exists; extend).
- README sections for `neo-tee` and `neo-hsm` clarifying their "feature-gated, awaiting wallet integration" status.

## Capabilities

### New Capabilities
- `style-guide`: native-contract style guide (extends existing) — unit-struct pattern, `assert_committee`/`parse_account`/`hash160_arg` shared helpers, test common module.
- `rpc-server-config`: jsonrpsee-backed RPC server replaces the warp-based one.

### Modified Capabilities
- `neo-rpc`: drops warp dependency, exposes `rpc_method!` macro, `rpc_types` sub-crate.
- `neo-native-contracts`: exposes `committee.rs`/`args.rs` shared helpers, drops dead `prefixes.rs` and `helpers` modules.
- `neo-primitives`: exposes `strip_hex_prefix` helper.
- `neo-storage`: exposes `key_builder!` macro/struct.
- `neo-crypto`: exposes `Sha256` re-export (drops wrapper).
- `neo-rpc-types` (new): pure DTOs + parameter-converter + `RpcError`.

### Removed Capabilities
- `neo-telemetry`: entirely removed.
- `neo-native-contracts::prefixes`: removed.
- `neo-native-contracts::helpers`: removed.

## Non-goals

- **Phase F (protocol completeness)** is **out of scope** for this change; it warrants its own dedicated proposal and effort estimate (6–10 weeks to 100% C# wire/protocol parity).
- **Splitting `neo-rpc` into `neo-rpc-server`/`neo-rpc-client`** is a follow-up after `neo-rpc-types` is extracted.
- **Splitting `neo-native-contracts` per-contract** (e.g., `neo-native-oracle`, `neo-native-notary`): kept as one crate for now; feature-gated external deps in Phase E5 are the lighter-touch alternative.
- **Migrating `neo-tee`/`neo-hsm` from feature-gated to active use** — they remain awaiting wallet/transaction-signing wiring. Their shared primitives (`normalize_public_key`, `script_hash_from_public_key`, `signature_redeem_script`) will be hoisted to `neo-crypto` per the existing dissolution plan.
- **Removing `neo-vm-rs` sibling dependency** — kept as-is.
- **Adding missing documentation** — separate effort.

## References

- `/Users/jinghuiliao/git/r3e/neo-rs/PROTOCOL_VERIFICATION_REPORT.md` — baseline protocol completeness (2026-06-12).
- `/Users/jinghuiliao/git/r3e/neo-rs/ARCHITECTURE.md` — workspace architecture.
- `/Users/jinghuiliao/git/r3e/neo-rs/CONVENTIONS.md` — existing style guide.
- `/Users/jinghuiliao/git/r3e/neo-rs/claudedocs/crate-boundary-audit-2026-06-08.md` — prior audit findings.
- `/Users/jinghuiliao/git/r3e/neo-rs/claudedocs/node-readiness-audit-2026-06-11.md` — `BlockchainHandle` dup finding.
- `/Users/jinghuiliao/git/r3e/neo-rs/claudedocs/neo-core-dissolution-validated-dag.md` — dissolution plan (tee/hsm shared primitive).
- `/Users/jinghuiliao/git/r3e/neo-rs/openspec/changes/2026-06-12-style-consistency-and-crate-consolidation/` — predecessor change (mostly done).
- `/Users/jinghuiliao/git/r3e/neo-rs/openspec/changes/2026-06-12-deep-refactoring/` — predecessor refactoring change.
