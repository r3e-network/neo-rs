# M7: Double-Verify Path in Preverify Pipeline — Fix Report

## Summary

Added a cached verification-result mechanism to avoid redundant
state-independent (ECDSA signature) re-verification when a
transaction flows from the preverify stage into mempool admission.
C# achieves the same via `Transaction.VerificationResult` caching in
`Blockchain.AskForTransaction()`; Rust now carries the cached outcome
through `PreverifyCompleted::cached_state_independent`.

## Problem Analysis

### C# Reference Implementation

```csharp
// C# Blockchain.AskForTransaction()
VerificationResult result = tx.Verify(snapshot, settings);
// ... verify, cache result on tx ...
MemoryPool.TryAdd(tx, snapshot);  // reads tx.VerificationResult
```

C# `Transaction.Verify()` caches its `VerificationResult` on the
transaction object. `MemoryPool.TryAdd()` checks the cached result
and skips redundant re-verification.

### neo-rs Before Fix

- `TransactionRouter::preverify()` only ran `Verifiable::verify()`
  (lightweight: version, signers, witnesses) — NOT the full
  `verify_state_independent()` with ECDSA signature validation.
- `MemoryPool::try_add()` always called `verify_transaction()`, which
  performs `verify_state_independent()` + `verify_state_dependent()`
  every time.
- No caching mechanism existed — every verification was a fresh run.

### Double-Verify Scenarios (Prevented by This Fix)

1. **Network transaction flow**: `TransactionRouter::preverify()` →
   `PreverifyCompleted` → `handle_preverify_completed()` →
   `on_new_transaction()` → `try_add()`. Before fix: the preverify
   only did structural checks; the full sig verification happened in
   `try_add()`. After fix: preverify does `verify_state_independent()`
   AND caches the result, so `try_add_cached()` only runs
   `verify_state_dependent()`.

2. **Future parallel-pipeline scenario**: If a parallel task
   pre-verifies transactions (like C#'s TransactionRouter) and then
   enqueues them to the mempool, the cached result prevents
   re-verifying the same signatures.

## Changes Made

### 1. `PreverifyCompleted` — Added `cached_state_independent`
**File:** `neo-mempool/src/admission/transaction_router.rs`

Added `pub cached_state_independent: Option<VerifyResult>` field.
`TransactionRouter::preverify()` now runs both `Verifiable::verify()`
(structural) AND `verify_state_independent()` (ECDSA fast-paths),
caching the latter's result in the new field.

### 2. `verify_transaction_dependent_only()` — New Function
**File:** `neo-mempool/src/admission/verification.rs`

Added a verification entry point that skips `verify_state_independent()`
and only runs `verify_state_dependent()`. Used by the cached-admission
path to avoid redundant ECDSA re-verification.

### 3. `MemoryPool::try_add_cached()` — Cached Admission
**File:** `neo-mempool/src/pool/memory_pool.rs`

New method accepting `Option<VerifyResult>`. When `Some(Succeed)` is
provided, it skips `verify_state_independent()` and only performs
`verify_state_dependent()`. When `None` is provided, it falls back to
the original full `verify_transaction()`.

```rust
pub fn try_add_cached(
    &self,
    transaction: Transaction,
    snapshot: &DataCache,
    cached_state_independent: Option<VerifyResult>,
) -> VerifyResult
```

### 4. `MempoolLike` Trait — Added `try_add_cached`
**File:** `neo-blockchain/src/service/service.rs`

Added `try_add_cached()` method to the trait with a default
implementation. All 4 mock implementations updated:
- `TestMempool`, `FixedResultMempool`, `RecordingMempool`
- `NodeMempoolAdapter` (RPC tests)

### 5. `on_new_transaction()` — Accepts Cached Result
**File:** `neo-blockchain/src/handlers/transactions.rs`

Signature changed to accept `cached_state_independent: Option<VerifyResult>`.
Uses `try_add_cached()` instead of `try_add()`. Callers updated:
- `handle_preverify_completed()` — passes `task.cached_state_independent`
- `handle_reverify()` — passes `None` (no cache available)
- `handle_fill_memory_pool()` — passes `None`

### 6. Public API Exports Updated
- `neo-mempool/src/admission/mod.rs` — exports `verify_transaction_dependent_only`
- `neo-mempool/src/lib.rs` — re-exports `verify_transaction_dependent_only`

## Code Path: Before vs After

```
BEFORE (every admission = fresh full verification):

  TLRN/P2P    TransactionRouter::preverify()     # Verifiable::verify() only
       ↓
  Blockchain  handle_preverify_completed()
       ↓
  Mempool     on_new_transaction() → try_add()
       ↓
  Verify      verify_transaction()               # FULL: state-indep + state-dep
              ├── verify_state_independent()     # ECDSA sigs, size, script
              └── verify_state_dependent()       # expiry, balance, witness engine


AFTER (cached state-independent, avoid re-verification):

  TLRN/P2P    TransactionRouter::preverify()     # Verifiable::verify()
              └── verify_state_independent()     # + ECDSA (cached!)
                   ↓
              PreverifyCompleted {
                  cached_state_independent: Some(Succeed)
              }
       ↓
  Blockchain  handle_preverify_completed()
       ↓
  Mempool     on_new_transaction(tx, Some(Succeed))
              → try_add_cached(tx, snapshot, Some(Succeed))
       ↓
  Verify      verify_transaction_dependent_only()   # SKIPs state-independent!
              └── verify_state_dependent()          # expiry, balance, witness engine only
```

For paths without preverify (oracle, inventory reverify), `None` is
passed, preserving the original full-verification behavior.

## Verification

- `cargo check --workspace`: passes
- `cargo test -p neo-mempool --lib`: 38/38 passed
- `cargo test -p neo-blockchain --lib`: 137/137 passed

## Files Changed

| File | Change |
|------|--------|
| `neo-mempool/src/admission/transaction_router.rs` | Added `cached_state_independent` field; `preverify()` now runs `verify_state_independent()` |
| `neo-mempool/src/admission/verification.rs` | Added `verify_transaction_dependent_only()` |
| `neo-mempool/src/admission/mod.rs` | Export `verify_transaction_dependent_only` |
| `neo-mempool/src/lib.rs` | Re-export `verify_transaction_dependent_only` |
| `neo-mempool/src/pool/memory_pool.rs` | Added `try_add_cached()` |
| `neo-blockchain/src/service/service.rs` | Added `try_add_cached` to `MempoolLike` trait + impl |
| `neo-blockchain/src/handlers/transactions.rs` | `on_new_transaction()` accepts cached result |
| `neo-blockchain/src/pipeline/handlers.rs` | `handle_preverify_completed()` and `handle_reverify()` pass cached result |
| `neo-blockchain/src/tests/pipeline/handlers.rs` | Updated mock impls |
| `neo-blockchain/src/tests/service/service.rs` | Updated mock impl |
| `neo-blockchain/src/tests/pipeline/handlers/transactions.rs` | Updated test callers |
| `neo-rpc/src/tests/server/support/test_support.rs` | Updated `NodeMempoolAdapter` |
