# Development Plan: Neo-RS Complete Crate Refactoring

## Overview

Transform neo-rs from a monolithic architecture into a modular system by extracting 108 files from neo-core into independently publishable crates (neo-storage and neo-p2p). This plan breaks 3 critical circular dependency chains using trait abstraction and generic type strategies, achieving zero performance regression while maintaining 90%+ test coverage.

---

## Phase 1: Foundation (Weeks 1-2)

### Task 1.1: Define IStorageValue Trait

- **ID**: P1-T1
- **Description**: Create the `IStorageValue` trait in neo-primitives to break Chain 1 (StorageItem → IInteroperable circular dependency). This trait abstracts storage value operations without requiring VM types, enabling StorageItem to live in neo-storage.
- **File Scope**:
  - CREATE: `neo-primitives/src/storage.rs`
  - MODIFY: `neo-primitives/src/lib.rs` (add public re-export)
- **Dependencies**: None
- **Test Command**: `cargo test -p neo-primitives --lib storage --all-features`
- **Test Focus**:
  - Default implementation for `Vec<u8>` works correctly (to_bytes, from_bytes, size)
  - Trait methods serialize and deserialize data without loss
  - Size calculation matches serialized byte length
  - Custom implementations can be created (test with a mock struct)

**Acceptance Criteria:**
- [ ] `IStorageValue` trait defined with `to_bytes()`, `from_bytes()`, `size()`
- [ ] Default impl for `Vec<u8>` passes all serialization tests
- [ ] Trait documentation includes usage examples
- [ ] Test coverage ≥90%

---

### Task 1.2: Define IVerificationContext Trait

- **ID**: P1-T2
- **Description**: Create the `IVerificationContext` and `IWitness` traits in neo-primitives to break Chain 2 (Transaction → ApplicationEngine circular dependency). These traits allow transactions to verify witnesses without depending on concrete ApplicationEngine implementation.
- **File Scope**:
  - CREATE: `neo-primitives/src/verification.rs`
  - MODIFY: `neo-primitives/src/lib.rs` (add public re-export)
- **Dependencies**: None (can run parallel with P1-T1)
- **Test Command**: `cargo test -p neo-primitives --lib verification --all-features`
- **Test Focus**:
  - Mock verifier implementation passes basic verification flow
  - Gas consumption tracking works correctly
  - `should_abort()` returns true when gas limit exceeded
  - Error types (VerificationError) cover all failure modes
  - IWitness trait can be implemented by simple structs

**Acceptance Criteria:**
- [ ] `IVerificationContext` trait with `verify_witness()`, `get_gas_consumed()`, `get_max_gas()`
- [ ] `IWitness` trait with `invocation_script()`, `verification_script()`
- [ ] `VerificationError` enum with all failure cases
- [ ] Mock implementations pass unit tests
- [ ] Test coverage ≥90%

---

### Task 1.3: Define BlockchainProvider and PeerRegistry Traits

- **ID**: P1-T3
- **Description**: Create the `BlockchainProvider` and `PeerRegistry` traits in neo-primitives to break Chain 3 (LocalNode → Blockchain circular dependency). These traits enable P2P actors to interact with blockchain and peers via abstract interfaces.
- **File Scope**:
  - CREATE: `neo-primitives/src/blockchain.rs`
  - CREATE: `neo-primitives/src/error.rs` (RelayError, SendError)
  - MODIFY: `neo-primitives/src/lib.rs` (add public re-exports)
- **Dependencies**: None (can run parallel with P1-T1, P1-T2)
- **Test Command**: `cargo test -p neo-primitives --lib blockchain --all-features`
- **Test Focus**:
  - Mock blockchain provider implements all required methods
  - Mock peer registry tracks broadcast calls correctly
  - Associated types (Block, Header) can be parameterized
  - Error types (RelayError, SendError) cover all failure cases
  - PeerInfo serialization/deserialization works

**Acceptance Criteria:**
- [ ] `BlockchainProvider` trait with `height()`, `get_block()`, `relay_block()`, etc.
- [ ] `PeerRegistry` trait with `broadcast()`, `send_to()`, `get_peers()`, etc.
- [ ] `IMessage`, `IBlock`, `IHeader` marker traits defined
- [ ] `RelayError` and `SendError` enums defined
- [ ] Mock implementations in tests demonstrate trait usage
- [ ] Test coverage ≥90%

---

### Task 1.4: Define IBlockchainSnapshot Trait

- **ID**: P1-T4
- **Description**: Create the `IBlockchainSnapshot` trait in neo-primitives to provide read-only blockchain state access during verification. This trait is used by payloads (Transaction, Block) to query storage without depending on DataCache.
- **File Scope**:
  - MODIFY: `neo-primitives/src/verification.rs` (add IBlockchainSnapshot trait)
  - CREATE: `neo-primitives/tests/snapshot_tests.rs`
