# Lock Ordering Guide

This document defines the lock hierarchy across the Neo-rs codebase to prevent deadlocks.

## Overview

When multiple locks need to be acquired, they MUST be acquired in the order specified below. Violating this ordering can lead to deadlocks where two threads wait for each other indefinitely.

## Global Lock Hierarchy

### Level 1: Storage Layer
These locks must be acquired first as they represent the foundation of data access.

1. **Store locks** (persistence layer)
   - `StoreCache` internal locks
   - RocksDB locks (internal to storage backend)

### Level 2: Ledger State
Locks that manage blockchain state and headers.

2. **LedgerContext locks** (in order):
   - `hashes_by_index` (RwLock)
   - `headers_by_index` (RwLock)
   - `blocks_by_hash` (RwLock)
   - `extensibles_by_hash` (RwLock)
   - `transactions_by_hash` (RwLock)

3. **HeaderCache locks**
   - Internal cache locks

### Level 3: System Context
System-wide state and registries.

4. **ServiceRegistry locks** (in order):
   - `services_by_name` (RwLock)
   - `typed_services` (RwLock)
   - `services` (RwLock)

5. **NeoSystemContext locks** (in order):
   - `system` (RwLock)
   - `current_wallet` (RwLock)
   - `wallet_changed_handlers` (RwLock)

### Level 4: Consensus and Mempool
Transaction and consensus state.

6. **MemoryPool locks**
   - `memory_pool` (Mutex)

7. **RelayExtensibleCache locks**
   - Internal relay cache locks

### Level 5: Event Handlers
Event notification locks (can be acquired in any order among themselves).

8. **Event handler locks** (any order):
   - `committing_handlers` (RwLock)
   - `committed_handlers` (RwLock)
   - `transaction_added_handlers` (RwLock)
   - `transaction_removed_handlers` (RwLock)
   - `log_handlers` (RwLock)
   - `logging_handlers` (RwLock)
   - `notify_handlers` (RwLock)

### Level 6: P2P and Network
Network-related locks should be acquired last.

9. **LocalNode state locks** (in order):
   - `peers_by_ip` (RwLock)
   - `peers` (RwLock)
   - `remote_nodes` (RwLock)
   - `pending_connections` (RwLock)
   - `broadcasts` (RwLock)
   - `seed_list` (RwLock)
   - `port` (RwLock)
   - `system_context` (RwLock)
   - `config` (RwLock)
   - `capabilities` (RwLock)
   - `ban_list` (RwLock)
   - `inbound_rate_limiter` (RwLock)

10. **NeoSystem locks**
    - `self_ref` (Mutex)

## Rules

### Core Principles

1. **Never hold a lock across an `.await` point**
   - Async operations can yield control, causing locks to be held for unpredictable durations
   - If async operations are needed, clone the data or drop the lock before awaiting
   
   ```rust
   // WRONG - holding lock across await
   let guard = self.data.lock();
   some_async_call().await;
   drop(guard);
   
   // CORRECT - clone and drop before await
   let data = self.data.lock().clone();
   drop(guard);
   some_async_call().await;
   
   // CORRECT - scope ensures lock is dropped
   let value = {
       self.data.lock().clone()
   };
   some_async_call().await;
   ```

2. **Always acquire locks in the order specified above**
   - If you need locks from levels 2 and 4, acquire level 2 first
   - Within a level, acquire locks in the specified order

3. **Minimize lock scope**
   - Hold locks for the shortest time possible
   - Use explicit scopes `{}` to ensure early drop

4. **Never acquire the same lock twice**
   - This includes recursive calls
   - Use lock-free methods or pass data by value

5. **Document lock ordering in complex functions**
   - Add comments when multiple locks are involved
   - Reference this guide in documentation

### Safe Patterns

#### Pattern 1: Sequential Lock Acquisition
```rust
// Acquire locks in order, drop early
{
    let hashes = self.ledger.hashes_by_index.write();
    // ... modify hashes ...
} // Drop level 2 lock

{
    let mempool = self.memory_pool.lock();
    // ... modify mempool ...
} // Drop level 4 lock
```

#### Pattern 2: Read-Only Access
```rust
// Multiple read locks are safe (no deadlock with other readers)
let hashes = self.ledger.hashes_by_index.read();
let headers = self.ledger.headers_by_index.read();
// ... read operations ...
```

