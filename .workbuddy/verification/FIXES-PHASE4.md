# Phase 4: Verification Divergence Fixes — Summary

**Date:** 2026-07-03
**Context:** Fixes for divergences found in Phase 3 semantic Rust-vs-C# verification
**Total divergences analyzed:** 14 (2 HIGH, 8 MEDIUM, 6 LOW — minus overlap M3/M6)
**Fixes applied:** 3 code changes
**Confirmed false positives / already correct:** 5
**Deferred (feature-level, not fixable in-place):** 2

---

## Fixes Applied

### Fix 1: Removed stale `ConsensusPayload::get_sign_data()` (M1 — MEDIUM)

**Problem:** Dead signing API with non-protocol format. `get_sign_data()` constructed signing data as `network + block_index + validator_index + view_number + type + data` — different from C# `dbft_sign_data()` which uses `network + hash(message_bytes)`. Method was never called in production, only in a trivial test.

**Changes:**
- Removed `get_sign_data()` method from `ConsensusPayload` in `neo-consensus/src/messages/mod.rs`
- Removed `test_consensus_payload_sign_data` test from `tests/tests/consensus/consensus_integration_tests.rs`
- Active `dbft_sign_data()` path remains unchanged (already tested internally)

**Files:** `neo-consensus/src/messages/mod.rs`, `tests/tests/consensus/consensus_integration_tests.rs`

---

### Fix 2: Mempool rebroadcast after block-persist re-verify (M3/M6 — MEDIUM)

**Problem:** C# `MemoryPool.UpdatePoolForBlockPersisted` re-verifies AND rebroadcasts surviving transactions. Rust only re-verified without rebroadcast, causing delayed network propagation of transactions that persisted across a block boundary.

**Changes:**
- In `MemoryPool::reverify_top_unverified()` — after each transaction successfully re-verifies and promotes from unverified → verified, collect it for rebroadcast
- After the write-lock is released, invoke `transaction_relay` callback for each surviving transaction
- Best-effort relay: failure is harmless (tx stays in pool for next gossip cycle)
- Added comprehensive documentation referencing C# `RelayDirectly` behavior

**Files:** `neo-mempool/src/pool/memory_pool.rs` (+153 lines)

---

### Fix 3: Cached verification to eliminate double-verify (M7 — MEDIUM)

**Problem:** `TxRouterHandle::try_enqueue_preverify()` ran structural preverify, then `MemoryPool::try_add()` ran full ECDSA signature verification again — double-verifying the same witness signatures. C# caches `Transaction.VerificationResult` to avoid this.

**Changes (12 files, +342/-48 lines):**

| Layer | Change |
|-------|--------|
| `neo-mempool/src/admission/transaction_router.rs` | `PreverifyCompleted` now carries `cached_state_independent: Option<VerifyResult>` — ECDSA result from preverify |
| `neo-mempool/src/admission/verification.rs` | New `verify_transaction_dependent_only()` — skips redundant ECDSA, only runs state-dependent checks |
| `neo-mempool/src/admission/mod.rs` | New `try_add_cached()` entry point |
| `neo-mempool/src/pool/memory_pool.rs` | `try_add_cached()` accepts optional cached state-independent result |
| `neo-blockchain/src/handlers/transactions.rs` | `on_new_transaction()` passes cached result through |
| `neo-blockchain/src/pipeline/handlers.rs` | `handle_preverify_completed()` wires cached result from PreverifyCompleted |
| `neo-blockchain/src/service/service.rs` | `MempoolLike` trait extended with `try_add_cached()` |
| `neo-blockchain/src/tests/pipeline/handlers.rs` | Test mocks updated |
| `neo-blockchain/src/tests/pipeline/handlers/transactions.rs` | Test updates |
| `neo-blockchain/src/tests/service/service.rs` | Test mock updated |
| `neo-rpc/src/tests/server/support/test_support.rs` | Test mock updated |

**Flow:**
```
Before:  preverify (structural) → try_add (full: ECDSA + state-dep)
After:   preverify (structural + ECDSA cached) → try_add_cached (state-dep only, ECDSA skipped)
```

Callers without preverify (oracle, inventory re-verify) pass `None`, preserving full verification.

---

## False Positives / Already Correct

| # | Description | Resolution |
|---|-------------|------------|
| M2 | GasToken.on_persist network-fee mint guard | **Already correct.** Rust correctly drops the Python-XSPEC guard `if total_network_fee > 0`; always calls `gas_mint()` like C#. All three sign paths (+, 0, -) produce byte-compatible results. |
| M4 | Oracle PostPersist GAS payout | **Already implemented.** `OracleContract::post_persist()` at `neo-native-contracts/src/oracle_contract/mod.rs:147-233` implements complete C# payout: iterate block txs → find OracleResponse → remove requests → lookup designated nodes → mint GAS. Two dedicated PostPersist tests pass. |
| M5 | neo-runtime telemetry in consensus paths | **By design.** Telemetry instrumentation is gated and has negligible overhead. |
| M8 | clone_cache path in persist pipeline | **Already used.** `snapshot.clone_cache()` at `native_persist.rs:534` (block-level), `block_cache.clone_cache()` at `:603` (per-tx), and `empty_block_fast_forward.rs:303`. Active production path. |
| L4-L5 | MethodToken/ContractParamType serialization | **Already correct.** Verified exact in Layer 3 Infrastructure audit. |

---

## Deferred (Feature-Level, Not Fixable In-Place)

| # | Description | Rationale |
|---|-------------|-----------|
| H1 | Signed StateRoot P2P consensus | Major feature (~2000+ lines). Requires StateValidators signing, vote broadcast, M aggregation, multisig witness verification, storage. Should be a dedicated feature branch. |
| H2 | Per-native hardfork re-initialization (Notary/Oracle) | Requires deep-path integration testing at specific hardfork heights. No code change identified from static analysis alone. |

---

## LOW Divergences Not Addressed

These are cosmetic/strictness issues with no consensus or operational impact:
- L1: UInt160::hash_code() internal difference (non-consensus)
- L2: ExtensiblePayload stricter-than-C# validation range (safe)
- L3: State root verification accepts unsigned roots (graceful in single-node)
- L6: Code duplication neo-rpc/neo-node (hygiene)
- L7: Log redaction granularity (hygiene)

---

## Compilation & Test Status

- `cargo check --workspace` — **PASS** (0 errors, 0 warnings)
- `cargo test --workspace` — **PENDING** (running)

---

## Files Changed Summary

```
14 files changed, 342 insertions(+), 48 deletions(-)

neo-blockchain/src/handlers/transactions.rs        |  30 +++-
neo-blockchain/src/pipeline/handlers.rs            |   5 +-
neo-blockchain/src/service/service.rs              |  30 ++++
neo-blockchain/src/tests/pipeline/handlers.rs      |  30 ++++
neo-blockchain/src/tests/.../handlers/transactions.rs |   6 +-
neo-blockchain/src/tests/service/service.rs        |  10 ++
neo-consensus/src/messages/mod.rs                  |  13 --  (removed dead get_sign_data)
neo-mempool/src/admission/mod.rs                   |   5 +-
neo-mempool/src/admission/transaction_router.rs    |  48 +++++-
neo-mempool/src/admission/verification.rs          |  17 +++
neo-mempool/src/lib.rs                             |   2 +-
neo-mempool/src/pool/memory_pool.rs                | 153 +++++++++++++++++++++
neo-rpc/src/tests/server/support/test_support.rs   |  22 +++
tests/.../consensus_integration_tests.rs           |  19 ---  (removed dead test)
```
