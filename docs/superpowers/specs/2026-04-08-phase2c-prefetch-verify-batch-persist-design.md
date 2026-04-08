# Phase 2C: Prefetch Verification + Batch Persistence

**Goal**: +30-50% faster initial sync by pre-verifying block signatures before the actor and batching consecutive block persistence into single RocksDB commits.

**Context**: Phases 1 and 2A/2B shipped. The Blockchain actor is the remaining sequential bottleneck — it processes one message at a time. During initial sync, it spends 10-100ms on signature verification and 1-5s on persistence per block, blocking all other messages.

## 2C-1. Pre-verify Signatures in Network Layer

**Files**: `neo-core/src/network/p2p/remote_node/inventory.rs`

**Problem**: When a block arrives from the network, RemoteNode sends it to the Blockchain actor which verifies signatures synchronously (10-100ms) before persistence. During this time, the actor can't process other blocks.

**Current flow**:
```
RemoteNode::on_block()
  → blockchain.tell(InventoryBlock { block, relay })  // queued
  → Actor: on_new_block()
    → block.verify_with_cache()  // 10-100ms BLOCKING actor
    → persist_block_sequence()   // 1-5s BLOCKING actor
```

**New flow**:
```
RemoteNode::on_block()
  → rayon::spawn: block.verify_state_independent()  // parallel, off-actor
  → on success: blockchain.tell(InventoryBlock { block, pre_verified: true })
  → Actor: on_new_block()
    → skip state-independent verification (already done)
    → block.verify_state_dependent_only()  // fast, just state checks
    → persist_block_sequence()
```

**Implementation**:

Add a `pre_verified: bool` field to `BlockchainCommand::InventoryBlock`. In `RemoteNode::on_block()`, run `verify_state_independent()` on the rayon thread pool before sending to the actor. The block verification methods in `block.rs` (`verify_with_cache`) already call `verify_transactions` which splits into independent + dependent phases (from Phase 1). We add a flag to skip the independent phase when pre-verified.

**Changes**:

1. In `types.rs`: Add `pre_verified: bool` to `InventoryBlock` variant
2. In `inventory.rs`: Call `block.verify_state_independent_all(settings)` before sending to actor. Use `rayon::spawn` for the CPU work, then send verified block.
3. In `block.rs`: Add `verify_transactions_state_dependent_only()` that skips the parallel phase
4. In `block_processing.rs`: When `pre_verified == true`, call the state-dependent-only variant

**Safety**: Signature verification is pure computation — reads transaction data and public keys, performs ECDSA. No shared mutable state. The `ProtocolSettings` needed for verification is `Arc` and `Send + Sync`.

**Risk**: Low. We're moving existing pure computation earlier in the pipeline. If pre-verification fails, the block is dropped before reaching the actor (same outcome as current code, just detected earlier).

## 2C-2. Batch Consecutive Block Persistence

**File**: `neo-core/src/neo_system/persistence.rs`, `neo-core/src/ledger/blockchain/block_processing.rs`

**Problem**: `persist_block_sequence` persists blocks one at a time, each with its own `tx.commit()` call to RocksDB. The RocksDB commit (fsync + WAL) takes 50-500ms per block. For 10 consecutive blocks, that's 10 separate commits.

**Current flow**:
```
persist_block_sequence(block N):
  persist_block_internal(N) → tx.commit()  // 50-500ms
  persist_block_internal(N+1) → tx.commit()  // 50-500ms
  persist_block_internal(N+2) → tx.commit()  // 50-500ms
```

**New flow**:
```
persist_block_batch([N, N+1, N+2, ...]):
  for each block:
    execute_block_transactions(block)  // execute txs, merge to shared snapshot
    apply_tracked_items()              // merge to store transaction
  single tx.commit()                   // one 50-500ms write for all blocks
```

**Implementation**:

Modify `persist_block_internal` to accept an optional external `StoreTransaction` instead of creating its own. When batching, the caller creates one `StoreTransaction`, passes it through multiple `persist_block_internal` calls, then commits once.

1. In `persistence.rs`: Add `persist_block_internal_with_tx()` that takes `&mut StoreTransaction` and skips the commit step (returns after `apply_tracked_items`).
2. In `block_processing.rs`: Modify `persist_block_sequence` to:
   - Collect all available consecutive blocks upfront from `_block_cache_unverified`
   - Create one `StoreTransaction`
   - Call `persist_block_internal_with_tx()` for each block
   - Single `tx.commit()` at the end
   - Then `handle_persist_completed()` for each block

**Batch size limit**: Cap at 10-20 blocks per batch to bound memory usage and ensure timely event publishing.

**Safety**: Transaction execution within a batch is still sequential (state-dependent). The only change is deferring the RocksDB write to the end. If any block in the batch fails, roll back the entire batch (StoreTransaction is dropped without commit).

**Risk**: Medium. The main concern is that `invoke_committing` (MPT handlers) currently runs per-block between execution and commit. With batching, we'd accumulate all blocks' changes then run MPT for each block's tracked items. The deferred MPT from Phase 2A already handles this — the background thread processes each block's changes sequentially.

## Testing Strategy

1. **Existing tests**: All 169 test suites must pass
2. **Sync test**: Compare state root at block 10,000 before/after to verify no divergence
3. **Edge cases**: Batch with a faulting block mid-batch (verify partial rollback)

## Implementation Order

1. **2C-1 (Pre-verify)** — Lower risk, independent of persistence changes
2. **2C-2 (Batch persist)** — Higher impact, builds on existing persist flow

Each independently committable.
