# Phase 2A+2B: Deferred MPT State Root + DataCache Reuse

**Goal**: ~27% faster block persistence by deferring MPT trie computation to a background thread (18%) and reusing DataCache allocations across transactions (9%).

**Context**: Phase 1 shipped (RocksDB tuning, rayon parallel verification, Arc\<Block\>, DashMap). These Phase 2 changes target `persist_block_internal()` in `neo-core/src/neo_system/persistence.rs`, the single function where blocks are executed and committed.

## 2A. Deferred MPT State Root Computation

**File**: `neo-core/src/state_service/commit_handlers.rs`

**Problem**: `blockchain_committing_handler` (line 61) runs MPT trie update synchronously during block persistence. It calls `state_store.update_local_state_root_snapshot()` which traverses and rebalances the trie, then computes the root hash. This blocks the persistence pipeline for 10-100ms per block.

**Current call order** in `persist_block_internal()`:
```
line 488: invoke_committing() → MPT trie update (BLOCKS 10-100ms)
line 495: apply_tracked_items() → merge changes to store transaction
line 510: tx.commit() → RocksDB write
line 533: invoke_committed() → MPT trie persist to disk (BLOCKS)
```

**New call order**: Move MPT computation to after `tx.commit()`, run in background thread.

```
line 495: apply_tracked_items()
line 510: tx.commit()
line 533: invoke_committed() → now spawns background MPT (returns immediately)
          Background thread: update_local_state_root_snapshot() + update_local_state_root()
          Next block: before running its own MPT, waits for previous to finish
```

**Change to `StateServiceCommitHandlers`**:

Add a `pending_task: Mutex<Option<JoinHandle<()>>>` field. Restructure the two handlers:

```rust
// blockchain_committing_handler — collect data, spawn background work
fn blockchain_committing_handler(&self, ..., block: &Block, snapshot: &DataCache, ...) {
    // Wait for previous block's MPT to finish (if any)
    if let Some(handle) = self.pending_task.lock().take() {
        let _ = handle.join();
    }

    // Collect tracked items NOW (while snapshot is alive)
    let height = block.index();
    let changes: Vec<_> = snapshot.tracked_items()
        .into_iter()
        .map(|(key, trackable)| (key, trackable.item, trackable.state))
        .collect();

    // Spawn background MPT computation
    let state_store = Arc::clone(&self.state_store);
    let handle = std::thread::spawn(move || {
        state_store.update_local_state_root_snapshot(height, changes.into_iter());
        state_store.update_local_state_root(height);
    });

    *self.pending_task.lock() = Some(handle);
}

// blockchain_committed_handler — no-op (work merged into background task above)
fn blockchain_committed_handler(&self, ...) {
    // MPT persist is now handled by the background thread in committing_handler
}
```

**Safety**: The background thread receives owned data (`Vec` of tracked items) and an `Arc<StateStore>`. It doesn't share mutable state with the main persistence thread. The `pending_task` mutex ensures sequential MPT computation — block N's MPT finishes before block N+1's starts.

**Shutdown**: On drop, `StateServiceCommitHandlers` should join the pending task to ensure the last MPT completes.

**Risk**: Low. The state root is not needed for block processing — it's only used for state proof queries and state root validation (which runs asynchronously anyway). The database commit happens before MPT, so all storage changes are persisted regardless of MPT timing.

## 2B. DataCache Reuse Across Transactions

**File**: `neo-core/src/persistence/data_cache.rs`, `neo-core/src/neo_system/persistence.rs`

**Problem**: Each of N transactions in a block creates a new `DataCache::new_with_config()` (line 309 of persistence.rs). For a block with 1000 transactions, this creates and destroys 1000 DataCache instances. Each DataCache allocates:
- `HashMap<StorageKey, Trackable>` (dictionary)
- `HashSet<StorageKey>` (change_set)
- `RwLock<InnerState>` wrapper
- Various Arc pointers

**Solution**: Add a `reset()` method to DataCache that clears all internal state without deallocating the backing storage. Reuse one DataCache across all transactions in a block.

**New method on DataCache**:

```rust
/// Resets the cache for reuse, clearing all entries while retaining
/// allocated capacity. Much cheaper than drop + new for repeated use.
pub fn reset(&self) {
    let mut state = self.state.write();
    state.dictionary.clear();
    state.change_set.clear();
    // Reset any other per-transaction state
}
```

**Change in `persist_block_internal()`**:

```rust
// Before the transaction loop (line ~305):
let tx_snapshot = Arc::new(DataCache::new_with_config(
    false,
    Some(Arc::clone(&tx_store_get)),
    Some(Arc::clone(&tx_store_find)),
    tx_cache_config,
));

// Inside the loop, before each transaction:
tx_snapshot.reset();  // Clear previous tx's changes, keep allocations

// After the loop, on HALT:
let tracked = tx_snapshot.tracked_items();
base_snapshot.merge_tracked_items(&tracked);
```

**Subtlety**: The `Arc<DataCache>` is shared with the `ApplicationEngine` during execution. We must ensure the engine has dropped its reference before calling `reset()`. Since `tx_engine` is created and dropped within the loop body, this is guaranteed by Rust's drop semantics — the engine is dropped at the end of the loop iteration, before the next iteration calls `reset()`.

**Alternative approach if Arc sharing prevents reset**: Use `Arc::get_mut()` which returns `Some(&mut DataCache)` only when the Arc has exactly one strong reference. If the engine has dropped its clone, this succeeds. If not, fall back to creating a new DataCache.

**Risk**: Low. `HashMap::clear()` retains capacity (Rust guarantee). The DataCache interface doesn't change — `reset()` is additive. The existing `tracked_items()` and `merge_tracked_items()` calls work unchanged.

## Testing Strategy

1. **Existing tests**: All 169 test suites must pass — no behavioral change.
2. **MPT correctness**: State roots must match the reference C# implementation. Run `scripts/continuous-stateroot-validation.py` against known-good state roots.
3. **Concurrency**: The background MPT thread must complete before shutdown. Add a `Drop` impl for `StateServiceCommitHandlers` that joins pending work.

## Implementation Order

1. **2B (DataCache reset)** — Self-contained, no concurrency concerns, easy to verify
2. **2A (Deferred MPT)** — Background thread, requires careful ordering with next block

Each is independently committable and testable.
