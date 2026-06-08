# Phase 2A+2B: Deferred MPT + DataCache Reuse — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ~27% faster block persistence by reusing DataCache allocations (9%) and deferring MPT trie computation to a background thread (18%).

**Architecture:** Two independent changes to `persist_block_internal()`: (1) Add `DataCache::reset()` and reuse one instance across all transactions in a block, (2) Move MPT state root computation to a background thread with join-before-next-block semantics.

**Tech Stack:** Rust, std::thread, parking_lot::Mutex

---

### Task 1: DataCache Reset and Reuse

**Files:**
- Modify: `neo-core/src/persistence/data_cache.rs` (add `reset()` method)
- Modify: `neo-core/src/neo_system/persistence.rs:297-432` (reuse DataCache in tx loop)

- [ ] **Step 1: Add `reset()` method to DataCache**

In `neo-core/src/persistence/data_cache.rs`, add this method to the `impl DataCache` block (after the existing `clear()` or `commit()` method):

```rust
    /// Resets the cache for reuse, clearing all tracked entries while retaining
    /// allocated capacity. Much cheaper than drop + new for repeated use within
    /// a block's transaction loop.
    pub fn reset(&self) {
        let mut state = self.state.write();
        state.dictionary.clear();
        state.change_set.clear();
        // Reset pattern tracker for fresh access pattern detection
        drop(state);
        *self.pattern_tracker.write() = AccessPatternTracker::new();
    }
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p neo-core 2>&1 | grep "^error" | head -5`
Expected: No errors

- [ ] **Step 3: Move DataCache creation before the transaction loop**

In `neo-core/src/neo_system/persistence.rs`, move the `tx_snapshot` creation from inside the `for tx in &ledger_block.transactions` loop (line 309) to just before it (around line 296).

Find the line:
```rust
        for tx in &ledger_block.transactions {
```

Add this BEFORE it:
```rust
        // Pre-allocate one DataCache for reuse across all transactions.
        // reset() clears state between transactions while retaining HashMap capacity.
        let tx_snapshot = Arc::new(DataCache::new_with_config(
            false,
            Some(Arc::clone(&tx_store_get)),
            Some(Arc::clone(&tx_store_find)),
            tx_cache_config,
        ));
```

- [ ] **Step 4: Replace per-tx DataCache creation with reset()**

Inside the loop, find the existing DataCache creation block (was line 309-314):
```rust
            let tx_snapshot = Arc::new(DataCache::new_with_config(
                false,
                Some(Arc::clone(&tx_store_get)),
                Some(Arc::clone(&tx_store_find)),
                tx_cache_config,
            ));
```

Replace it with:
```rust
            tx_snapshot.reset();
```

- [ ] **Step 5: Verify compilation and tests**

Run: `cargo check --workspace 2>&1 | grep "^error" | head -5`
Expected: No errors

Run: `cargo test --workspace 2>&1 | grep -c "^test result: ok"`
Expected: 169

- [ ] **Step 6: Commit**

```bash
git add neo-core/src/persistence/data_cache.rs neo-core/src/neo_system/persistence.rs
git commit -m "perf: reuse DataCache across transactions within a block

Add DataCache::reset() that clears internal state while retaining
HashMap/HashSet allocated capacity. Move DataCache creation before
the per-transaction loop and call reset() between iterations.

For a block with 1000 transactions, this eliminates ~1000 HashMap
allocation/deallocation cycles.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Deferred MPT State Root via Background Thread

**Files:**
- Modify: `neo-core/src/state_service/commit_handlers.rs` (restructure handlers)

- [ ] **Step 1: Add pending_task field and Drop impl**

In `neo-core/src/state_service/commit_handlers.rs`, update the struct:

```rust
use std::thread::JoinHandle;