- **Dependencies**: P1-T2 (extends verification module)
- **Test Command**: `cargo test -p neo-primitives --test snapshot_tests --all-features`
- **Test Focus**:
  - Mock snapshot can store and retrieve arbitrary key-value pairs
  - `height()` returns correct blockchain height
  - `contains_transaction()` correctly checks transaction existence
  - Mock implementation works with verification context

**Acceptance Criteria:**
- [ ] `IBlockchainSnapshot` trait with `height()`, `get_storage()`, `contains_transaction()`
- [ ] Trait documentation explains usage in verification flow
- [ ] Mock snapshot implementation in tests
- [ ] Test coverage ≥90%

---

### Task 1.5: Set Up Benchmark Infrastructure

- **ID**: P1-T5
- **Description**: Establish baseline benchmarks for hot paths (cache operations, serialization) to validate zero performance regression throughout the refactoring. Configure CI to run benchmarks on every PR.
- **File Scope**:
  - CREATE: `neo-storage/benches/cache_baseline.rs`
  - CREATE: `neo-p2p/benches/serialization_baseline.rs`
  - CREATE: `.github/workflows/benchmark.yml`
  - CREATE: `scripts/capture_baseline.sh`
- **Dependencies**: None (can run parallel with all P1 tasks)
- **Test Command**: `cargo bench --package neo-storage --package neo-p2p -- --save-baseline before-refactor`
- **Test Focus**:
  - DataCache::get() operation baseline captured
  - DataCache::add() operation baseline captured
  - Block::serialize() operation baseline captured
  - Transaction::deserialize() operation baseline captured
  - Baseline results saved to `target/criterion/before-refactor/`

**Acceptance Criteria:**
- [ ] Benchmark suite runs successfully for neo-storage and neo-p2p
- [ ] Baseline results captured and committed to repository
- [ ] CI workflow configured to run benchmarks on PRs
- [ ] Performance regression detection threshold set to 0%
- [ ] Documentation explains how to run benchmarks locally

---

## Phase 2: neo-storage Completion (Weeks 3-6)

**Status**: TODO - will be detailed after Phase 1 completion

**High-level tasks:**
- Migrate StorageItem with generic `StorageItem<V: IStorageValue>`
- Migrate DataCache with generic `DataCache<K, V>`
- Migrate StoreCache and ClonedCache
- Migrate storage providers (RocksDB, Memory)
- Update all neo-core imports to `use neo_storage::`

---

## Phase 3: neo-p2p Payloads (Weeks 7-10)

**Status**: TODO - will be detailed after Phase 2 completion

**High-level tasks:**
- Migrate Transaction (6 files) with IVerificationContext pattern
- Migrate Block, Header, Witness, Signer (5 files)
- Migrate TransactionAttribute + variants (9 files)
- Migrate WitnessCondition + variants (9 files)
- Migrate remaining payloads (16 files)

---

## Phase 4: neo-p2p Actors (Weeks 11-14)

**Status**: TODO - will be detailed after Phase 3 completion

**High-level tasks:**
- Migrate LocalNode with `<B: BlockchainProvider, P: PeerRegistry>`
- Migrate RemoteNode with trait bounds
- Migrate TaskManager with trait bounds
- Migrate capabilities and messages (12 files)

---

## Phase 5: Migration Tooling (Weeks 15-16)

**Status**: TODO - will be detailed after Phase 4 completion

**High-level tasks:**
- Build AST parser for import transformation
- Build migration validator
- Test migration tool on neo-plugins
- Write migration guide (MIGRATION.md)

---

## Phase 6: Release (Weeks 17-18)

**Status**: TODO - will be detailed after Phase 5 completion

**High-level tasks:**
- Release v0.8.0-alpha1 for early testing
- Gather feedback and fix issues
- Release v0.8.0 stable
- Publish all crates to crates.io

---

## Parallelization Matrix (Phase 1)

| Task ID | Can Run With | Blocks |
|---------|--------------|--------|
| P1-T1   | P1-T2, P1-T3, P1-T5 | P2-T1 (StorageItem migration) |
| P1-T2   | P1-T1, P1-T3, P1-T5 | P1-T4, P3-T1 (Transaction migration) |
| P1-T3   | P1-T1, P1-T2, P1-T5 | P4-T1 (LocalNode migration) |
| P1-T4   | P1-T1, P1-T3, P1-T5 | P3-T1 (Transaction migration) |
| P1-T5   | All tasks (independent) | Phase 2+ (requires baseline) |

**Maximum parallelism**: 4 tasks can run simultaneously (P1-T1, P1-T2, P1-T3, P1-T5).

---

## Test Commands Summary

### Phase 1 Test Commands

