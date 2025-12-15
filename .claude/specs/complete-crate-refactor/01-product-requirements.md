# Neo-RS Complete Crate Refactoring - Product Requirements Document

**Version:** 1.0
**Date:** 2025-12-14
**Status:** APPROVED
**Quality Score:** 94/100

---

## 1. Executive Summary

### 1.1 Vision Statement

Transform neo-rs from a monolithic architecture with neo-core as the "god crate" into a modular, well-structured workspace where:
- **neo-p2p** is a complete P2P networking crate (82 files from neo-core)
- **neo-storage** is a complete storage layer crate (26 files from neo-core)
- **neo-crypto** remains the complete cryptography crate (already done)

Each domain crate must have **ZERO dependency on neo-core** and be **independently publishable to crates.io**.

### 1.2 User-Approved Configuration

| Decision | Choice |
|----------|--------|
| **Version Strategy** | v0.8.0 breaking changes |
| **"Complete" Definition** | Zero neo-core dependency + independently publishable |
| **Circular Dep Strategy** | Trait abstraction + dependency injection |
| **Migration Support** | Full support (docs + examples + tools) |
| **Priority Order** | Complexity low → high |
| **Performance Threshold** | 0% regression (strict) |
| **Test Coverage** | 90%+ line coverage per crate |
| **Migration Tool** | Advanced: 95% automation coverage (+6 weeks) |
| **Release Strategy** | Two-phase: v0.8.0-alpha1 → v0.8.0 |

---

## 2. Problem Statement

### 2.1 Current State Analysis

**neo-core is a 500+ file monolithic crate containing:**
- P2P networking (82 files, ~6,788 LOC) - tightly coupled to ledger/smart_contract
- Persistence (26 files, ~4,053 LOC) - blocked by IInteroperable trait
- Smart contracts, VM integration, native contracts
- Actor runtime, system orchestration

**Existing "domain" crates are incomplete:**
- neo-crypto: ✅ Complete (14 files)
- neo-storage: ⚠️ Only 6 files (traits + basic types, missing StorageItem, DataCache)
- neo-p2p: ⚠️ Only 12 files (enums only, all payloads/actors in neo-core)

### 2.2 Three Critical Circular Dependencies

**Chain 1: Storage ↔ VM**
```
StorageItem → IInteroperable → StackItem → neo-vm
```

**Chain 2: Execution ↔ Networking**
```
Transaction/Block → ApplicationEngine → DataCache → NeoSystem → LocalNode
```

**Chain 3: P2P ↔ Ledger**
```
LocalNode → Blockchain → NeoSystem → PeerManagerService → LocalNode
```

---

## 3. Goals and Success Criteria

### 3.1 Primary Goals

1. **G-1:** neo-storage contains ALL storage functionality (0 dependencies on neo-core)
2. **G-2:** neo-p2p contains ALL P2P functionality (0 dependencies on neo-core)
3. **G-3:** All 3 circular dependency chains are broken via trait abstraction
4. **G-4:** Zero performance regression in hot paths
5. **G-5:** 90%+ test coverage per extracted crate
6. **G-6:** Advanced migration tool with 95% automation

### 3.2 Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| neo-core dependency count | 0 | `cargo tree -p neo-storage -i neo-core` returns empty |
| Performance regression | ≤0% | Benchmark suite comparison |
| Test coverage | ≥90% | `cargo tarpaulin` per crate |
| Migration tool coverage | ≥95% | AST transformation success rate |
| Build time increase | ≤10% | CI build time comparison |

---

## 4. Epics and User Stories

### Epic 1: Circular Dependency Resolution (E-1)

**Goal:** Break all 3 circular dependency chains using trait abstraction

#### E-1-US-1: Extract ISerializable Trait
**As a** library user
**I want** serialization traits in neo-primitives
**So that** all crates can implement serialization without circular deps

**Acceptance Criteria:**
- [ ] `ISerializable` trait defined in neo-primitives
- [ ] Supports `serialize()`, `deserialize()`, `size()`
- [ ] No external crate dependencies

