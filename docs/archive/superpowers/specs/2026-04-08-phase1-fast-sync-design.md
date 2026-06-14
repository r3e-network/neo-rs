# Phase 1: Fast Initial Sync

**Goal**: 30-45% faster block sync during initial chain catchup by addressing four low-risk system bottlenecks.

**Context**: The VM layer has been micro-optimized (51-85% faster per opcode across 15 commits). The remaining bottlenecks are architectural: block cloning, lock contention, sequential signature verification, and conservative RocksDB defaults.

## 1A. RocksDB Write-Heavy Tuning

**File**: `neo-core/src/persistence/providers/rocksdb_store_provider.rs`, function `build_db_options()` (line 1013)

**Problem**: Default settings are tuned for balanced read/write. During initial sync, writes outnumber reads 100:1. The 64MB write buffer fills quickly, triggering frequent memtable flushes that stall writes.

**Changes** (all in `build_db_options()`):

| Setting | Current | New | Rationale |
|---------|---------|-----|-----------|
| `write_buffer_size` | 64MB | 256MB | Fewer memtable flushes during block batch writes |
| `max_write_buffer_number` | 4 | 6 | More memtables buffer before write stall |
| `memtable_prefix_bloom_ratio` | 0.02 | 0.1 | Better hit rate on memtable key lookups |
| `max_background_jobs` | 16 (hardcoded) | `available_parallelism` | Match CPU core count dynamically |
| `bytes_per_sync` | 0 (bursty) | 1048576 (1MB) | Smooth I/O instead of large bursts |

**New settings** to add:

```rust
options.set_level_zero_slowdown_writes_trigger(30);  // default 20
options.set_level_zero_stop_writes_trigger(48);       // default 36
options.set_max_total_wal_size(512 * 1024 * 1024);    // 512MB WAL cap
```

These delay RocksDB's internal write throttling during heavy sync, avoiding stalls. The WAL cap prevents unbounded WAL growth.

**Risk**: Very low — these are tuning constants. If write amplification increases, the existing configurable `write_buffer_size` field already provides an escape hatch.

## 1B. Arc\<Block\> to Eliminate Block Cloning

**Files**: `neo-core/src/ledger/blockchain/mod.rs`, `block_processing.rs`, `handlers.rs`

**Problem**: `Block` is cloned 3-8 times per processing cycle:
- `block_processing.rs:31` — `self.add_unverified_block(block.clone())`
- `block_processing.rs:84` — `cache.insert(hash, block.clone())`
- `block_processing.rs:88` — `self.persist_block_sequence(block.clone())`
- `block_processing.rs:97` — `self.add_unverified_block(block.clone())`
- `handlers.rs:86` — `cache.insert(hash, block.clone())`
- `handlers.rs:94` — `self.ledger.insert_block(block.clone())`
- `handlers.rs:127,233,371` — additional clones for relay/events

A `Block` contains a `Header` (~150 bytes) and `Vec<Transaction>` (100s of KB for full blocks). Each clone is a deep copy.

**Change**: Wrap `Block` in `Arc<Block>` at the entry point (network receive / deserialization). All downstream code shares the same allocation.

```rust
// mod.rs — cache types change
_block_cache: Arc<DashMap<UInt256, Arc<Block>>>,          // was RwLock<HashMap<..., Block>>
_block_cache_unverified: Arc<DashMap<u32, Vec<Arc<Block>>>>,

// block_processing.rs — entry point wraps in Arc
pub(super) async fn on_new_block(&self, block: Arc<Block>, verify: bool) -> VerifyResult {
    let hash = block.header.hash();
    // All downstream: Arc::clone(&block) instead of block.clone()
}
```

**Callers that need `&Block`**: Work automatically via `Arc::deref()`.

**Callers that need owned `Block`**: `Arc::try_unwrap(block).unwrap_or_else(|arc| (*arc).clone())` — rare, only at final consumption points.

**Method signatures that change**:
- `on_new_block(&self, block: &Block, ...)` → `on_new_block(&self, block: Arc<Block>, ...)`
- `add_unverified_block(block: Block)` → `add_unverified_block(block: Arc<Block>)`
- `persist_block_sequence(block: Block)` → `persist_block_sequence(block: Arc<Block>)`
- `handle_new_block(block: Block)` → `handle_new_block(block: Arc<Block>)`

**Risk**: Low. Arc\<Block\> auto-derefs to `&Block`. Most call sites pass `&block` which works unchanged. The `block.header.hash()` call that previously required `.clone()` now works directly since `hash()` takes `&self` with interior mutability.

## 1C. DashMap for Block Caches

**File**: `neo-core/src/ledger/blockchain/mod.rs`