pub struct StateServiceCommitHandlers {
    state_store: Arc<StateStore>,
    exception_policy: UnhandledExceptionPolicy,
    disabled: AtomicBool,
    /// Handle to the background thread computing the previous block's MPT state root.
    /// Joined before starting the next block's MPT to ensure sequential ordering.
    pending_task: parking_lot::Mutex<Option<JoinHandle<()>>>,
}
```

Update the constructor:
```rust
    pub fn new(state_store: Arc<StateStore>) -> Self {
        let exception_policy = state_store.exception_policy();
        Self {
            state_store,
            exception_policy,
            disabled: AtomicBool::new(false),
            pending_task: parking_lot::Mutex::new(None),
        }
    }
```

Add a Drop impl to join on shutdown:
```rust
impl Drop for StateServiceCommitHandlers {
    fn drop(&mut self) {
        if let Some(handle) = self.pending_task.lock().take() {
            let _ = handle.join();
        }
    }
}
```

- [ ] **Step 2: Restructure blockchain_committing_handler to spawn background work**

Replace the `ICommittingHandler` implementation:

```rust
impl ICommittingHandler for StateServiceCommitHandlers {
    fn run_during_fast_sync(&self) -> bool {
        true
    }

    fn blockchain_committing_handler(
        &self,
        _system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        _application_executed_list: &[ApplicationExecuted],
    ) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }

        // Wait for previous block's MPT to finish before starting this one.
        if let Some(handle) = self.pending_task.lock().take() {
            if let Err(payload) = handle.join() {
                self.handle_panic(
                    payload.downcast::<String>().unwrap_or_else(|_| Box::new("unknown".to_string())),
                    "committing (join)",
                );
                return;
            }
        }

        // Collect tracked items NOW while snapshot is still alive.
        let height = block.index();
        let changes: Vec<_> = snapshot
            .tracked_items()
            .into_iter()
            .map(|(key, trackable)| (key, trackable.item, trackable.state))
            .collect();

        // Spawn background MPT computation.
        let state_store = Arc::clone(&self.state_store);
        let handle = std::thread::spawn(move || {
            state_store.update_local_state_root_snapshot(height, changes.into_iter());
            state_store.update_local_state_root(height);
        });

        *self.pending_task.lock() = Some(handle);
    }
}
```

- [ ] **Step 3: Make blockchain_committed_handler a no-op**

Replace the `ICommittedHandler` implementation:

```rust
impl ICommittedHandler for StateServiceCommitHandlers {
    fn blockchain_committed_handler(&self, _system: &dyn Any, _block: &Block) {
        // MPT persist is now handled by the background thread spawned in
        // blockchain_committing_handler. No work needed here.
    }
}
```

- [ ] **Step 4: Add a flush method for graceful shutdown**

Add a public method to ensure background work completes:

```rust
impl StateServiceCommitHandlers {
    // ... existing methods ...

    /// Blocks until any pending background MPT computation completes.
    /// Call before shutdown to ensure the last state root is persisted.
    pub fn flush(&self) {
        if let Some(handle) = self.pending_task.lock().take() {
            let _ = handle.join();
        }
    }
}
```

- [ ] **Step 5: Verify compilation and tests**

Run: `cargo check --workspace 2>&1 | grep "^error" | head -5`
Expected: No errors

Run: `cargo test --workspace 2>&1 | grep -c "^test result: ok"`
Expected: 169

- [ ] **Step 6: Commit**

```bash
git add neo-core/src/state_service/commit_handlers.rs
git commit -m "perf: defer MPT state root computation to background thread

Move update_local_state_root_snapshot() + update_local_state_root()
to a background std::thread spawned during blockchain_committing.
Previous block's MPT is joined before starting the next, ensuring
sequential ordering while overlapping MPT I/O with the next block's
transaction execution.

Eliminates 10-100ms of synchronous trie computation from the
block persistence critical path.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Final Verification

- [ ] **Step 1: Full workspace test suite**

Run: `cargo test --workspace 2>&1 | grep -c "^test result: ok"`
Expected: 169

- [ ] **Step 2: Push**

```bash
git push
```