**Technical Design:**
```rust
// neo-primitives/src/serialization.rs
pub trait ISerializable: Sized {
    fn serialize(&self) -> Result<Vec<u8>, SerializationError>;
    fn deserialize(data: &[u8]) -> Result<Self, SerializationError>;
    fn size(&self) -> usize;
}
```

#### E-1-US-2: Extract IStorageValue Trait
**As a** storage implementer
**I want** a storage value trait without VM dependencies
**So that** StorageItem can live in neo-storage

**Acceptance Criteria:**
- [ ] `IStorageValue` trait in neo-storage
- [ ] Breaks Chain 1 (StorageItem → IInteroperable)
- [ ] Supports byte serialization + size

#### E-1-US-3: Extract BlockchainProvider Trait
**As a** P2P implementer
**I want** blockchain access via trait bounds
**So that** LocalNode doesn't depend on concrete Blockchain

**Acceptance Criteria:**
- [ ] `BlockchainProvider` trait in neo-primitives
- [ ] Methods: `get_block()`, `get_header()`, `height()`, `relay_block()`
- [ ] Breaks Chain 3 (LocalNode → Blockchain)

#### E-1-US-4: Extract VerificationContext Trait
**As a** payload implementer
**I want** verification without ApplicationEngine dependency
**So that** Transaction/Block can live in neo-p2p

**Acceptance Criteria:**
- [ ] `IVerificationContext` trait in neo-primitives
- [ ] Breaks Chain 2 (Transaction → ApplicationEngine)
- [ ] Supports witness verification, gas calculation

#### E-1-US-5: Implement Dependency Injection Infrastructure
**As a** system architect
**I want** DI containers for service resolution
**So that** crates can be composed at runtime

**Acceptance Criteria:**
- [ ] `Arc<dyn Trait>` pattern for shared ownership
- [ ] Constructor injection for hot paths
- [ ] No performance overhead via monomorphization

---

### Epic 2: neo-storage Completion (E-2)

**Goal:** Make neo-storage a complete, independently publishable crate

#### E-2-US-1: Migrate StorageItem
**As a** storage user
**I want** StorageItem in neo-storage
**So that** I can use storage without neo-core

**Acceptance Criteria:**
- [ ] StorageItem supports `to_bytes()`, `from_bytes()`, `size()`
- [ ] Generic over cached type via `IStorageValue` trait
- [ ] 100% test coverage for serialization

#### E-2-US-2: Migrate DataCache
**As a** smart contract developer
**I want** DataCache in neo-storage
**So that** caching is decoupled from neo-core

**Acceptance Criteria:**
- [ ] DataCache with generic key/value types
- [ ] Supports tracking states (Added, Changed, Deleted)
- [ ] OnEntryDelegate abstracted via trait

#### E-2-US-3: Migrate StoreCache and ClonedCache
**As a** storage implementer
**I want** all cache types in neo-storage
**So that** cache hierarchy is complete

**Acceptance Criteria:**
- [ ] StoreCache wraps generic DataCache
- [ ] ClonedCache supports cache forking
- [ ] All tests pass with 90%+ coverage

#### E-2-US-4: Migrate Storage Providers
**As a** node operator
**I want** RocksDB and Memory providers in neo-storage
**So that** storage backends are pluggable

**Acceptance Criteria:**
- [ ] RocksDB provider with all features
- [ ] Memory provider for testing
- [ ] IStoreProvider trait for extension

#### E-2-US-5: Update All neo-core Imports
**As a** maintainer
**I want** all imports migrated to neo-storage
**So that** neo-core has zero storage definitions

**Acceptance Criteria:**
- [ ] Zero `pub struct` in neo-core/src/persistence/
- [ ] All imports use `use neo_storage::`
- [ ] Re-exports deprecated with warnings

---

### Epic 3: neo-p2p Completion (E-3)

**Goal:** Make neo-p2p a complete P2P networking crate

#### E-3-US-1: Migrate P2P Payloads
**As a** P2P implementer
**I want** all payloads in neo-p2p
**So that** message handling is self-contained

