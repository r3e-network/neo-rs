# Phase 1: Fast Initial Sync — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 30-45% faster initial block sync via RocksDB tuning, parallel signature verification, Arc\<Block\>, and DashMap caches.

**Architecture:** Four independent optimizations applied in order: (1) RocksDB write-heavy constants, (2) rayon parallel verification, (3) Arc\<Block\> to eliminate deep copies, (4) DashMap for concurrent block caches. Each is independently committable.

**Tech Stack:** Rust, RocksDB, rayon 1.10, dashmap 5.5

---

### Task 1: RocksDB Write-Heavy Tuning

**Files:**
- Modify: `neo-core/src/persistence/providers/rocksdb_store_provider.rs:1042-1060`

- [ ] **Step 1: Update write buffer and memtable settings**

In `build_db_options()`, change these existing lines:

```rust
// neo-core/src/persistence/providers/rocksdb_store_provider.rs
// FIND (around line 1042-1060):
    options.set_max_background_jobs(16);
    options.set_bytes_per_sync(0);
    // ...
    } else {
        options.set_write_buffer_size(64 * 1024 * 1024);
    }
    options.set_max_write_buffer_number(4);
    options.set_min_write_buffer_number_to_merge(2);

    // Advanced Performance Tuning
    options.set_allow_mmap_reads(true);
    options.set_allow_mmap_writes(false);
    options.set_enable_pipelined_write(true);
    options.set_memtable_prefix_bloom_ratio(0.02);

// REPLACE WITH:
    if let Ok(parallelism) = std::thread::available_parallelism() {
        options.set_max_background_jobs(parallelism.get() as i32);
    } else {
        options.set_max_background_jobs(16);
    }
    options.set_bytes_per_sync(1048576); // 1MB — smooth I/O instead of bursty

    // ...
    } else {
        options.set_write_buffer_size(256 * 1024 * 1024); // 256MB for fewer flushes during sync
    }
    options.set_max_write_buffer_number(6);
    options.set_min_write_buffer_number_to_merge(2);

    // Advanced Performance Tuning
    options.set_allow_mmap_reads(true);
    options.set_allow_mmap_writes(false);
    options.set_enable_pipelined_write(true);
    options.set_memtable_prefix_bloom_ratio(0.1); // better hit rate on memtable lookups

    // Delay write stalls during heavy initial sync
    options.set_level_zero_slowdown_writes_trigger(30);
    options.set_level_zero_stop_writes_trigger(48);
    options.set_max_total_wal_size(512 * 1024 * 1024); // 512MB WAL cap
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p neo-core 2>&1 | grep "^error" | head -5`
Expected: No errors

- [ ] **Step 3: Run tests**

Run: `cargo test -p neo-core 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add neo-core/src/persistence/providers/rocksdb_store_provider.rs
git commit -m "perf: tune RocksDB for write-heavy initial sync

- write_buffer_size: 64MB → 256MB (fewer memtable flushes)
- max_write_buffer_number: 4 → 6 (more buffering before stall)
- memtable_prefix_bloom_ratio: 0.02 → 0.1 (better hit rate)
- max_background_jobs: dynamic to match CPU cores
- bytes_per_sync: 0 → 1MB (smooth I/O)
- level_zero_slowdown/stop: relaxed triggers (30/48)
- max_total_wal_size: 512MB cap"
```

---

### Task 2: Parallel Transaction Signature Verification

**Files:**
- Modify: `Cargo.toml` (workspace dependency)
- Modify: `neo-core/Cargo.toml` (enable rayon)
- Modify: `neo-core/src/network/p2p/payloads/block.rs:214-233`

- [ ] **Step 1: Add rayon workspace dependency**

In workspace `Cargo.toml`, add under `[workspace.dependencies]`:

```toml
rayon = "1.10"
```

In `neo-core/Cargo.toml`, change the existing optional rayon entry to non-optional under `[dependencies]`:

```toml
rayon = { workspace = true }
```

(Remove the `optional = true` from the existing `rayon = { version = "1.10", optional = true }` line and change it to use the workspace version.)

- [ ] **Step 2: Verify dependency resolves**

Run: `cargo check -p neo-core 2>&1 | grep "^error" | head -5`
Expected: No errors

- [ ] **Step 3: Implement two-phase parallel verification**

In `neo-core/src/network/p2p/payloads/block.rs`, replace `verify_transactions()`:

