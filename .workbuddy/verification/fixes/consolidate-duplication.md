# Code Duplication Consolidation

## Summary

Three code duplication issues identified during engineering review were addressed:

1. **Storage prefix constants** duplicated across crates
2. **`parse_script_hash_or_address`** duplicated in RPC client and server
3. **`max_valid_until_block_increment`** computed instead of using a shared constant

`cargo check --workspace` passes cleanly with zero errors and zero warnings.

---

## 1. Storage Prefix Constants (LedgerContract)

### Identified Duplication

| File | Lines | Constants |
|---|---|---|
| `neo-native-contracts/src/ledger_contract/storage.rs` | 10-16 | `PREFIX_BLOCK_HASH = 9`, `PREFIX_BLOCK = 5`, `PREFIX_TRANSACTION = 11`, `PREFIX_CURRENT_BLOCK = 12` |
| `neo-blockchain/src/ledger/ledger_records.rs` | 52-59 | Same 4 constants |

> Note: The review also listed `contract_management/mod.rs` at line 43 as a third duplication site. Inspection showed that file defines *different* constants (`PREFIX_CONTRACT = 8`, `PREFIX_CONTRACT_HASH = 12`, `PREFIX_NEXT_AVAILABLE_ID = 15`) for the ContractManagement contract — these are not duplicates of the LedgerContract constants.

### Resolution

**Canonical source**: `neo-native-contracts/src/ledger_contract/storage.rs`
The blockchain crate (`neo-blockchain`) already depends on `neo-native-contracts`.

Changes:

1. `neo-native-contracts/src/ledger_contract/storage.rs` — Changed `const` → `pub const` for all 4 prefix constants
2. `neo-native-contracts/src/ledger_contract/mod.rs` — Changed `mod storage;` → `pub mod storage;` with doc comment
3. `neo-blockchain/src/ledger/ledger_records.rs` — Replaced local constant definitions with:
   ```rust
   use neo_native_contracts::ledger_contract::storage::{
       PREFIX_BLOCK, PREFIX_BLOCK_HASH, PREFIX_CURRENT_BLOCK, PREFIX_TRANSACTION,
   };
   ```

### Backward Compatibility

- The constants retain the same path: `ledger_contract::storage::PREFIX_*`
- No public API breakage — the blockchain crate was the only external consumer
- `LedgerContract` methods using these constants are unchanged

---

## 2. `parse_script_hash_or_address` Deduplication

### Identified Duplication

| Location | Signature | UInt160 Parse | Error Type |
|---|---|---|---|
| `neo-rpc/src/client/utility/parsing.rs:215` | `(&str, &ProtocolSettings) -> CoreResult<UInt160>` | `UInt160::parse()` | `CoreError` |
| `neo-rpc/src/server/rpc_helpers/mod.rs:102` | `(&str, u8) -> Result<UInt160, RpcException>` | `UInt160::try_parse()` | `RpcException` |

Both functions perform the same core logic: try to parse as a hex UInt160, fall back to wallet address parsing via `WalletAddress::to_script_hash`.

### Resolution

**Shared implementation**: `neo-rpc/src/client/utility/parsing.rs` (available when `server` feature is active since `server` implies `client`)

Changes:

1. `neo-rpc/src/client/utility/parsing.rs` — Added `pub(crate) parse_script_hash_or_address_inner(text: &str, address_version: u8) -> CoreResult<UInt160>` containing the shared core logic (uses `UInt160::try_parse` + `WalletHelper::to_script_hash`)
2. `neo-rpc/src/client/utility.rs` — Re-exported `parse_script_hash_or_address_inner` via `pub(crate) use`
3. `neo-rpc/src/client/mod.rs` — Re-exported at `client` module level: `pub(crate) use utility::parse_script_hash_or_address_inner;`
4. `neo-rpc/src/server/rpc_helpers/mod.rs` — Replaced inline implementation with delegation:
   ```rust
   crate::client::parse_script_hash_or_address_inner(text, address_version)
       .map_err(map_address_error)
   ```

### Backward Compatibility

- Client: `parse_script_hash_or_address(value, protocol_settings) -> CoreResult<UInt160>` — unchanged public API
- Server: `parse_script_hash_or_address(text, address_version) -> Result<UInt160, RpcException>` — unchanged public API
- Server: `parse_script_hash_or_address_with_error(text, address_version, map_error) -> Result<UInt160, RpcException>` — unchanged public API
- Server: `expect_script_hash_or_address_param(...)` — unchanged

---

## 3. `max_valid_until_block_increment` Consolidation

### Identified Duplication

| Location | Value | Form |
|---|---|---|
| `neo-config/src/settings/protocol.rs:150` | `86_400_000 / 15_000` | Inline computation |
| `neo-config/src/settings/protocol.rs:202` | `5_760` | Literal |
| `neo-config/src/settings/protocol.rs:254` | `5_760` | Literal |
| `neo-native-contracts/src/policy_contract/mod.rs:63` | `5_760` | `pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT` |
| `neo-config/src/tests/settings/protocol.rs:17` | `5_760` | Test assertion literal |
| `neo-native-contracts/src/tests/policy_contract/tests.rs:355` | `5_760` | Test seed value |

Verification: `86_400_000 / 15_000 = 5_760` (24 hours = 86,400,000 ms; 15 seconds/block = 15,000 ms/block; 5,760 blocks per day). The `1000 / 15` reference in the review is consistent — 15 seconds = 15 * 1000 ms, and 24h = 86,400 * 1000 ms, giving `(86,400 * 1000) / (15 * 1000) = 86,400 / 15 = 5,760`.

### Resolution

**Shared constant**: `neo-primitives/src/utils/constants.rs` (canonical home for protocol constants; both `neo-config` and `neo-native-contracts` depend on `neo-primitives`)

Changes:

1. `neo-primitives/src/utils/constants.rs` — Added:
   ```rust
   /// Default maximum valid-until-block increment (24 hours / 15-second blocks).
   /// Computed as `86_400_000 ms/day / 15_000 ms/block = 5_760`.
   pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;
   ```

2. `neo-config/src/settings/protocol.rs` — Replaced all 3 occurrences (`86_400_000 / 15_000` and two `5_760` literals) with `constants::DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT`

3. `neo-native-contracts/src/policy_contract/mod.rs` — Updated to reference the shared constant:
   ```rust
   pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 =
       neo_primitives::constants::DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT;
   ```

4. Test files updated to use the constant:
   - `neo-config/src/tests/settings/protocol.rs:17`
   - `neo-native-contracts/src/tests/policy_contract/tests.rs:355`

### Dependency Analysis

`neo-config` does not depend on `neo-native-contracts`, preventing direct import of the existing `DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT` constant. Both crates depend on `neo-primitives`, making it the natural home for the shared definition. `neo-native-contracts` maintains backward compatibility by still exporting the constant at its original path.

### Backward Compatibility

- `neo_primitives::constants::DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT` — new public constant
- `neo_native_contracts::policy_contract::DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT` — retains same value and path
- `ProtocolSettings` struct field value unchanged (`5_760` = `86_400_000 / 15_000`)

---

## Verification

```
$ cargo check --workspace
    Checking neo-native-contracts v0.10.0
    Checking neo-mempool v0.10.0
    Checking neo-wallets v0.10.0
    Checking neo-blockchain v0.10.0
    Checking neo-system v0.10.0
    Checking neo-oracle-service v0.10.0
    Checking neo-rpc v0.10.0
    Checking neo-node v0.10.0
    Checking neo-tests v0.10.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.98s
```

Zero errors. Zero warnings.