**Files to migrate:**
- Transaction (6 files)
- Block (1 file)
- Header (1 file)
- Witness, Signer (2 files)
- TransactionAttribute + variants (9 files)
- WitnessCondition + variants (9 files)
- ExtensiblePayload, OracleResponse (2 files)
- Remaining payloads (15 files)

**Acceptance Criteria:**
- [ ] All 45 payload files in neo-p2p/src/payloads/
- [ ] IVerifiable implemented via trait bounds
- [ ] Verification logic uses IVerificationContext

#### E-3-US-2: Migrate P2P Capabilities
**As a** P2P implementer
**I want** capability negotiation in neo-p2p
**So that** node handshake is self-contained

**Acceptance Criteria:**
- [ ] All 7 capability files migrated
- [ ] NodeCapability enum complete
- [ ] Version negotiation logic included

#### E-3-US-3: Migrate P2P Messages
**As a** P2P implementer
**I want** message framing in neo-p2p
**So that** protocol handling is complete

**Acceptance Criteria:**
- [ ] Message struct with command dispatch
- [ ] Binary serialization for all commands
- [ ] Compression support (LZ4)

#### E-3-US-4: Migrate LocalNode Actor
**As a** node operator
**I want** LocalNode in neo-p2p
**So that** P2P networking is independently usable

**Acceptance Criteria:**
- [ ] LocalNode generic over `<B: BlockchainProvider, P: PeerRegistry>`
- [ ] No direct neo-core dependencies
- [ ] Actor messages defined in neo-p2p

**Technical Design:**
```rust
pub struct LocalNode<B, P>
where
    B: BlockchainProvider,
    P: PeerRegistry,
{
    blockchain: Arc<B>,
    peers: Arc<P>,
    config: NetworkConfig,
}
```

#### E-3-US-5: Migrate RemoteNode Actor
**As a** P2P implementer
**I want** RemoteNode in neo-p2p
**So that** peer communication is complete

**Acceptance Criteria:**
- [ ] RemoteNode with handshake, inventory, message handlers
- [ ] Uses BlockchainProvider trait for queries
- [ ] Timer management included

#### E-3-US-6: Migrate TaskManager
**As a** P2P implementer
**I want** TaskManager in neo-p2p
**So that** sync coordination is complete

**Acceptance Criteria:**
- [ ] Task scheduling for block/transaction sync
- [ ] Timeout management
- [ ] Uses BlockchainProvider trait

---

### Epic 4: Migration Tooling (E-4)

**Goal:** Provide advanced AST-based migration tool with 95% automation

#### E-4-US-1: Build AST Parser
**As a** downstream user
**I want** automated import migration
**So that** I can upgrade without manual changes

**Acceptance Criteria:**
- [ ] Uses `syn`, `quote`, `proc-macro2`
- [ ] Parses all Rust files in project
- [ ] Identifies neo-core imports to migrate

#### E-4-US-2: Build Import Transformer
**As a** downstream user
**I want** automatic import path updates
**So that** migration is painless

**Acceptance Criteria:**
- [ ] Maps old paths to new paths
- [ ] Handles wildcard imports
- [ ] Preserves formatting

**Transformation Rules:**
```
neo_core::persistence::* → neo_storage::*
neo_core::network::p2p::payloads::* → neo_p2p::payloads::*
neo_core::network::p2p::LocalNode → neo_p2p::actors::LocalNode
neo_core::cryptography::* → neo_crypto::*
```

#### E-4-US-3: Build Type Migration Assistant
**As a** downstream user
**I want** trait bound suggestions
**So that** I know what changes are needed

**Acceptance Criteria:**
- [ ] Identifies broken trait bounds
- [ ] Suggests generic type parameters
- [ ] Generates migration report

#### E-4-US-4: Build Migration Validator
**As a** downstream user
**I want** validation of migration completeness
**So that** I know all changes are correct

**Acceptance Criteria:**
- [ ] Runs `cargo check` after transformation
- [ ] Reports remaining errors
- [ ] Suggests fixes for common issues

---

### Epic 5: Release Management (E-5)

**Goal:** Two-phase release with validation