```bash
# Test all Phase 1 tasks
cargo test -p neo-primitives --all-features

# Test individual tasks
cargo test -p neo-primitives --lib storage          # P1-T1
cargo test -p neo-primitives --lib verification     # P1-T2
cargo test -p neo-primitives --lib blockchain       # P1-T3
cargo test -p neo-primitives --test snapshot_tests  # P1-T4

# Capture baseline benchmarks (P1-T5)
cargo bench --package neo-storage --package neo-p2p -- --save-baseline before-refactor

# Coverage check (must reach 90%)
cargo tarpaulin -p neo-primitives --out Html --output-dir target/coverage
```

### Dependency Validation Commands

```bash
# Verify no circular dependencies (run after each phase)
cargo tree -p neo-primitives -i neo-core    # Should return empty
cargo tree -p neo-storage -i neo-core       # Should return empty (Phase 2+)
cargo tree -p neo-p2p -i neo-core          # Should return empty (Phase 3+)

# Verify compilation with new traits
cargo check --package neo-primitives --all-features
cargo build --package neo-primitives --all-features
```

---

## Acceptance Criteria (Phase 1 Complete)

- [ ] All 5 Phase 1 tasks completed successfully
- [ ] neo-primitives compiles with zero errors
- [ ] Test coverage for neo-primitives ≥90%
- [ ] Benchmark baselines captured and saved
- [ ] CI pipeline configured for benchmarks
- [ ] All trait documentation includes usage examples
- [ ] Mock implementations demonstrate trait usage
- [ ] Zero dependencies on neo-core in neo-primitives

---

## Technical Notes

### Key Architectural Decisions

1. **Trait Abstraction Strategy**: Use traits in neo-primitives to define contracts between layers, enabling dependency inversion.

2. **Generic Type Strategy**:
   - Hot paths (DataCache, StorageItem): Use generic types for monomorphization (zero-cost abstraction)
   - Cold paths (LocalNode, RemoteNode): Use trait objects (`dyn Trait`) for flexibility

3. **Dependency Injection**: Constructor injection with `Arc<dyn Trait>` for service composition in actors.

4. **Error Handling**: Each crate defines its own error types; trait errors live in neo-primitives.

### Performance Constraints

- Cache operations (DataCache::get/add): Must remain at 10-20ns (hot path)
- Block relay (LocalNode::relay_block): 2ns trait dispatch overhead acceptable (cold path, 0.001% of 100ms operation)
- Transaction verification: 1-5ms total (trait dispatch overhead <0.001%)

### Test Coverage Requirements

- **Minimum threshold**: 90% line coverage per crate
- **Measurement**: `cargo tarpaulin -p <crate> --out Html`
- **Enforcement**: CI blocks PRs below 90% coverage

### Benchmark Requirements

- **Baseline capture**: Run before any refactoring begins
- **Regression threshold**: 0% (any regression requires justification)
- **Measurement**: `cargo bench -- --save-baseline <name>`
- **Comparison**: `cargo bench -- --baseline <name>`

---

## Risk Mitigation

### High-Priority Risks

1. **Risk**: Trait dispatch overhead causes performance regression
   - **Mitigation**: Use monomorphization for hot paths, benchmark every change
   - **Detection**: Automated benchmark CI on every PR

2. **Risk**: Generic type complexity confuses downstream users
   - **Mitigation**: Provide type aliases (e.g., `BytesStorageItem = StorageItem<Vec<u8>>`), comprehensive documentation
   - **Detection**: External developer review before v0.8.0 release

3. **Risk**: Test coverage drops below 90%
   - **Mitigation**: Enforce coverage checks in CI, block PRs below threshold
   - **Detection**: `cargo tarpaulin` runs on every commit

### Phase 1 Specific Risks

- **Risk**: Trait definitions incomplete or incorrect
  - **Mitigation**: Review traits with 2+ senior engineers, create mock implementations in tests
  - **Detection**: Integration tests in Phase 2 will validate trait contracts

- **Risk**: Benchmark infrastructure doesn't capture accurate baselines
  - **Mitigation**: Run benchmarks on dedicated CI machines, average 10+ runs, disable CPU frequency scaling
  - **Detection**: Manual validation of benchmark results

---

## Next Steps After Phase 1

1. **Technical Review**: Present Phase 1 results to team, validate trait designs
2. **Phase 2 Planning**: Detail neo-storage migration tasks based on Phase 1 traits
3. **Integration Testing**: Verify traits work with existing neo-core code (create adapter implementations)
4. **Documentation**: Update ARCHITECTURE.md with trait diagrams and usage examples

---

**Document Version**: 1.0
**Generated Date**: 2025-12-14
**Status**: READY FOR EXECUTION
**Estimated Duration**: 2 weeks (Phase 1 only)
**Team Size**: 1-2 senior Rust developers

**Success Criteria Summary**:
- ✅ All traits defined in neo-primitives
- ✅ Zero dependencies on neo-core
- ✅ 90%+ test coverage
- ✅ Benchmark baselines captured
- ✅ CI pipeline operational
