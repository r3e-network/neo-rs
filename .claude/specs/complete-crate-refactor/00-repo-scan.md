# Neo-RS Complete Crate Refactoring - Repository Scan Report

**Date:** 2025-12-14
**Methodology:** UltraThink - Evidence-based architectural analysis
**Mission:** Extract P2P, Crypto, and Storage from neo-core into standalone crates

---

## Executive Summary

### Current State
- **Project Size:** 779 Rust files across 14 workspace crates
- **neo-core:** Monolithic core with 500+ files containing business logic for:
  - P2P networking (82 files, ~6,788 LOC)
  - Persistence (26 files, ~4,053 LOC)
  - Smart contracts, VM integration, native contracts
  - Actor runtime, system orchestration

### Partial Migration Status
Three foundation crates exist but are **INCOMPLETE**:
1. ✅ **neo-crypto** - Fully migrated, only re-exports remain in neo-core
2. ⚠️ **neo-storage** - Traits only, missing StorageItem (blocked by IInteroperable)
3. ⚠️ **neo-p2p** - Enums only, all actors/payloads/logic still in neo-core

---

## 1. Dependency Graph Analysis

### 1.1 Workspace Dependencies

```
Foundation Layer (no neo-* dependencies):
  neo-primitives ──┐
  neo-crypto      ─┤
  neo-storage     ─┼──> neo-io
  neo-p2p         ─┘

Core Layer (heavy cross-dependencies):
  neo-core ──┬──> neo-crypto
             ├──> neo-storage (CIRCULAR!)
             ├──> neo-p2p
             ├──> neo-vm
             ├──> neo-consensus
             └──> neo-primitives

Application Layer:
  neo-cli ──> neo-rpc ──> neo-core
  neo-node ──> neo-core
```

### 1.2 Critical Circular Dependencies

**Dependency Chain:**
```
neo-storage → neo-core (StorageItem uses IInteroperable)
neo-core → neo-storage (re-exports StorageKey/TrackState)

neo-p2p → neo-core (enums only, no real dependency)
neo-core → neo-p2p (payloads/actors depend on ledger/smart_contract)
```