```rust
// FIND (line 214-233):
    fn verify_transactions(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        let snapshot = store_cache.data_cache();
        let mut context = TransactionVerificationContext::new();
        for (index, tx) in self.transactions.iter().enumerate() {
            let result = tx.verify(settings, snapshot, Some(&context), &[]);
            if result != VerifyResult::Succeed {
                tracing::warn!(
                    target: "neo::block",
                    block_index = self.header.index(),
                    tx_index = index,
                    tx_hash = %tx.hash(),
                    result = ?result,
                    "Transaction failed verification"
                );
                return false;
            }
            context.add_transaction(tx);
        }
        true
    }

// REPLACE WITH:
    fn verify_transactions(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        use rayon::prelude::*;

        // Phase 1: parallel state-independent verification (includes signatures).
        // This is pure computation with no shared mutable state.
        let block_index = self.header.index();
        let failed = self.transactions
            .par_iter()
            .enumerate()
            .find_any(|(_, tx)| tx.verify_state_independent(settings) != VerifyResult::Succeed);

        if let Some((index, tx)) = failed {
            tracing::warn!(
                target: "neo::block",
                block_index,
                tx_index = index,
                tx_hash = %tx.hash(),
                result = ?tx.verify_state_independent(settings),
                "Transaction failed state-independent verification"
            );
            return false;
        }

        // Phase 2: sequential state-dependent verification (needs shared context).
        let snapshot = store_cache.data_cache();
        let mut context = TransactionVerificationContext::new();
        for (index, tx) in self.transactions.iter().enumerate() {
            let result = tx.verify_state_dependent(settings, snapshot, Some(&context), &[]);
            if result != VerifyResult::Succeed {
                tracing::warn!(
                    target: "neo::block",
                    block_index,
                    tx_index = index,
                    tx_hash = %tx.hash(),
                    result = ?result,
                    "Transaction failed state-dependent verification"
                );
                return false;
            }
            context.add_transaction(tx);
        }
        true
    }
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p neo-core 2>&1 | grep "^error" | head -5`
Expected: No errors

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml neo-core/Cargo.toml neo-core/src/network/p2p/payloads/block.rs
git commit -m "perf: parallelize transaction signature verification with rayon

Split verify_transactions() into two phases:
1. Parallel state-independent checks (signatures, script validation)
   via rayon par_iter — scales with CPU cores
2. Sequential state-dependent checks (expiry, conflicts, policy)
   retains existing TransactionVerificationContext semantics

On 8-core machine, phase 1 is ~8x faster for blocks with 100+ txs."
```

---

### Task 3: Arc\<Block\> to Eliminate Deep Copies

**Files:**
- Modify: `neo-core/src/ledger/blockchain/types.rs:9-11,26-28`
- Modify: `neo-core/src/ledger/blockchain/mod.rs:117-133,141-152`
- Modify: `neo-core/src/ledger/blockchain/block_processing.rs` (full file)
- Modify: `neo-core/src/ledger/blockchain/handlers.rs` (block-related methods)

- [ ] **Step 1: Update types to use Arc\<Block\>**

In `neo-core/src/ledger/blockchain/types.rs`:

```rust
// FIND:
pub(super) struct UnverifiedBlocksList {
    pub(super) blocks: Vec<Block>,
    nodes: HashSet<String>,
}
// ...
pub struct PersistCompleted {
    /// The block that was persisted.
    pub block: Block,
}

// REPLACE WITH:
pub(super) struct UnverifiedBlocksList {
    pub(super) blocks: Vec<Arc<Block>>,
    nodes: HashSet<String>,
}
// ...
pub struct PersistCompleted {
    /// The block that was persisted.
    pub block: Arc<Block>,
}
```

Add `use std::sync::Arc;` at the top of the file if not already present.

- [ ] **Step 2: Update Blockchain struct cache types**

In `neo-core/src/ledger/blockchain/mod.rs`, update the struct fields:

```rust
// FIND (line 117-118):
    _block_cache: Arc<RwLock<HashMap<UInt256, Block>>>,
    _block_cache_unverified: Arc<RwLock<HashMap<u32, UnverifiedBlocksList>>>,

// REPLACE WITH:
    _block_cache: Arc<RwLock<HashMap<UInt256, Arc<Block>>>>,
    _block_cache_unverified: Arc<RwLock<HashMap<u32, UnverifiedBlocksList>>>,