#### E-5-US-1: Release v0.8.0-alpha1
**As a** early adopter
**I want** alpha release for testing
**So that** I can validate migration approach

**Acceptance Criteria:**
- [ ] neo-storage fully extracted
- [ ] neo-p2p payloads extracted
- [ ] Migration tool available
- [ ] Breaking changes documented

#### E-5-US-2: Release v0.8.0
**As a** production user
**I want** stable release
**So that** I can upgrade with confidence

**Acceptance Criteria:**
- [ ] All epics complete
- [ ] 90%+ test coverage
- [ ] Performance validated
- [ ] Migration guide complete

#### E-5-US-3: Deprecation Warnings
**As a** maintainer
**I want** deprecation warnings for old paths
**So that** users know to migrate

**Acceptance Criteria:**
- [ ] All re-exports marked `#[deprecated]`
- [ ] Warning message includes new path
- [ ] 2 release grace period

#### E-5-US-4: Migration Documentation
**As a** downstream user
**I want** comprehensive migration guide
**So that** I can upgrade successfully

**Acceptance Criteria:**
- [ ] MIGRATION.md with step-by-step guide
- [ ] Before/after code examples
- [ ] Troubleshooting section
- [ ] FAQ for common issues

---

## 5. Technical Specifications

### 5.1 Trait Definitions for Dependency Breaking

#### Chain 1 Resolution: IStorageValue
```rust
// neo-storage/src/traits.rs
pub trait IStorageValue: Clone + Send + Sync + 'static {
    fn to_bytes(&self) -> Vec<u8>;
    fn from_bytes(data: &[u8]) -> Result<Self, StorageError>;
    fn size(&self) -> usize;
}

// neo-core implementation
impl IStorageValue for StorageItem {
    fn to_bytes(&self) -> Vec<u8> { ... }
    fn from_bytes(data: &[u8]) -> Result<Self, StorageError> { ... }
    fn size(&self) -> usize { self.value.len() }
}
```

#### Chain 2 Resolution: IVerificationContext
```rust
// neo-primitives/src/verification.rs
pub trait IVerificationContext: Send + Sync {
    fn verify_witness(
        &self,
        hash: &UInt160,
        witness: &dyn IWitness,
    ) -> Result<bool, VerificationError>;

    fn get_gas_consumed(&self) -> i64;
    fn get_max_gas(&self) -> i64;
}
```

#### Chain 3 Resolution: BlockchainProvider
```rust
// neo-primitives/src/blockchain.rs
pub trait BlockchainProvider: Send + Sync + 'static {
    type Block: IBlock;
    type Header: IHeader;

    fn height(&self) -> u32;
    fn get_block(&self, height: u32) -> Option<Self::Block>;
    fn get_header(&self, hash: &UInt256) -> Option<Self::Header>;
    fn relay_block(&self, block: Self::Block) -> Result<(), RelayError>;
}

pub trait PeerRegistry: Send + Sync + 'static {
    fn connected_count(&self) -> usize;
    fn broadcast(&self, message: &dyn IMessage);
    fn get_peers(&self) -> Vec<PeerInfo>;
}
```

### 5.2 Generic Type Strategy

**Hot paths (monomorphization for zero-cost):**
```rust
pub struct DataCache<V: IStorageValue> {
    dictionary: HashMap<StorageKey, Trackable<V>>,
}

impl<V: IStorageValue> DataCache<V> {
    #[inline]
    pub fn try_get(&self, key: &StorageKey) -> Option<&V> { ... }
}
```

**Cold paths (dynamic dispatch acceptable):**
```rust
pub struct LocalNode {
    blockchain: Arc<dyn BlockchainProvider<Block = Block, Header = Header>>,
    peers: Arc<dyn PeerRegistry>,
}
```

### 5.3 Performance Requirements

