# Code Quality LOW-Severity Fixes

**Date**: 2026-07-03
**Status**: All fixes applied and verified with `cargo check --workspace` (clean, zero warnings).

## Issues Fixed

### 1a. Quote-escape doc comment (FIXED)

**File**: `neo-serialization/src/json/escape.rs:23-25`

**Problem**: Module-level documentation said "the quote character is emitted as `"` (not `\"`)" which implied the quote was unescaped. The actual behavior (confirmed by `write_char_escape` at line 278 and `CSharpEscapeFormatter`) is that the quote character is emitted as the six-character Unicode escape `\u0022`.

**Fix**: Updated the doc comment to:
> Note that the quote character is emitted as the six-character Unicode escape `\u0022` (not the short two-character form `\"`)

### 1b. Oracle MIN_GAS_FOR_RESPONSE error message (ALREADY FIXED)

**File**: `neo-native-contracts/src/oracle_contract/mod.rs:319`

The error message already correctly reads:
```
"gasForResponse {gas_for_response} must be at least 0.1 GAS."
```

No change needed. This was previously fixed (was "0.1 datoshi", now correctly says "0.1 GAS").

### 1c. Oracle signature assembly silent returns (FIXED)

**File**: `neo-oracle-service/src/service/transactions/signature.rs:26, :34`

**Problem**: Two silent `return` statements on key/sign failure with no observability.

**Fix**: Replaced `let Ok(...) = ... else { return; }` and `Err(_) => return` with `match` expressions that log the error via `warn!` before returning:
- `get_public_key_point()` failure: `warn!(target: "neo::oracle", %e, "failed to get public key point for oracle signature")`
- `key.sign(&message)` failure: `warn!(target: "neo::oracle", %e, "failed to sign oracle response message")`

### 2d. TEE wallet sealed-key load warnings (FIXED)

**File**: `neo-tee/src/wallet/tee_wallet.rs:90`

**Problem**: Sealed-key load failure was logged at `warn!` level. A sealed-key that was previously stored should always be recoverable; failure to load indicates a storage/key-derivation error that deserves escalation.

**Fix**: Changed `warn!` to `error!` and updated import from `use tracing::{info, warn}` to `use tracing::{error, info}`.

### 2e. node.rs ledger pre-checks swallow errors (ALREADY FIXED)

**File**: `neo-system/src/composition/node.rs:340-364`

**Problem**: The original issue described `.unwrap_or(false)` silently swallowing storage errors in the transaction verification pre-check path.

**Status**: Already fixed in a prior phase — the code now uses thorough `?` propagation with descriptive error messages (see "fails closed" comment at line 340-342):
```rust
ledger
    .contains_transaction(snapshot, &hash)
    .map_err(|error| CoreError::other(format!("ledger contains_transaction: {error}")))?
```

### 3f. NEP-2 flags byte 0xe0 (FIXED)

**File**: `neo-wallets/src/crypto/key_pair.rs:331`

**Problem**: Hardcoded `0xe0` flags byte had only the comment `// Flags` with no explanation.

**Fix**: Added descriptive comment explaining the flag bits per NEP-2 specification:
```rust
result.push(0xe0); // NEP-2 flags: isCompressed (0x80) | isEC (0x40) | reserved (0x20)
```

| Bit | Value | Meaning |
|-----|-------|---------|
| 7   | 0x80  | isCompressed — public key is in compressed form |
| 6   | 0x40  | isEC — the key is an elliptic curve key |
| 5   | 0x20  | Reserved — must be 1 for backwards compatibility with C# reference implementation |

### 3g. Hardcoded storage_price/exec_fee_factor init (FIXED)

**File**: `neo-execution/src/application_engine/state.rs:93-94, :171-172`

**Problem**: `exec_fee_factor: 30u32 * (FEE_FACTOR as u32)` and `storage_price: 100_000u32` are hardcoded magic numbers with no comment explaining they are temporary defaults that get overwritten.

**Fix**: Added comments on both initialization sites (in `new()` and `new_with_preloaded_native()`):
```
// Safe defaults; overwritten by refresh_policy_settings().
```
The function `refresh_policy_settings()` (in `witness_and_misc.rs:776`) queries the Policy native contract for `getExecFeeFactor`/`getExecPicoFeeFactor` and `getStoragePrice`, overwriting these values.

## Verification

```sh
$ cargo check --workspace
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.63s
```

All crates compile with zero errors and zero warnings.
