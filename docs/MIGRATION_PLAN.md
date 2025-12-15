# Architecture Migration Plan

## Overview

This document outlines the step-by-step migration plan to align neo-rs architecture with the ideal 14-crate design.

## Phase 1: Break the Monolith (Critical Priority)

### Task 1.1: Consolidate Mempool (LOW RISK - START HERE)

**Objective**: Remove duplicate MemoryPool, use neo-mempool as single source of truth.

**Current State**:

- `neo-core/src/ledger/memory_pool.rs` (700 LOC) - C# parity implementation
- `neo-mempool/src/pool.rs` (445 LOC) - Rust-idiomatic implementation

**Migration Steps**:

1. **Analyze feature parity** between the two implementations

    ```bash
    # Key differences to reconcile:
    # - neo-core version: TransactionVerificationContext, conflict handling
    # - neo-mempool version: FeePolicy, priority queue, statistics
    ```

2. **Enhance neo-mempool** to include C# parity features:
    - Add `TransactionVerificationContext`
    - Add conflict attribute handling (`Conflicts` attribute)
    - Add `reverify_top_unverified_transactions` logic
    - Add event callbacks (transaction_added, transaction_removed)

3. **Update neo-core to depend on neo-mempool**:

    ```toml
    # neo-core/Cargo.toml
    [dependencies]
    neo-mempool = { path = "../neo-mempool" }
    ```

4. **Create backward-compatible re-export**:

    ```rust
    // neo-core/src/ledger/mod.rs
    pub use neo_mempool::Mempool as MemoryPool;
    ```

5. **Remove old implementation**:
    - Delete `neo-core/src/ledger/memory_pool.rs`
    - Delete `neo-core/src/ledger/pool_item.rs`
    - Delete `neo-core/src/ledger/transaction_verification_context.rs`

6. **Update all consumers** to use new import path

**Validation**:

```bash
cargo build --all-features
cargo test -p neo-mempool
cargo test -p neo-core --test '*mempool*'
```

### Task 1.2: Consolidate Telemetry (LOW RISK)

**Objective**: Remove `neo-core/src/telemetry/`, use `neo-telemetry` exclusively.

**Current State**:

- `neo-telemetry/` - Standalone metrics crate
- `neo-core/src/telemetry/` - Duplicate telemetry code

**Migration Steps**:

1. **Compare implementations**:

    ```rust
    // neo-core/src/telemetry/mod.rs - What's here?
    // neo-telemetry/src/lib.rs - What's here?
    ```

2. **Move any unique code from neo-core to neo-telemetry**

3. **Update neo-core to depend on neo-telemetry**:

    ```toml
    # neo-core/Cargo.toml
    [dependencies]
    neo-telemetry = { path = "../neo-telemetry" }
    ```

4. **Create backward-compatible re-export**:

    ```rust
    // neo-core/src/lib.rs
    pub use neo_telemetry as telemetry;
    ```

5. **Remove old implementation**:
    - Delete `neo-core/src/telemetry/` directory

**Validation**:

```bash
cargo build --all-features
cargo test -p neo-telemetry
```

### Task 1.3: Extract State Service (MEDIUM RISK)

**Objective**: Create `neo-state` crate from `neo-core/src/state_service/`.

**Current State**:

- `neo-core/src/state_service/` contains state root tracking, MPT operations

**Migration Steps**:

1. **Create new crate**:

    ```bash
    mkdir neo-state
    ```

2. **Create Cargo.toml**:

    ```toml
    [package]
    name = "neo-state"
    version.workspace = true
    edition.workspace = true
    description = "State service for Neo N3 blockchain"

    [dependencies]
    neo-primitives = { workspace = true }
    neo-crypto = { workspace = true }
    neo-storage = { workspace = true }
    thiserror = { workspace = true }
    tracing = { workspace = true }
    tokio = { workspace = true }
    ```

3. **Move files**:

    ```
    neo-core/src/state_service/ -> neo-state/src/
    - state_root.rs
    - state_store.rs
    - mod.rs -> lib.rs
    ```

4. **Update imports in moved files**

5. **Add neo-state to workspace**:

    ```toml
    # Cargo.toml (workspace)
    members = [
        # ... existing
        "neo-state",
    ]
    ```

6. **Update neo-core to depend on neo-state**:

    ```toml
    # neo-core/Cargo.toml
    [dependencies]
    neo-state = { path = "../neo-state" }
    ```

7. **Create backward-compatible re-export**:
    ```rust
    // neo-core/src/lib.rs
    pub use neo_state as state_service;
    ```

**Validation**:

```bash
cargo build -p neo-state
cargo test -p neo-state
cargo build --all-features
```

## Phase 2: High Priority Migrations

### Task 2.1: Move Storage Implementations (LOW RISK)

**Objective**: Move storage implementations from neo-core to neo-storage.

**Current State**:

- `neo-storage/` - Only traits
- `neo-core/src/persistence/` - RocksDB, Memory implementations

**Migration Steps**:

1. **Move provider implementations**:

    ```
    neo-core/src/persistence/providers/ -> neo-storage/src/providers/
    - rocksdb_store_provider.rs
    - memory_store.rs
    - memory_snapshot.rs
    ```

2. **Update neo-storage Cargo.toml**:

    ```toml
    [dependencies]
    rocksdb = { workspace = true, optional = true }

    [features]
    default = []
    rocksdb = ["dep:rocksdb"]
    ```

3. **Keep DataCache in neo-core** (it has smart contract dependencies)

4. **Create backward-compatible re-exports**

### Task 2.2: Extract P2P Implementation (HIGH RISK)

**Objective**: Move P2P implementation from neo-core to neo-p2p.

**Current State**:

- `neo-p2p/` - Only type definitions (~400 LOC)
- `neo-core/src/network/` - Full implementation (~50 files)

**Migration Steps**:

1. **Identify P2P-only code** (no smart contract/ledger dependencies):
    - Message handling
    - Peer management
    - Protocol messages
    - Network encoding

2. **Identify code that must stay in neo-core**:
    - Block/transaction relay logic (needs ledger)
    - Inventory handling (needs smart contracts)

3. **Create feature flags** for gradual migration:

    ```toml
    # neo-p2p/Cargo.toml
    [features]
    default = []
    full = ["tokio/full", "socket2"]
    ```

4. **Move incrementally** with backward-compatible re-exports

5. **Extensive testing** at each step

### Task 2.3: Enhance neo-chain (MEDIUM RISK)

**Objective**: Move blockchain orchestration from neo-core to neo-chain.

**Current State**:

- `neo-chain/` - Thin wrapper (~1,500 LOC)
- `neo-core/src/ledger/` - Full blockchain logic

**Migration Steps**:

1. **Move Blockchain struct** to neo-chain:

    ```
    neo-core/src/ledger/blockchain/ -> neo-chain/src/blockchain/
    ```

2. **Keep Block/Transaction types** in neo-core (used everywhere)

3. **Move validation logic** to neo-chain:

    ```
    neo-core/src/ledger/verify_result.rs -> neo-chain/src/
    ```

4. **Update dependencies** - neo-chain depends on neo-core

## Phase 3: Polish

### Task 3.1: Clean Up neo-core

After Phase 1 and 2, neo-core should only contain:

- Block, Transaction, Witness types
- Native contracts
- Wallets
- Smart contract engine
- Application engine

Remove any remaining misplaced code.

### Task 3.2: Foundation Crate Consolidation (Optional)

Consider merging:

- `neo-io` into `neo-primitives`
- `neo-json` into `neo-primitives`

**Only do this if it simplifies the dependency graph without breaking changes.**

## Migration Checklist

### Before Each Task

- [ ] Create feature branch
- [ ] Review affected code
- [ ] Identify all consumers
- [ ] Plan backward compatibility

### During Each Task

- [ ] Move code incrementally
- [ ] Update imports
- [ ] Add re-exports for compatibility
- [ ] Update Cargo.toml dependencies
- [ ] Run tests after each file move

### After Each Task

- [ ] Full workspace build
- [ ] All tests pass
- [ ] Documentation updated
- [ ] Commit with descriptive message

## Rollback Strategy

If any migration causes issues:

1. **Immediate**: Revert to previous commit
2. **Short-term**: Use feature flags to disable new code
3. **Long-term**: Re-plan migration with smaller steps

## Success Criteria

1. **No functionality regression** - All tests pass
2. **Clean crate responsibilities** - Each crate does one thing
3. **No circular dependencies** - Clear dependency graph
4. **Build time improvement** - Incremental builds faster
5. **Documentation complete** - Each crate has clear docs

## Timeline

| Week   | Tasks                                     | Owner |
| ------ | ----------------------------------------- | ----- |
| Week 1 | Task 1.1 (Mempool), Task 1.2 (Telemetry)  | -     |
| Week 2 | Task 1.3 (State), Task 2.1 (Storage)      | -     |
| Week 3 | Task 2.2 (P2P) - Part 1                   | -     |
| Week 4 | Task 2.2 (P2P) - Part 2, Task 2.3 (Chain) | -     |
| Week 5 | Task 3.1, Task 3.2, Final testing         | -     |

## Dependencies Between Tasks

```
Task 1.1 (Mempool) ──┐
Task 1.2 (Telemetry) ├── Can run in parallel
Task 1.3 (State) ────┘
         │
         ▼
Task 2.1 (Storage) ──┐
Task 2.2 (P2P) ──────┼── Depends on Phase 1
Task 2.3 (Chain) ────┘
         │
         ▼
Task 3.1 (Clean neo-core) ── Depends on Phase 2
Task 3.2 (Foundation) ────── Optional, can be deferred
```