#### Pattern 3: Lock and Clone
```rust
// Clone data to avoid holding lock during expensive operations
let data = {
    let guard = self.shared_state.read();
    guard.clone()
};
// Lock is dropped, now do expensive work with `data`
expensive_operation(data);
```

### Dangerous Patterns

#### Anti-pattern 1: Reverse Order Acquisition
```rust
// WRONG - acquiring higher-level lock before lower-level lock
let mempool = self.memory_pool.lock();  // Level 4
let hashes = self.ledger.hashes_by_index.write();  // Level 2 - DEADLOCK RISK!
```

#### Anti-pattern 2: Holding Lock Across Await
```rust
// WRONG - async operation while holding lock
let guard = self.data.lock();
some_async_fn().await;  // DEADLOCK RISK - lock held during yield
drop(guard);
```

#### Anti-pattern 3: Nested Lock Acquisition Without Order
```rust
// WRONG - acquiring locks without considering hierarchy
fn update_state(&self) {
    let a = self.lock_a.write();  // Unknown level
    self.helper_that_locks();      // Takes another lock - what level?
}
```

## Module-Specific Guidelines

### LedgerContext (`neo-core/src/ledger/ledger_context.rs`)

The `LedgerContext` contains multiple independent `RwLock` fields. When multiple locks are needed:

1. `hashes_by_index` first
2. `headers_by_index` second
3. `blocks_by_hash` third
4. Other locks follow alphabetically

Most methods only acquire a single lock, which is safe.

### ServiceRegistry (`neo-core/src/neo_system/registry.rs`)

Lock order is documented in the file:
1. `services_by_name`
2. `typed_services`
3. `services`

The `register()` method correctly follows this order.

### NeoSystem (`neo-core/src/neo_system/core.rs`)

Complex lock interactions documented at struct level. Follow the order specified in module documentation.

### Blockchain Actor (`neo-core/src/ledger/blockchain/mod.rs`)

Uses async RwLocks from tokio. Same principles apply:
- Never hold across message processing boundaries
- Acquire in hierarchy order

## Testing for Deadlocks

### Manual Testing
1. Run stress tests with multiple concurrent operations
2. Use thread sanitizer: `RUSTFLAGS="-Z sanitizer=thread" cargo test`

### Runtime Detection
Consider using these tools:
- `parking_lot` (already in use) provides deadlock detection in debug builds
- Enable with `RUST_BACKTRACE=1` and `PARKING_LOT_DEADLOCK_DETECTION=1`

### Code Review Checklist
- [ ] All lock acquisitions follow hierarchy order
- [ ] No locks held across `.await` points
- [ ] Lock scopes are minimized
- [ ] Complex locking is documented
- [ ] No recursive lock acquisitions

## Violation Examples and Fixes

### Example 1: Wrong Order in Service Registration

**Problem:**
```rust
// Acquiring in wrong order
let services = self.services.write();       // Level 3 (lowest)
let typed = self.typed_services.write();    // Level 2 (higher)
```

**Fix:**
```rust
// Correct order
let typed = self.typed_services.write();    // Level 2 first
let services = self.services.write();       // Level 3 second
```

### Example 2: Lock Held Across Await

**Problem:**
```rust
async fn process_block(&self, block: Block) {
    let mut cache = self.block_cache.write();
    let verified = verify_block(&block).await;  // WRONG - lock held
    cache.insert(block.hash(), verified);
}
```

**Fix:**
```rust
async fn process_block(&self, block: Block) {
    let verified = verify_block(&block).await;  // Verify first
    let mut cache = self.block_cache.write();   // Then lock
    cache.insert(block.hash(), verified);
}  // Lock dropped at scope end
```

## Future Work

1. **Static Analysis**: Develop linting rules to detect lock ordering violations
2. **Lock Wrapper Types**: Create typed wrappers that encode hierarchy in the type system
3. **Lock-Free Alternatives**: Consider lock-free data structures where appropriate
4. **Performance Profiling**: Identify lock contention hotspots

## References

- Rust Nomicon: [Data Races and Race Conditions](https://doc.rust-lang.org/nomicon/races.html)
- `parking_lot` documentation: [Deadlock Detection](https://docs.rs/parking_lot/latest/parking_lot/#deadlock-detection)
- [Lock Ordering Best Practices](https://en.wikipedia.org/wiki/Lock_(computer_science)#Avoiding_deadlock)

## Changelog

- 2024-01: Initial lock hierarchy documentation
- Current: Documented all major subsystem lock orders
