# Crate Cleanup Refactoring - Development Plan

## Overview
Eliminate 20+ duplicate type definitions across neo-rs crates by consolidating cryptography, storage, and P2P types into their specialized crates while maintaining backward compatibility through re-exports from neo-core.

## Task Breakdown

### Task 1: Consolidate Cryptography Types (neo-crypto ← neo-core)
- **ID**: task-1
- **Description**: Move all cryptographic types from `neo-core/src/cryptography/` to `neo-crypto/src/`, remove duplicates, and establish `neo-crypto` as the single source of truth for all crypto operations. Add re-exports in neo-core for backward compatibility.
- **File Scope**:
  - Source: `neo-core/src/cryptography/**/*.rs`
  - Target: `neo-crypto/src/**/*.rs`
  - Re-exports: `neo-core/src/lib.rs`, `neo-core/src/cryptography/mod.rs` (convert to re-export module)
  - Affected types: `ECCurve`, `ECPoint`, `HashAlgorithm`, `Crypto`, `NeoHash`, `Secp256k1Crypto`, `Secp256r1Crypto`, `Ed25519Crypto`, `Base58`, `Hex`, `ECDsa`, `ECC`, `Bls12381Crypto`, `BloomFilter`, MPT Trie (`neo-core/src/cryptography/mpt_trie/`)
- **Dependencies**: None
- **Test Command**:
  ```bash
  cargo test --package neo-crypto --lib --all-features -- --nocapture
  cargo test --package neo-core --lib --all-features -- cryptography --nocapture
  cargo test --workspace --all-features -- --nocapture
  ```
- **Test Focus**:
  - Verify all crypto operations (hashing, signing, verification) still work
  - Test MPT Trie operations (insert, delete, proof generation)
  - Ensure BloomFilter functionality is preserved
  - Validate backward compatibility: imports from `neo_core::cryptography::*` resolve correctly
  - Check that dependent crates (neo-vm, neo-consensus, neo-plugins) compile without changes

### Task 2: Consolidate Storage Types (neo-storage ← neo-core)
- **ID**: task-2
- **Description**: Move all storage abstractions and implementations from `neo-core/src/persistence/` to `neo-storage/src/`, remove duplicates (`StorageKey`, `StorageItem`, traits), and expand neo-storage to include providers (RocksDB, Memory). Establish neo-storage as the complete storage layer.
- **File Scope**:
  - Source: `neo-core/src/persistence/**/*.rs`
  - Target: `neo-storage/src/**/*.rs`
  - Re-exports: `neo-core/src/lib.rs`, `neo-core/src/persistence/mod.rs` (convert to re-export module)
  - Affected types: `StorageKey`, `StorageItem`, `SeekDirection`, `TrackState`, `IReadOnlyStore`, `IWriteStore`, `IStore`, `IReadOnlyStoreGeneric`, `IStoreProvider`, `IStoreSnapshot`, `DataCache`, `Trackable`, `StoreCache`, `StoreFactory`, `MemoryStore`, `RocksDBStoreProvider`
  - Special attention: `neo-core/src/persistence/providers/rocksdb_store_provider.rs` (contains production RocksDB logic)
- **Dependencies**: task-1 (if storage uses crypto types, ensure imports are updated)
- **Test Command**:
  ```bash
  cargo test --package neo-storage --lib --all-features -- --nocapture
  cargo test --package neo-core --lib --all-features -- persistence --nocapture
  cargo test --package neo-core --test integration_tests -- --nocapture
  ```
- **Test Focus**:
  - Test RocksDB provider operations (open, read, write, seek, snapshot)
  - Test MemoryStore provider operations
  - Validate DataCache tracking (TrackState transitions)
  - Test snapshot isolation and consistency
  - Ensure backward compatibility: imports from `neo_core::persistence::*` resolve correctly
  - Verify ledger and mempool modules compile and function correctly