```

(The unverified cache doesn't change its outer type since UnverifiedBlocksList already holds `Vec<Arc<Block>>` from Step 1.)

- [ ] **Step 3: Update persist_block_via_system**

In `neo-core/src/ledger/blockchain/mod.rs`:

```rust
// FIND (line 141-152):
    fn persist_block_via_system(&self, block: &Block) -> bool {
        // ...
        let hash = block.header.clone().hash();
        match system.persist_block(block.clone()) {

// REPLACE WITH:
    fn persist_block_via_system(&self, block: &Block) -> bool {
        // ...
        let hash = block.header.hash();
        match system.persist_block(block.clone()) {
```

(Just remove the `.clone()` on header — `hash()` takes `&self`.)

- [ ] **Step 4: Update on_new_block to take Arc\<Block\>**

In `neo-core/src/ledger/blockchain/block_processing.rs`, update the full method:

```rust
// FIND (line 8):
    pub(super) async fn on_new_block(&self, block: &Block, verify: bool) -> VerifyResult {

// REPLACE WITH:
    pub(super) async fn on_new_block(&self, block: Arc<Block>, verify: bool) -> VerifyResult {
```

Then replace all `block.clone()` with `Arc::clone(&block)` throughout on_new_block:

```rust
// line 14: remove the .clone() on header (hash() takes &self)
        let hash = block.header.hash();

// line 31:
            self.add_unverified_block(Arc::clone(&block)).await;

// line 84:
            cache.insert(hash, Arc::clone(&block));

// line 88:
            if self.persist_block_sequence(Arc::clone(&block)).await {

// line 95:
                header_cache.add(block.header.clone());

// line 97:
            self.add_unverified_block(Arc::clone(&block)).await;
```

- [ ] **Step 5: Update add_unverified_block and persist_block_sequence**

```rust
// FIND (line 102):
    async fn add_unverified_block(&self, block: Block) {

// REPLACE WITH:
    async fn add_unverified_block(&self, block: Arc<Block>) {
```

And in `persist_block_sequence`:

```rust
// FIND (line 124):
    async fn persist_block_sequence(&self, block: Block) -> bool {

// REPLACE WITH:
    async fn persist_block_sequence(&self, block: Arc<Block>) -> bool {
```

The body references to `block` work via Arc deref. The `PersistCompleted { block }` at line 130 and 161 now passes `Arc<Block>` which matches the updated type from Step 1.

- [ ] **Step 6: Update handlers.rs**

In `neo-core/src/ledger/blockchain/handlers.rs`, replace `block.clone()` with `Arc::clone(&block)` at lines 86, 94, 127, 233, 371 (anywhere a block is cloned for caching or event publishing).

For `handle_persist_completed`, update the parameter type of `PersistCompleted` to use the `Arc<Block>` from the updated struct (Step 1). All `.clone()` on blocks inside this method become `Arc::clone(&block)`.

- [ ] **Step 7: Update callers of on_new_block**

Search for all call sites of `on_new_block` and `handle_new_block` — wrap the `Block` argument in `Arc::new()` at the boundary where blocks are first received:

```rust
// Wherever blocks enter the system from network:
let block: Block = deserialize_from_network(...);
let result = blockchain.on_new_block(Arc::new(block), true).await;
```

- [ ] **Step 8: Verify compilation and tests**

Run: `cargo check --workspace 2>&1 | grep "^error" | head -10`
Expected: No errors

Run: `cargo test --workspace 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add neo-core/src/ledger/blockchain/
git commit -m "perf: use Arc<Block> to eliminate deep block copies

Wrap Block in Arc at network boundary. All downstream processing
shares the same allocation via Arc::clone() (~1ns atomic increment)
instead of deep-copying header + transactions (~10-100µs).

Eliminates 3-8 block.clone() calls per block processing cycle."
```

---

### Task 4: DashMap for Block Caches

**Files:**
- Modify: `neo-core/Cargo.toml` (add dashmap)
- Modify: `neo-core/src/ledger/blockchain/mod.rs` (cache types + constructor)
- Modify: `neo-core/src/ledger/blockchain/block_processing.rs` (cache access)
- Modify: `neo-core/src/ledger/blockchain/handlers.rs` (cache access)

- [ ] **Step 1: Add dashmap dependency**

In `neo-core/Cargo.toml` under `[dependencies]`, add:

```toml
dashmap = { workspace = true }
```

- [ ] **Step 2: Update Blockchain struct to use DashMap**

In `neo-core/src/ledger/blockchain/mod.rs`:

```rust
// Add import:
use dashmap::DashMap;

// FIND (line 117-121):
    _block_cache: Arc<RwLock<HashMap<UInt256, Arc<Block>>>>,
    _block_cache_unverified: Arc<RwLock<HashMap<u32, UnverifiedBlocksList>>>,
    _extensible_witness_white_list: Arc<RwLock<HashSet<UInt160>>>,
    _inventory_cache: Arc<RwLock<HashMap<InventoryCacheKey, InventoryPayload>>>,

// REPLACE WITH:
    _block_cache: Arc<DashMap<UInt256, Arc<Block>>>,
    _block_cache_unverified: Arc<DashMap<u32, UnverifiedBlocksList>>,
    _extensible_witness_white_list: Arc<RwLock<HashSet<UInt160>>>,
    _inventory_cache: Arc<DashMap<InventoryCacheKey, InventoryPayload>>,
```

Update the constructor:

```rust
// FIND (line 129-133):
            _block_cache: Arc::new(RwLock::new(HashMap::with_capacity(1024))),
            _block_cache_unverified: Arc::new(RwLock::new(HashMap::with_capacity(256))),
            _extensible_witness_white_list: Arc::new(RwLock::new(HashSet::new())),
            _inventory_cache: Arc::new(RwLock::new(HashMap::with_capacity(2048))),

// REPLACE WITH:
            _block_cache: Arc::new(DashMap::with_capacity(1024)),
            _block_cache_unverified: Arc::new(DashMap::with_capacity(256)),
            _extensible_witness_white_list: Arc::new(RwLock::new(HashSet::new())),
            _inventory_cache: Arc::new(DashMap::with_capacity(2048)),
```

- [ ] **Step 3: Update block_processing.rs cache access**

Replace all `self._block_cache.write().await` and `self._block_cache.read().await` patterns with direct DashMap calls. Key changes in `on_new_block`:

```rust
// FIND (line 71-85) — block cache write lock:
        {
            let mut cache = self._block_cache.write().await;
            if cache.contains_key(&hash) {
                return VerifyResult::AlreadyExists;
            }
            if cache.len() >= MAX_BLOCK_CACHE_SIZE {
                // ...
                return VerifyResult::Invalid;
            }
            cache.insert(hash, Arc::clone(&block));
        }

// REPLACE WITH:
        {
            if self._block_cache.contains_key(&hash) {
                return VerifyResult::AlreadyExists;
            }
            if self._block_cache.len() >= MAX_BLOCK_CACHE_SIZE {
                tracing::warn!(
                    target: "neo",
                    cache_size = self._block_cache.len(),
                    "block cache full, rejecting new block"
                );
                return VerifyResult::Invalid;
            }
            self._block_cache.insert(hash, Arc::clone(&block));
        }
```

Similarly update `add_unverified_block` and `persist_block_sequence` to use DashMap directly instead of `.write().await`.

For `add_unverified_block`, the entry API differs slightly:
```rust
// DashMap entry API:
self._block_cache_unverified
    .entry(block.index())
    .or_insert_with(UnverifiedBlocksList::new)
    .blocks.push(block);
```

For `persist_block_sequence`, the `.get_mut()` pattern:
```rust
// DashMap get_mut:
if let Some(mut entry) = self._block_cache_unverified.get_mut(&next_index) {
    if let Some(next_block) = entry.blocks.pop() {
        if entry.blocks.is_empty() {
            drop(entry); // release shard lock before remove
            self._block_cache_unverified.remove(&next_index);
        }
        Some(next_block)
    } else {
        drop(entry);
        self._block_cache_unverified.remove(&next_index);
        None
    }
}
```

- [ ] **Step 4: Update handlers.rs cache access**

Same pattern — replace `.write().await` / `.read().await` with direct DashMap calls:

```rust
// handlers.rs handle_persist_completed:
// FIND:
        {
            let mut cache = self._block_cache.write().await;
            cache.insert(hash, Arc::clone(&block));
            // ...
            cache.remove(&prev_hash);
        }

// REPLACE WITH:
        {
            self._block_cache.insert(hash, Arc::clone(&block));
            let prev_hash = *block.prev_hash();
            if !prev_hash.is_zero() {
                self._block_cache.remove(&prev_hash);
            }
        }
```

And for unverified cache removal:
```rust
// FIND:
        {
            let mut unverified = self._block_cache_unverified.write().await;
            unverified.remove(&index);
        }

// REPLACE WITH:
        self._block_cache_unverified.remove(&index);
```

- [ ] **Step 5: Update inventory cache access**

Search for all `_inventory_cache.write().await` and `_inventory_cache.read().await` patterns and convert to direct DashMap calls.

- [ ] **Step 6: Verify compilation and tests**

Run: `cargo check --workspace 2>&1 | grep "^error" | head -10`
Expected: No errors

Run: `cargo test --workspace 2>&1 | tail -5`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
git add neo-core/Cargo.toml neo-core/src/ledger/blockchain/
git commit -m "perf: replace RwLock<HashMap> block caches with DashMap

DashMap uses sharded locking — readers on different shards never
block each other, and writers only lock their shard. Eliminates
global write-lock contention under concurrent peer connections.

Caches migrated: _block_cache, _block_cache_unverified, _inventory_cache.
_extensible_witness_white_list stays as RwLock (rebuilt atomically)."
```

---

### Task 5: Final Verification

- [ ] **Step 1: Full workspace test suite**

Run: `cargo test --workspace 2>&1 | grep -c "^test result: ok"`
Expected: 169

- [ ] **Step 2: Verify no regressions**

Run: `cargo bench -p neo-vm --bench vm_execution -- --quick 2>&1 | tail -20`
Expected: VM benchmarks unchanged or improved

- [ ] **Step 3: Push all commits**

```bash
git push
```