**Problem**: Block caches use `Arc<RwLock<HashMap>>`:
```rust
_block_cache: Arc<RwLock<HashMap<UInt256, Block>>>,
_block_cache_unverified: Arc<RwLock<HashMap<u32, UnverifiedBlocksList>>>,
_inventory_cache: Arc<RwLock<HashMap<InventoryCacheKey, InventoryPayload>>>,
```

Under concurrent peer connections, readers block on writers and vice versa. A single slow insert blocks all lookups.

**Change**: Replace with `DashMap` (already a workspace dependency at v5.5):
```rust
_block_cache: Arc<DashMap<UInt256, Arc<Block>>>,
_block_cache_unverified: Arc<DashMap<u32, Vec<Arc<Block>>>>,
_inventory_cache: Arc<DashMap<InventoryCacheKey, InventoryPayload>>,
```

**API migration**:
- `cache.write().await.insert(k, v)` → `cache.insert(k, v)`
- `cache.read().await.get(&k)` → `cache.get(&k)`
- `cache.write().await.remove(&k)` → `cache.remove(&k)`

**The `_inventory_cache_order` VecDeque** stays as `Arc<RwLock<VecDeque>>` — it's an ordered queue, not a lookup structure. DashMap doesn't preserve insertion order.

**Risk**: Low. DashMap is a mature crate (50M+ downloads). API is a superset of HashMap. The only subtlety: `DashMap::get()` returns a `Ref<K,V>` guard that holds a shard lock, so references must not be held across `.await` points. All current usage patterns are short-lived (get → clone/check → drop), so this is safe.

## 1D. Parallel Transaction Signature Verification

**Problem**: During block verification, each transaction's witnesses are checked sequentially. With ~100 transactions per block and ~0.5ms per ECDSA verification, this is ~50ms per block of serial CPU work.

**Change**: Add `rayon` to workspace dependencies and parallelize the verification loop.

**Dependency**: Add to workspace `Cargo.toml`:
```toml
rayon = "1.10"
```

And to `neo-core/Cargo.toml`:
```toml
rayon = { workspace = true }
```

**Implementation site**: `neo-core/src/network/p2p/payloads/block.rs`, method `verify_transactions()` (line 214).

Transaction verification already has a two-phase split:
- `tx.verify_state_independent(settings)` — Pure checks including signature verification. No shared state. **Parallelizable.**
- `tx.verify_state_dependent(settings, snapshot, context, ...)` — Conflict detection using shared `TransactionVerificationContext`. **Must be sequential.**

The optimization: run all state-independent checks in parallel first, then run state-dependent checks sequentially only for transactions that passed phase 1.

```rust
// Phase 1: parallel state-independent verification (includes signatures)
use rayon::prelude::*;
let independent_results: Vec<(usize, VerifyResult)> = self.transactions
    .par_iter()
    .enumerate()
    .map(|(i, tx)| (i, tx.verify_state_independent(settings)))
    .collect();

// Short-circuit on first failure
for (index, result) in &independent_results {
    if *result != VerifyResult::Succeed {
        // log warning and return false
        return false;
    }
}

// Phase 2: sequential state-dependent verification
let snapshot = store_cache.data_cache();
let mut context = TransactionVerificationContext::new();
for (index, tx) in self.transactions.iter().enumerate() {
    let result = tx.verify_state_dependent(settings, snapshot, Some(&context), &[]);
    if result != VerifyResult::Succeed {
        return false;
    }
    context.add_transaction(tx);
}
```

**Safety**: Phase 1 is pure computation — reads transaction data, performs ECDSA, returns pass/fail. Phase 2 retains the existing sequential semantics with `TransactionVerificationContext`.

**Fallback**: If the node has only 1 CPU core, rayon degrades to sequential execution automatically.

**Risk**: Low. Rayon is the standard Rust parallelism library. Signature verification is embarrassingly parallel. The only requirement is that `snapshot` and `settings` implement `Sync` (they do, as they're behind `Arc`).

## Testing Strategy

1. **Existing tests**: All 169 test suites must continue passing — these changes don't alter logic, only performance characteristics.
2. **Sync benchmark**: Measure blocks-per-second syncing the first 10,000 blocks before and after.
3. **RocksDB**: Monitor `rocksdb.stall` metrics during sync to confirm fewer write stalls.

## Implementation Order

1. **1A (RocksDB)** — Pure constant changes, zero dependency on other parts
2. **1D (rayon)** — New dependency + parallel loop, independent of cache changes
3. **1B (Arc\<Block\>)** — Type change that propagates through block processing
4. **1C (DashMap)** — Cache type change, depends on 1B being done first (combines with Arc change)

Each step is independently testable and committable.