### Task 3: Consolidate P2P Types (neo-p2p ← neo-core)
- **ID**: task-3
- **Description**: Move all P2P protocol types and network logic from `neo-core/src/network/` to `neo-p2p/src/`, remove duplicates (`MessageCommand`, `MessageFlags`, `VerifyResult`, `WitnessConditionType`, etc.), and establish neo-p2p as the single source of truth for networking.
- **File Scope**:
  - Source: `neo-core/src/network/**/*.rs`
  - Target: `neo-p2p/src/**/*.rs`
  - Re-exports: `neo-core/src/lib.rs`, `neo-core/src/network/mod.rs` (convert to re-export module)
  - Affected types: `MessageCommand`, `MessageFlags`, `VerifyResult`, `ContainsTransactionType`, `WitnessConditionType`, `WitnessRuleAction`, `NodeCapabilityType`, `LocalNode`, `RemoteNode`, `TaskManager`, all payload types (`VersionPayload`, `PingPayload`, `BlockPayload`, etc.)
  - Special attention: `neo-core/src/ledger/verify_result.rs` (move `VerifyResult` to neo-p2p)
- **Dependencies**: task-1, task-2 (network layer may depend on crypto and storage abstractions)
- **Test Command**:
  ```bash
  cargo test --package neo-p2p --lib --all-features -- --nocapture
  cargo test --package neo-core --lib --all-features -- network --nocapture
  cargo test --package neo-core --test p2p_message_tests -- --nocapture
  ```
- **Test Focus**:
  - Test message serialization/deserialization for all payload types
  - Validate handshake protocol flows
  - Test peer connection lifecycle (connect, ping, disconnect)
  - Verify capability negotiation
  - Test transaction relay and block relay mechanisms
  - Ensure backward compatibility: imports from `neo_core::network::*` resolve correctly
  - Verify neo-node and neo-cli network functionality

### Task 4: Remove Duplicate Primitives and Enums
- **ID**: task-4
- **Description**: Remove duplicate type definitions from neo-core that already exist in neo-primitives and specialized crates. Replace with imports. Specifically remove `Hardfork` from neo-core (keep in neo-primitives) and ensure all primitive enums are sourced from neo-primitives only.
- **File Scope**:
  - Remove: `neo-core/src/hardfork.rs` (duplicate of `neo-primitives/src/hardfork.rs`)
  - Update imports in: `neo-core/src/**/*.rs` (replace `crate::hardfork::Hardfork` with `neo_primitives::Hardfork`)
  - Verify: `neo-core/src/lib.rs` (add re-export: `pub use neo_primitives::Hardfork;`)
  - Affected enums: `Hardfork`, `WitnessScope`, `ContractParameterType`, `TransactionAttributeType`, `InventoryType`, `OracleResponseCode`
- **Dependencies**: task-1, task-2, task-3 (all other consolidations must be complete)
- **Test Command**:
  ```bash
  cargo test --package neo-primitives --lib --all-features -- --nocapture
  cargo test --package neo-core --lib --all-features -- --nocapture
  cargo build --workspace --all-features
  cargo clippy --workspace --all-features -- -D warnings
  ```
- **Test Focus**:
  - Verify no compilation errors across the entire workspace
  - Test that hardfork detection logic works (protocol version checks)
  - Validate that all enum variants are accessible from their correct locations
  - Ensure no dead code warnings for removed duplicates
  - Run full integration test suite to catch any missed import paths
  - Verify neo-cli and neo-node binaries build successfully

### Task 5: Update Dependencies and Documentation
- **ID**: task-5
- **Description**: Update `Cargo.toml` dependency declarations to reflect new crate boundaries, add backward-compatible re-exports in neo-core, update architecture documentation, and verify full workspace builds with no warnings.
- **File Scope**:
  - `neo-crypto/Cargo.toml` (may need additional dependencies for MPT, BloomFilter)
  - `neo-storage/Cargo.toml` (add rocksdb, ensure all provider deps are present)
  - `neo-p2p/Cargo.toml` (may need tokio, async-trait for network actors)
  - `neo-core/Cargo.toml` (update to depend on consolidated crates)
  - `neo-core/src/lib.rs` (add comprehensive re-export module for backward compatibility)
  - `docs/ARCHITECTURE.md` (update crate responsibility section)
  - `README.md` (update crate overview if needed)