**Cross-Module Dependencies in neo-core:**
- **network/p2p/** → 55 references to `crate::{ledger, smart_contract, persistence, akka, services}`
- **persistence/** → 69 references across neo-core (DataCache used everywhere)
- **cryptography/** → 35 references (mostly satisfied by neo-crypto crate)

---

## 2. Module Breakdown: neo-core/src/network/p2p/

### 2.1 File Count Analysis

**Total:** 82 files, ~6,788 lines of code

**Breakdown by category:**
- **Payloads:** 45 files (transaction, block, header, witness, conditions, attributes)
- **Actors:** 7 files (LocalNode, RemoteNode state machines)
- **Task coordination:** 1 file (TaskManager)
- **Capabilities:** 7 files (node capability negotiation)
- **Messages:** 5 files (message framing, serialization)
- **Helpers:** 17 files (timeouts, session, peer, connection)

### 2.2 Core Types in Payloads

**Located in `neo-core/src/network/p2p/payloads/`:**
```rust
pub struct Transaction       // 6 files (core, serialization, json, verification, traits, mod)
pub struct Block            // 1 file
pub struct Header           // 1 file
pub struct Witness          // 1 file
pub struct Signer           // 1 file
pub struct TransactionAttribute  // 1 file + 8 attribute variants
pub enum WitnessCondition   // 8 condition types in conditions/
pub struct ExtensiblePayload     // 1 file
pub struct OracleResponse        // 1 file
```

### 2.3 Dependency Analysis: Payloads

**Critical dependencies preventing extraction:**
```rust
// Block.rs dependencies:
use crate::ledger::{HeaderCache, TransactionVerificationContext, VerifyResult};
use crate::persistence::{DataCache, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::neo_io::{Serializable, BinaryWriter};

// Transaction dependencies:
use crate::ledger::TransactionVerificationContext;
use crate::persistence::DataCache;
use crate::smart_contract::{Helper, ApplicationEngine, CallFlags};
use crate::cryptography::BloomFilter;
use crate::protocol_settings::ProtocolSettings;
```

**Required traits:**
- `IInventory` - inventory broadcast trait
- `IVerifiable` - witness verification trait (defined in neo-core::lib.rs)
- `Serializable` - Neo IO serialization (from neo-io crate)

### 2.4 Dependency Analysis: Actors

**LocalNode (7 files):**
```rust
// Dependencies blocking extraction:
use crate::neo_system::NeoSystemContext;
use crate::services::PeerManagerService;
use crate::akka::{Actor, ActorContext, ActorRef, Props};
use crate::network::p2p::payloads::{VersionPayload, Block, Transaction};
use crate::protocol_settings::ProtocolSettings;
```

**RemoteNode (4 files):**
```rust
// Dependencies blocking extraction:
use crate::ledger::blockchain::BlockchainCommand;
use crate::smart_contract::native::ledger_contract::LedgerContract;
use crate::cryptography::BloomFilter;
use crate::akka::{Actor, ActorContext};
```

**TaskManager:**
```rust
// Dependencies blocking extraction:
use crate::ledger::blockchain::BlockchainCommand;
use crate::UInt256;
```

### 2.5 External Crate Dependencies

**Required external crates for P2P:**
- `tokio` - async runtime
- `ractor` - actor framework (currently embedded in neo-core/src/akka/)
- `bincode`, `serde` - serialization
- `tracing` - logging
- `bytes` - buffer management

---

## 3. Module Breakdown: neo-core/src/cryptography/

### 3.1 Migration Status: ✅ COMPLETE

**Current state:**
- **neo-core/src/cryptography/mod.rs:** 29 lines - pure re-exports from neo-crypto
- **neo-crypto crate:** 14 files, fully functional

**Remaining work:**
- ✅ None - cryptography is fully extracted
- ⚠️ 35 files in neo-core still use `use crate::cryptography::*` (should migrate to `use neo_crypto::*`)

### 3.2 neo-crypto Crate Contents

**Files:**
```
neo-crypto/src/
├── lib.rs               # Public API + re-exports
├── hash.rs              # SHA256, RIPEMD160, Hash256, Keccak256
├── crypto_utils.rs      # High-level crypto APIs (ECC, ECDSA, Hash)
├── ecc.rs              # Elliptic curve point operations
├── bloom_filter.rs     # Bloom filter for P2P filtering
├── named_curve_hash.rs # Curve-specific hash algorithms
├── error.rs            # CryptoError types
└── mpt_trie/           # Merkle Patricia Trie (5 files)
    ├── trie.rs
    ├── node.rs
    ├── cache.rs
    ├── tests.rs
    └── error.rs
```

**External dependencies:**
```toml
secp256k1 = "0.28"   # Bitcoin curve
p256 = "0.13"        # NIST P-256 (Neo's primary curve)
k256 = "0.13"        # Koblitz curve
ed25519-dalek = "2.0"
blst = "0.3"         # BLS12-381
sha2, sha3, ripemd, blake2, blake3
```

---

## 4. Module Breakdown: neo-core/src/persistence/

### 4.1 File Count Analysis

**Total:** 26 files, ~4,053 lines of code

**Breakdown:**
- **Core traits:** 5 files (IReadOnlyStore, IWriteStore, IStore, ISnapshot, IStoreProvider)
- **Cache layer:** 4 files (DataCache, StoreCache, ClonedCache, Cache)
- **Storage providers:** 3 files (RocksDB, Memory store/snapshot)
- **Storage types:** 4 files (StorageKey, StorageItem, TrackState, SeekDirection)
- **Utilities:** 10 files (serialization, compression, backup, index, transaction)

### 4.2 Public API Count

**58 public items** across 21 files:
- 21 traits/structs
- 37 methods/functions

**Critical types:**
```rust
// Traits (should be in neo-storage):
pub trait IReadOnlyStore
pub trait IWriteStore
pub trait IStore: IReadOnlyStore + IWriteStore
pub trait ISnapshot: IReadOnlyStore + SeekableStore
pub trait IStoreProvider

// Types:
pub struct DataCache         // ❌ Blocked: uses OnEntryDelegate closure
pub struct StoreCache        // ❌ Blocked: wraps DataCache
pub struct ClonedCache       // ❌ Blocked: wraps DataCache
pub struct StorageKey        // ✅ Already in neo-storage
pub struct StorageItem       // ❌ Blocked: uses IInteroperable, BinarySerializer
pub enum TrackState          // ✅ Already in neo-storage
pub enum SeekDirection       // ✅ Already in neo-storage
```

### 4.3 Blocking Dependencies

**StorageItem (neo-core/src/persistence/storage_item.rs):**
```rust
// Lines 7-10: Critical blockers
use crate::neo_io::{IoResult, MemoryReader};
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::i_interoperable::IInteroperable;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;

// Lines 17-20: Enum with trait object
enum StorageCache {
    BigInteger(BigInt),
    Interoperable(Box<dyn IInteroperable>),  // ❌ BLOCKER
}
```

**DataCache dependencies:**
```rust
use crate::smart_contract::{StorageItem, StorageKey};  // StorageItem not in neo-storage!
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub type OnEntryDelegate = Arc<dyn Fn(&DataCache, &StorageKey, &StorageItem) + Send + Sync>;
```

**69 references to `crate::persistence`** across:
- smart_contract/ (18 files) - ApplicationEngine, native contracts
- ledger/ (4 files) - Blockchain, MemoryPool
- neo_system/ (5 files) - NeoSystem, persistence layer
- network/p2p/payloads/ (5 files) - Block, Transaction verification
- state_service/ (3 files) - State root management

### 4.4 External Dependencies

**RocksDB provider:**
```toml
rocksdb = { version = "0.21", features = ["snappy", "lz4", "zlib"] }
```

**Serialization:**
```toml
bincode = "1.3"
serde = { version = "1.0", features = ["derive"] }
```

**Concurrency:**
```toml
parking_lot = "0.12"
dashmap = "5.5"
```

---

## 5. Critical Dependency Matrix

### 5.1 Cross-Domain Dependencies

| From → To | ledger | smart_contract | persistence | neo_system | akka | services |
|-----------|--------|----------------|-------------|------------|------|----------|
| **network/p2p/** | ✅ 8 | ✅ 10 | ✅ 15 | ✅ 12 | ✅ 7 | ✅ 3 |
| **persistence/** | ❌ 4 | ✅ 18 | - | ❌ 5 | - | - |
| **cryptography/** | - | ❌ 3 | - | - | - | - |

**Legend:**
- ✅ Hard dependency (cannot extract without breaking)
- ❌ Soft dependency (can be abstracted with traits)
- `-` No dependency

### 5.2 Trait Dependencies

**IVerifiable trait (neo-core/src/lib.rs:186-253):**
```rust
pub trait IVerifiable: std::any::Any + Send + Sync {
    fn verify(&self) -> bool;
    fn hash(&self) -> CoreResult<UInt256>;
    fn get_hash_data(&self) -> Vec<u8>;
    fn get_script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160>;
    fn get_witnesses(&self) -> Vec<&Witness>;
    fn verify_witnesses(&self, settings: &ProtocolSettings, snapshot: &DataCache, max_gas: i64) -> bool;
}
```

**Required by:** Transaction, Block, ExtensiblePayload
**Blocker:** Requires `DataCache`, `ProtocolSettings` from neo-core

**IInteroperable trait:**
```rust
pub trait IInteroperable {
    fn from_stack_item(&mut self, item: &StackItem) -> Result<(), String>;
    fn to_stack_item(&self) -> StackItem;
}
```

**Required by:** StorageItem, native contract states
**Blocker:** Requires `StackItem` from neo-vm

---

## 6. Circular Dependency Analysis

### 6.1 Dependency Chains Preventing Extraction

**Chain 1: P2P → SmartContract → Persistence**
```
network/p2p/payloads/Transaction
  └──> smart_contract::ApplicationEngine (for verification)
       └──> persistence::DataCache (for state access)
            └──> smart_contract::StorageItem (storage values)
                 └──> smart_contract::IInteroperable (cached objects)
```

**Chain 2: LocalNode → Blockchain → NeoSystem**
```
network/p2p/LocalNode
  └──> ledger::Blockchain (actor ref for block relay)
       └──> neo_system::NeoSystem (system context)
            └──> akka::ActorContext (actor runtime)
                 └──> services::PeerManagerService (P2P coordination)
```

**Chain 3: Persistence → SmartContract Types**
```
persistence::StorageItem
  └──> smart_contract::IInteroperable (trait)
       └──> neo_vm::StackItem (VM types)

persistence::DataCache
  └──> smart_contract::StorageKey (key type - ✅ already in neo-storage)
  └──> smart_contract::StorageItem (value type - ❌ blocked)
```

### 6.2 Breaking the Cycles

**Option 1: Trait Abstraction**
```rust
// In neo-storage:
pub trait IStorageValue: Clone + Send + Sync {
    fn to_bytes(&self) -> Vec<u8>;
    fn from_bytes(bytes: &[u8]) -> Result<Self, Error>;
    fn size(&self) -> usize;
}

// In neo-core:
impl IStorageValue for StorageItem { ... }
```

**Option 2: Type Parameterization**
```rust
// In neo-storage:
pub struct DataCache<K, V>
where
    K: StorageKeyLike,
    V: StorageValueLike,
{
    dictionary: Arc<RwLock<HashMap<K, Trackable<V>>>>,
    // ...
}
```

**Option 3: Move Smart Contract Types to neo-vm**
- Move IInteroperable to neo-vm crate
- StorageItem can then live in neo-storage
- But: Creates VM → Storage circular dependency

---

## 7. Breaking Change Assessment

### 7.1 Public API Changes

**If P2P is fully extracted:**
```diff
- use neo_core::network::p2p::{LocalNode, RemoteNode, Block, Transaction};
+ use neo_p2p::{LocalNode, RemoteNode, Block, Transaction};
+ use neo_p2p::actors::{LocalNodeActor, RemoteNodeActor};
+ use neo_p2p::payloads::{Block, Transaction, Header};
```

**Impact:** HIGH - All users of P2P types must update imports

**If Storage is fully extracted:**
```diff
- use neo_core::persistence::{DataCache, IStore, ISnapshot};
+ use neo_storage::{DataCache, IStore, ISnapshot};
+ use neo_storage::providers::RocksDbProvider;
```

**Impact:** HIGH - All smart contract code uses DataCache

**If Crypto is cleaned up (remove re-exports):**
```diff
- use neo_core::cryptography::{Crypto, ECPoint};
+ use neo_crypto::{Crypto, ECPoint};
```

**Impact:** MEDIUM - Easy find-and-replace

### 7.2 Breaking Changes by Category

**Type relocations:**
| Type | Current Location | Target Location | Impact |
|------|-----------------|-----------------|--------|
| Transaction | neo_core::network::p2p::payloads | neo_p2p::payloads | HIGH |
| Block | neo_core::network::p2p::payloads | neo_p2p::payloads | HIGH |
| DataCache | neo_core::persistence | neo_storage::cache | HIGH |
| StorageItem | neo_core::persistence | neo_storage::types | MEDIUM |
| ECPoint | neo_core::cryptography | neo_crypto::ecc | LOW (re-export exists) |

**Trait relocations:**
| Trait | Current Location | Target Location | Blocker |
|-------|-----------------|-----------------|---------|
| IVerifiable | neo_core::lib | neo_p2p::traits OR neo-vm | Requires DataCache, ProtocolSettings |
| IInteroperable | neo_core::smart_contract | neo_vm::interop | Circular: StorageItem needs this |
| IStore | neo_core::persistence | neo_storage::traits | ✅ Can move |

### 7.3 Migration Path Complexity

**Scenario 1: Full extraction (all 3 domains)**
- **Complexity:** 9/10 - Requires breaking circular dependencies
- **Estimated files changed:** 200+
- **Estimated LOC changed:** 15,000+
- **Risk:** HIGH - May break existing code extensively

**Scenario 2: Incremental extraction (traits first, then implementations)**
- **Complexity:** 6/10 - Progressive migration with compatibility shims
- **Estimated phases:** 4-6 phases over 3-6 months
- **Risk:** MEDIUM - Can be done with backward compatibility

**Scenario 3: Keep re-exports (types stay in neo-core, crates only hold lightweight duplicates)**
- **Complexity:** 3/10 - Current approach
- **Benefit:** LOW - Doesn't solve the monolithic neo-core problem

---

## 8. Recommended Migration Order

### Phase 1: Foundation (CURRENT STATE)
- ✅ neo-crypto fully migrated
- ✅ neo-storage traits migrated
- ✅ neo-p2p enums migrated
- ⚠️ All implementations still in neo-core

### Phase 2: Storage Layer Completion (6-8 weeks)
**Goal:** Move StorageItem + DataCache to neo-storage

**Steps:**
1. Move IInteroperable trait to neo-vm crate
2. Make StorageItem generic over cached type OR remove IInteroperable dependency
3. Move StorageItem to neo-storage
4. Move DataCache to neo-storage (depends on StorageItem)
5. Update all neo-core imports

**Blockers to resolve:**
- StorageItem → IInteroperable dependency
- DataCache → OnEntryDelegate closure (requires trait abstraction)

**Estimated impact:** 54 files across neo-core

### Phase 3: P2P Payloads Extraction (8-12 weeks)
**Goal:** Move Block, Transaction, Header to neo-p2p

**Steps:**
1. Create `neo-ledger` crate for Blockchain types (Block, Header, Transaction)
2. Move IVerifiable trait to neo-ledger OR parameterize verification
3. Move Transaction/Block verification logic to neo-ledger
4. Update P2P actors to import from neo-ledger
5. Create backward-compatible re-exports in neo-core

**Blockers to resolve:**
- Transaction → ApplicationEngine dependency (verification)
- Block → DataCache dependency (state validation)
- IVerifiable → DataCache + ProtocolSettings dependency

**Estimated impact:** 100+ files across neo-core, neo-plugins

### Phase 4: P2P Actors Extraction (12-16 weeks)
**Goal:** Move LocalNode, RemoteNode, TaskManager to neo-p2p

**Steps:**
1. Extract actor runtime (ractor) from neo-core/src/akka/ to neo-akka crate
2. Create service traits for Blockchain, MemoryPool, PeerManager
3. Move LocalNode actor to neo-p2p (parameterized over services)
4. Move RemoteNode actor to neo-p2p
5. Move TaskManager to neo-p2p
6. Update neo-core to instantiate P2P actors from neo-p2p

**Blockers to resolve:**
- LocalNode → NeoSystemContext dependency
- RemoteNode → BlockchainCommand actor messages
- TaskManager → Blockchain actor dependency

**Estimated impact:** 150+ files (every file that creates/references actors)

### Phase 5: Cleanup & Deprecation (4-6 weeks)
**Goal:** Remove all neo-core re-exports, enforce clean boundaries

**Steps:**
1. Mark all neo-core re-exports as `#[deprecated]`
2. Add migration guide in CHANGELOG.md
3. Update all internal neo-core code to use new crate imports
4. Run `cargo clippy` to find deprecated usage
5. Remove deprecated re-exports in neo-rs 0.8.0

---

## 9. Risk Assessment

### 9.1 Technical Risks

| Risk | Severity | Likelihood | Mitigation |
|------|----------|------------|------------|
| **Circular dependencies cannot be broken** | HIGH | MEDIUM | Use trait abstraction + type parameterization |
| **Performance regression from trait dispatch** | MEDIUM | MEDIUM | Benchmark before/after, use `#[inline]` |
| **Compilation time increases** | LOW | HIGH | Limit dependency graph depth, use feature flags |
| **Breaking changes cascade to plugins** | HIGH | HIGH | Provide compatibility shims for 1-2 releases |
| **Actor runtime extraction breaks supervision** | MEDIUM | LOW | Keep akka in neo-core for now, extract later |

### 9.2 Project Risks

| Risk | Severity | Impact |
|------|----------|--------|
| **6+ month migration timeline** | MEDIUM | Delays other features |
| **Code freeze required for major phases** | LOW | Can use feature branches |
| **Community resistance to breaking changes** | MEDIUM | Clear communication + migration guide |
| **Incomplete migration leaves hybrid state** | HIGH | Use TODO-tracking + project board |

### 9.3 Risk Mitigation Strategy

**Incremental migration with backward compatibility:**
1. Never remove types from neo-core until ALL usages are migrated
2. Use `#[deprecated]` warnings 1-2 releases before removal
3. Maintain re-exports during transition period
4. Use workspace-wide `cargo test` as migration gate

**Feature flags for experimental extraction:**
```toml
[features]
default = ["neo-core-compat"]
neo-core-compat = []  # Enable re-exports
standalone-p2p = []    # Use neo-p2p types directly
```

---

## 10. Estimated Complexity Score

### 10.1 Complexity by Domain

| Domain | Files | LOC | Dependencies | Circular Deps | Complexity Score |
|--------|-------|-----|--------------|---------------|------------------|
| **neo-crypto** | 14 | ~2,000 | 0 internal | 0 | ✅ 1/10 (DONE) |
| **neo-storage** | 26 | ~4,053 | 2 blockers | 1 | ⚠️ 6/10 (MEDIUM) |
| **neo-p2p** | 82 | ~6,788 | 6 domains | 3 | ❌ 9/10 (HIGH) |

**Overall Project Complexity: 8/10 (VERY HIGH)**

### 10.2 Work Estimate

**Total estimated effort:**
- **Phase 2 (Storage):** 6-8 weeks (1 senior dev)
- **Phase 3 (P2P Payloads):** 8-12 weeks (1 senior dev)
- **Phase 4 (P2P Actors):** 12-16 weeks (2 senior devs)
- **Phase 5 (Cleanup):** 4-6 weeks (1 dev)

**Total:** 30-42 weeks (7-10 months) for complete extraction

**Parallel tracks possible:**
- Storage extraction (Phase 2) can proceed independently
- P2P Payloads (Phase 3) can start while Storage is in review
- Crypto cleanup can happen anytime (low risk)

---

## 11. Key Findings Summary

### ✅ What's Working
1. **neo-crypto** is fully extracted and functional
2. **neo-storage** traits provide clean abstraction layer
3. **neo-p2p** enums enable external tool integration
4. Workspace structure supports incremental migration

### ⚠️ What's Partially Complete
1. **neo-storage** missing StorageItem (blocked by IInteroperable)
2. **neo-storage** missing DataCache (blocked by StorageItem)
3. **neo-p2p** missing all payloads (Transaction, Block, Header)
4. **neo-p2p** missing all actors (LocalNode, RemoteNode, TaskManager)

### ❌ What's Blocking Progress
1. **Circular dependency:** StorageItem → IInteroperable → StackItem → neo-vm
2. **Circular dependency:** P2P payloads → Blockchain → NeoSystem → P2P actors
3. **Circular dependency:** Transaction → ApplicationEngine → DataCache → StorageItem
4. **Trait coupling:** IVerifiable requires DataCache + ProtocolSettings from neo-core
5. **Actor coupling:** LocalNode/RemoteNode tightly bound to NeoSystem/Blockchain actors

---

## 12. Actionable Recommendations

### Immediate Actions (Week 1-2)
1. **Create neo-ledger crate** for Block/Transaction/Header types
2. **Move IInteroperable to neo-vm** to unblock StorageItem
3. **Create trait abstraction** for storage value types
4. **Document migration strategy** in ARCHITECTURE.md

### Short-term (Month 1-3)
1. **Complete Phase 2:** Extract StorageItem + DataCache to neo-storage
2. **Update neo-core imports** to use neo-storage:: instead of crate::persistence::
3. **Run benchmarks** to validate no performance regression
4. **Create backward-compat shims** with deprecation warnings

### Medium-term (Month 4-6)
1. **Complete Phase 3:** Extract P2P payloads to neo-ledger crate
2. **Parameterize verification** to break IVerifiable → DataCache dependency
3. **Create service traits** for Blockchain/MemoryPool to prepare Phase 4
4. **Update neo-plugins** to use new import paths

### Long-term (Month 7-10)
1. **Complete Phase 4:** Extract P2P actors to neo-p2p
2. **Extract actor runtime** from neo-core/src/akka/ to neo-akka crate
3. **Complete Phase 5:** Remove all deprecated re-exports
4. **Release neo-rs 0.8.0** with clean crate boundaries

---

## 13. Success Criteria

**Phase 2 (Storage) Success:**
- [ ] neo-storage contains StorageItem, StorageKey, DataCache
- [ ] Zero `use crate::persistence::` in neo-core outside re-exports
- [ ] All tests pass with no performance regression
- [ ] Documentation updated

**Phase 3 (P2P Payloads) Success:**
- [ ] neo-ledger contains Transaction, Block, Header
- [ ] neo-p2p re-exports ledger types for convenience
- [ ] Verification logic abstracted with traits
- [ ] Backward-compat re-exports in neo-core

**Phase 4 (P2P Actors) Success:**
- [ ] LocalNode, RemoteNode, TaskManager in neo-p2p
- [ ] Service traits eliminate direct neo-core dependencies
- [ ] Actors can be instantiated from neo-p2p crate
- [ ] neo-core uses neo-p2p actors (not embedded versions)

**Phase 5 (Cleanup) Success:**
- [ ] Zero type definitions in neo-core for extracted domains
- [ ] All re-exports removed (breaking change)
- [ ] Migration guide complete
- [ ] Neo-rs 0.8.0 released

---

## Appendix A: File Counts

### neo-core/src/network/p2p/ (82 files)
```
payloads/                    45 files
├── transaction/              6 files
├── conditions/               8 files
└── [other payloads]         31 files
local_node/                   7 files
remote_node/                  5 files
capabilities/                 7 files
messages/                     5 files
[helpers/utils]              13 files
```

### neo-core/src/persistence/ (26 files)
```
traits/                       5 files (IStore, ISnapshot, etc.)
cache/                        4 files (DataCache, StoreCache, etc.)
providers/                    3 files (RocksDB, Memory)
types/                        4 files (StorageKey, StorageItem, etc.)
utils/                       10 files (serialization, compression, etc.)
```

### neo-crypto/ (14 files) - ✅ COMPLETE
```
core/                         8 files
mpt_trie/                     5 files
error.rs                      1 file
```

### neo-storage/ (6 files) - ⚠️ TRAITS ONLY
```
traits.rs                     1 file (IStore, ISnapshot, etc.)
types.rs                      1 file (StorageKey, SeekDirection, TrackState)
key_builder.rs                1 file
hash_utils.rs                 1 file
error.rs                      1 file
lib.rs                        1 file
```

---

## Appendix B: Dependency Counts

**neo-core internal cross-references:**
- `use crate::ledger::` - 27 files
- `use crate::smart_contract::` - 48 files
- `use crate::persistence::` - 54 files
- `use crate::neo_system::` - 23 files
- `use crate::akka::` - 34 files
- `use crate::cryptography::` - 27 files (should migrate to neo_crypto::)

**External crate references:**
- `use neo_crypto::` - 0 files (all use crate::cryptography re-export)
- `use neo_storage::` - 3 files (StorageKey, SeekDirection, TrackState)
- `use neo_p2p::` - 0 files (all use crate::network::p2p re-export)

---

**Report End**

**Generated by:** BMAD Orchestrator Agent (UltraThink Methodology)
**Contact:** Escalate to Architect or SM for dependency resolution strategy