| Operation | Max Latency | Benchmark |
|-----------|-------------|-----------|
| StorageKey hash | 10ns | `bench_storage_key_hash` |
| DataCache lookup | 50ns | `bench_cache_lookup` |
| Block serialization | 100μs | `bench_block_serialize` |
| Transaction verification | 1ms | `bench_tx_verify` |
| LocalNode message dispatch | 10μs | `bench_message_dispatch` |
| RemoteNode handshake | 50ms | `bench_handshake` |
| Inventory broadcast | 5ms | `bench_broadcast` |
| Full block sync | 100ms | `bench_block_sync` |

---

## 6. Timeline

### Phase 1: Foundation (Weeks 1-2)
- [ ] Define all abstraction traits
- [ ] Create neo-primitives trait modules
- [ ] Set up benchmark infrastructure

### Phase 2: neo-storage Completion (Weeks 3-6)
- [ ] Migrate StorageItem with IStorageValue
- [ ] Migrate DataCache with generics
- [ ] Migrate storage providers
- [ ] Update all neo-core imports
- [ ] Achieve 90%+ test coverage

### Phase 3: neo-p2p Payloads (Weeks 7-10)
- [ ] Migrate all 45 payload files
- [ ] Implement IVerificationContext pattern
- [ ] Migrate capability negotiation
- [ ] Migrate message framing

### Phase 4: neo-p2p Actors (Weeks 11-14)
- [ ] Migrate LocalNode with trait bounds
- [ ] Migrate RemoteNode
- [ ] Migrate TaskManager
- [ ] Integration testing

### Phase 5: Tooling & Release (Weeks 15-18)
- [ ] Build migration tool
- [ ] Release v0.8.0-alpha1
- [ ] Gather feedback
- [ ] Release v0.8.0

---

## 7. Risks and Mitigations

| Risk | Severity | Likelihood | Mitigation |
|------|----------|------------|------------|
| Circular deps cannot be broken | HIGH | LOW | Trait abstraction + type parameterization proven effective |
| Performance regression | HIGH | MEDIUM | Benchmark-driven development, monomorphization for hot paths |
| Actor runtime breaks | MEDIUM | LOW | Keep akka internal, parameterize over message types |
| Community resistance | LOW | MEDIUM | Clear communication, migration tool, deprecation grace period |
| Timeline slip | MEDIUM | MEDIUM | Parallel tracks, incremental releases |

---

## 8. Out of Scope

The following are explicitly NOT part of this refactoring:

1. **Actor runtime extraction** - neo-akka remains internal
2. **Smart contract extraction** - ApplicationEngine stays in neo-core
3. **Consensus extraction** - Consensus logic stays in neo-consensus
4. **RPC API changes** - neo-rpc interface unchanged
5. **Protocol changes** - Network protocol unchanged

---

## 9. Stakeholder Sign-off

| Role | Status | Date |
|------|--------|------|
| Product Owner | ✅ APPROVED | 2025-12-14 |
| Technical Lead | PENDING | - |
| QA Lead | PENDING | - |

---

## 10. Appendix: File Migration Checklist

### neo-storage (26 files)
- [ ] persistence/storage_item.rs
- [ ] persistence/data_cache.rs
- [ ] persistence/store_cache.rs
- [ ] persistence/cloned_cache.rs
- [ ] persistence/providers/rocksdb_store_provider.rs
- [ ] persistence/providers/memory_store.rs
- [ ] persistence/providers/memory_snapshot.rs
- [ ] persistence/trackable.rs
- [ ] ... (18 more files)

### neo-p2p (82 files)
- [ ] network/p2p/payloads/transaction/*.rs (6 files)
- [ ] network/p2p/payloads/block.rs
- [ ] network/p2p/payloads/header.rs
- [ ] network/p2p/payloads/witness.rs
- [ ] network/p2p/payloads/signer.rs
- [ ] network/p2p/payloads/conditions/*.rs (8 files)
- [ ] network/p2p/local_node/*.rs (7 files)
- [ ] network/p2p/remote_node/*.rs (5 files)
- [ ] network/p2p/capabilities/*.rs (7 files)
- [ ] network/p2p/messages/*.rs (5 files)
- [ ] ... (remaining files)

---

**Document End**

**Generated by:** BMAD Product Owner Agent
**Quality Score:** 94/100