- **Dependencies**: task-1, task-2, task-3, task-4 (all code migrations must be complete)
- **Test Command**:
  ```bash
  cargo build --workspace --all-features --release
  cargo test --workspace --all-features -- --nocapture
  cargo clippy --workspace --all-features -- -D warnings
  cargo doc --workspace --all-features --no-deps
  ```
- **Test Focus**:
  - Full workspace builds without errors or warnings
  - All tests pass (unit, integration, performance regression)
  - Documentation builds successfully
  - Verify binary sizes (should be similar or smaller due to deduplication)
  - Check that re-exports are correctly documented (rustdoc shows correct source)
  - Run smoke test: start neo-node, sync a few blocks, execute RPC queries

## Acceptance Criteria
- [ ] Zero duplicate type definitions across crates (verified by grep/ripgrep)
- [ ] neo-crypto contains ALL cryptographic operations (hash, ECC, MPT, BloomFilter)
- [ ] neo-storage contains ALL storage abstractions and providers (RocksDB, Memory, DataCache)
- [ ] neo-p2p contains ALL P2P protocol types and network logic
- [ ] neo-primitives is the single source for basic enums (Hardfork, WitnessScope, etc.)
- [ ] neo-core maintains backward compatibility via re-exports (no breaking changes)
- [ ] All unit tests pass: `cargo test --workspace --all-features`
- [ ] Code coverage ≥90% for modified modules
- [ ] No clippy warnings: `cargo clippy --workspace --all-features -- -D warnings`
- [ ] Full integration tests pass: `cargo test --package neo-core --test integration_tests`
- [ ] Performance regression tests show no degradation
- [ ] Documentation updated: ARCHITECTURE.md reflects new crate boundaries
- [ ] Binary builds successfully: neo-cli and neo-node compile and run

## Technical Notes

### Critical Constraints
1. **Zero Breaking Changes**: All public APIs in neo-core must remain accessible through re-exports. External crates (neo-cli, neo-node, neo-plugins) should require zero or minimal import path changes.
2. **Dependency Direction**: Foundation crates (neo-crypto, neo-storage, neo-p2p, neo-primitives) must NEVER depend on neo-core. Dependency flow is always upward (foundation → core).
3. **RocksDB Provider**: The `RocksDBStoreProvider` in neo-core/persistence is production-grade and performance-critical. Migration must preserve all optimizations and configurations.

### Re-export Strategy
- Create a compatibility module in `neo-core/src/lib.rs`:
  ```rust
  // Backward compatibility re-exports
  pub mod cryptography {
      pub use neo_crypto::*;
  }
  pub mod persistence {
      pub use neo_storage::*;
  }
  pub mod network {
      pub use neo_p2p::*;
  }
  ```

### Testing Strategy
- **Unit Tests**: Each specialized crate must have ≥90% coverage for moved code
- **Integration Tests**: Run full neo-core integration suite after each task
- **Compilation Test**: `cargo build --workspace --all-features` must succeed after each task
- **Performance Tests**: Run `neo-core/tests/performance_regression_tests.rs` to ensure no slowdowns

### Migration Sequence Rationale
1. **Crypto first** (task-1): Foundation layer, no dependencies on other modules
2. **Storage second** (task-2): May depend on crypto (hashing for keys), independent of network
3. **P2P third** (task-3): Depends on both crypto (signatures) and storage (blockchain state)
4. **Primitives cleanup** (task-4): Touches all modules, must come after structural changes
5. **Documentation last** (task-5): Documents the final state after all migrations

### Risk Mitigation
- **Import Path Breakage**: Use `cargo check --workspace` after each file move to catch import errors immediately
- **Test Failures**: Run `cargo test --workspace` after completing each task before moving to the next
- **Binary Size Regression**: Compare `target/release/neo-node` size before and after refactoring
- **Performance Regression**: Benchmark critical paths (block verification, transaction processing) before and after

### Rollback Plan
If a task fails integration tests:
1. Revert all changes for that task (use git)
2. Create a detailed failure report documenting the issue
3. Adjust the task plan to address the root cause
4. Retry the task with fixes applied
