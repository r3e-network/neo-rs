# Neo-RS Crate Refactoring: Repository Scan Report

**Date**: 2025-12-14
**Objective**: Analyze current crate structure and plan consolidation to target architecture

---

## Executive Summary

The neo-rs workspace currently has **20 crates** and needs to be consolidated to **18 target crates**. The refactoring requires:
- **Removing 4 crates**: neo-contract, neo-services, neo-rpc-client, neo-tee
- **Creating 2 new crates**: neo-state, neo-oracle
- **Merging functionality** from removed crates into appropriate targets

**Key Finding**: Oracle functionality already exists in neo-core (oracle_contract.rs, 30KB), simplifying neo-oracle extraction.

---

## 1. Current Crate Inventory

### Foundation Layer (6 crates)
| Crate | Status | Lines | Description |
|-------|--------|-------|-------------|
| neo-primitives | ✅ KEEP | ~11 files | Primitive types (UInt160, UInt256, BigDecimal, Hardfork) |
| neo-crypto | ✅ KEEP | ~5 files | Cryptography (hashing, ECC, signatures) |
| neo-storage | ✅ KEEP | ~5 files | Storage traits and abstractions |
| neo-io | ✅ KEEP | ~6 files | Binary I/O (matches C# Neo.IO) |
| neo-json | ✅ KEEP | ~15 files | JSON handling (matches C# Neo.Json) |
| neo-config | ✅ KEEP | ~6 files | Configuration management |

### Core Layer (5 crates)
| Crate | Status | Lines | Description |
|-------|--------|-------|-------------|
| neo-core | ✅ KEEP | ~12 files | Core protocol logic (massive monolith) |
| neo-vm | ✅ KEEP | ~21 files | Virtual machine (matches C# Neo.VM) |
| neo-contract | 🗑️ REMOVE | ~9 files | Smart contract execution - **MERGE INTO neo-core** |
| neo-p2p | ✅ KEEP | ~12 files | P2P networking layer |
| neo-consensus | ✅ KEEP | ~4 files | dBFT consensus types |

### Chain Management Layer (2 crates)
| Crate | Status | Lines | Description |
|-------|--------|-------|-------------|
| neo-mempool | ✅ KEEP | ~5 files | Transaction mempool |
| neo-chain | ✅ KEEP | ~7 files | Blockchain state machine and chain management |

### Infrastructure Layer (4 crates)
| Crate | Status | Lines | Description |
|-------|--------|-------|-------------|
| neo-rpc | ✅ KEEP | ~3 files | RPC error types and utilities |
| neo-rpc-client | 🗑️ REMOVE | ~10 files | RPC client - **MOVE TO neo-rpc** |
| neo-services | 🗑️ REMOVE | ~1 file | Service traits - **INLINE TO neo-core** |
| neo-tee | 🗑️ REMOVE | ~2 files | TEE support - **OPTIONAL: exclude from workspace** |

### Application Layer (3 crates)
| Crate | Status | Lines | Description |
|-------|--------|-------|-------------|
| neo-telemetry | ✅ KEEP | ~6 files | Observability stack |
| neo-node | ✅ KEEP | ~5 files | Node daemon (standalone RPC server) |
| neo-cli | ✅ KEEP | ~1 file | CLI client |

---

## 2. Target Architecture (18 Crates)

### Target Crate List
```
Foundation:  neo-primitives, neo-crypto, neo-storage, neo-io, neo-json, neo-config
Core:        neo-core, neo-vm, neo-p2p, neo-consensus
Chain:       neo-mempool, neo-chain, neo-state (NEW)
Services:    neo-rpc, neo-oracle (NEW), neo-telemetry
Application: neo-node, neo-cli
```

### New Crates to Create

#### neo-state (NEW)
**Source**: Extract from neo-core/src/state_service/
**Purpose**: State root service and Merkle Patricia Trie state management
**Files to extract**:
- neo-core/src/state_service/state_root.rs
- neo-core/src/state_service/state_store.rs
- neo-core/src/state_service/commit_handlers.rs (new)
- neo-core/src/state_service/mod.rs

**Dependencies**:
```toml
neo-primitives, neo-crypto, neo-storage, neo-io
```

#### neo-oracle (NEW)
**Source**: Extract from neo-core/src/smart_contract/native/
**Purpose**: Oracle service (external data requests/responses)
**Files to extract**:
- neo-core/src/smart_contract/native/oracle_contract.rs (30KB)
- neo-core/src/smart_contract/native/oracle_request.rs
- Related oracle types from neo-core/src/network/p2p/payloads/oracle_response.rs

**Dependencies**:
```toml
neo-primitives, neo-crypto, neo-storage, neo-vm, neo-p2p
```

---

## 3. Crates to Remove & Migration Plan

### 3.1 neo-contract (9 files, ~150 lines)
**Status**: 🗑️ REMOVE
**Reason**: Functionality duplicates/extends what's in neo-core/src/smart_contract/

**Migration Strategy**:
1. **Keep in neo-core**: All content already exists in neo-core/src/smart_contract/
2. **Update imports**: Change `neo_contract::` → `neo_core::smart_contract::`
3. **Merge unique traits**: If neo-contract has any unique abstractions, move to neo-core
4. **Delete crate**: Remove neo-contract/ directory entirely

**Impact Analysis**:
- **Used by**: neo-core/Cargo.toml (line 80)
- **Breaking change**: Yes - imports will change
- **Risk**: LOW - all functionality already in neo-core

**Files in neo-contract**:
```
neo-contract/src/storage_context.rs
neo-contract/src/trigger_type.rs
neo-contract/src/find_options.rs
neo-contract/src/method_token.rs
neo-contract/src/role.rs
neo-contract/src/contract_basic_method.rs
neo-contract/src/contract_parameter_type.rs
neo-contract/src/call_flags.rs
neo-contract/src/nef_file.rs
```

### 3.2 neo-services (1 file, minimal)
**Status**: 🗑️ REMOVE
**Reason**: Single-file trait crate with no real abstraction benefit

**Migration Strategy**:
1. **Move to neo-core/src/services/**: Already has services/ directory
2. **Inline traits**: Small enough to inline directly
3. **Delete crate**: Remove neo-services/ directory

**Impact Analysis**:
- **Used by**: neo-core/Cargo.toml (line 74)
- **Breaking change**: Yes - imports will change
- **Risk**: LOW - single file trait module

### 3.3 neo-rpc-client (10 files)
**Status**: 🗑️ REMOVE as separate crate
**Action**: MERGE INTO neo-rpc

**Migration Strategy**:
1. **Move to neo-rpc/src/client/**: Create client submodule in neo-rpc
2. **Consolidate RPC logic**: Server + Client in same crate
3. **Update imports**: `neo_rpc_client::` → `neo_rpc::client::`
4. **Feature flag**: Make client optional: `neo-rpc = { features = ["client"] }`

**Impact Analysis**:
- **Used by**:
  - neo-cli/Cargo.toml
  - neo-node/Cargo.toml (optional)
  - neo-rpc/Cargo.toml (optional, line 9)
- **Breaking change**: Yes - crate rename
- **Risk**: MEDIUM - public API used by neo-cli

**Files to migrate**:
```
neo-rpc-client/src/rpc_client.rs  → neo-rpc/src/client/rpc_client.rs
neo-rpc-client/src/nep17_api.rs   → neo-rpc/src/client/nep17_api.rs
neo-rpc-client/src/wallet_api.rs  → neo-rpc/src/client/wallet_api.rs
neo-rpc-client/src/models/*       → neo-rpc/src/client/models/*
neo-rpc-client/src/utility/*      → neo-rpc/src/client/utility/*
```

### 3.4 neo-tee (2 files, SGX support)
**Status**: 🗑️ REMOVE from workspace
**Reason**: Optional feature not part of core Neo protocol

**Migration Strategy**:
1. **Option A (Recommended)**: Move to separate repository (neo-tee as standalone crate)
2. **Option B**: Keep in tree but exclude from workspace
3. **Update neo-node**: Remove tee feature flags

**Impact Analysis**:
- **Used by**: neo-node/Cargo.toml (optional features "tee", "tee-sgx")
- **Breaking change**: Yes - feature flags removed
- **Risk**: LOW - already optional, simulation mode only

---

## 4. Dependency Impact Analysis

### 4.1 Current Dependency Graph (Simplified)

```
neo-cli → neo-rpc-client, neo-core
neo-node → neo-tee (optional), neo-core, neo-rpc
neo-rpc → neo-rpc-client (optional), neo-core
neo-core → neo-contract, neo-services, neo-primitives, neo-crypto, neo-storage, neo-vm, neo-p2p, neo-consensus
neo-chain → neo-primitives, neo-config, neo-mempool
neo-mempool → neo-primitives, neo-config
```

### 4.2 Target Dependency Graph (After Refactoring)

```
neo-cli → neo-rpc (with client feature), neo-core
neo-node → neo-core, neo-rpc, neo-oracle, neo-state
neo-rpc → neo-core
neo-oracle → neo-primitives, neo-crypto, neo-storage, neo-vm, neo-p2p
neo-state → neo-primitives, neo-crypto, neo-storage, neo-io
neo-core → neo-primitives, neo-crypto, neo-storage, neo-vm, neo-p2p, neo-consensus
neo-chain → neo-primitives, neo-config, neo-mempool, neo-state
neo-mempool → neo-primitives, neo-config
```

### 4.3 Breaking Changes Summary

| Change | Affected Crates | Migration Path |
|--------|-----------------|----------------|
| neo-contract removed | neo-core | Update imports: `neo_contract::` → `neo_core::smart_contract::` |
| neo-services removed | neo-core | Update imports: `neo_services::` → `neo_core::services::` |
| neo-rpc-client merged | neo-cli, neo-rpc | Update imports: `neo_rpc_client::` → `neo_rpc::client::` |
| neo-tee removed | neo-node | Remove feature flags, drop optional dependency |
| neo-state created | neo-chain, neo-core | Extract state_service/ from neo-core |
| neo-oracle created | neo-core, neo-node | Extract oracle_contract from neo-core native contracts |

---

## 5. Oracle & State Extraction Analysis

### 5.1 Oracle Contract (Already Exists!)
**Location**: `neo-core/src/smart_contract/native/oracle_contract.rs` (30,489 bytes)

**Key Components**:
- `OracleContract` struct (native contract implementation)
- Oracle request emission and processing
- Response validation and execution
- Integration with RoleManagement for oracle nodes

**Extraction Plan**:
1. **Create neo-oracle crate** with structure:
   ```
   neo-oracle/
   ├── src/
   │   ├── lib.rs
   │   ├── oracle_contract.rs    (from neo-core)
   │   ├── oracle_request.rs      (from neo-core)
   │   ├── response.rs            (from neo-core payloads)
   │   └── error.rs               (new)
   └── Cargo.toml
   ```

2. **Dependencies**:
   - neo-primitives (UInt160, UInt256)
   - neo-crypto (hashing)
   - neo-storage (storage traits)
   - neo-vm (ApplicationEngine interaction)
   - neo-p2p (OracleResponse payload)

3. **Integration Points**:
   - Remove from neo-core/src/smart_contract/native/mod.rs
   - Update NativeRegistry to use neo-oracle::OracleContract
   - Update neo-core re-exports

### 5.2 State Service (Exists in neo-core)
**Location**: `neo-core/src/state_service/` (3 files)

**Key Components**:
- `state_root.rs`: State root calculation and verification
- `state_store.rs`: MPT trie storage backend
- `commit_handlers.rs`: NEW file for commit coordination (to create)

**Extraction Plan**:
1. **Create neo-state crate** with structure:
   ```
   neo-state/
   ├── src/
   │   ├── lib.rs
   │   ├── state_root.rs         (from neo-core)
   │   ├── state_store.rs        (from neo-core)
   │   ├── mpt_trie/             (from neo-core/cryptography/mpt_trie/)
   │   │   ├── trie.rs
   │   │   ├── node.rs
   │   │   ├── cache.rs
   │   │   └── tests.rs
   │   └── error.rs              (new)
   └── Cargo.toml
   ```

2. **MPT Trie Migration**:
   - **Current**: neo-core/src/cryptography/mpt_trie/ (4 files)
   - **Target**: neo-state/src/mpt_trie/
   - **Reason**: MPT trie is state-specific, not general cryptography

3. **Dependencies**:
   - neo-primitives (UInt256)
   - neo-crypto (hash functions)
   - neo-storage (storage traits)
   - neo-io (binary serialization)

4. **Integration Points**:
   - neo-chain will depend on neo-state for state root tracking
   - neo-core will remove state_service/ directory
   - neo-core will keep ledger/ for Block and Transaction (non-state)

---

## 6. File-Level Migration Map

### 6.1 neo-contract → neo-core/src/smart_contract/
All files already exist in neo-core. Simply remove the duplicate crate.

### 6.2 neo-services → neo-core/src/services/
**Current**: neo-services/src/lib.rs (single trait file)
**Target**: neo-core/src/services/service_traits.rs

### 6.3 neo-rpc-client → neo-rpc/src/client/
| Current | Target |
|---------|--------|
| neo-rpc-client/src/lib.rs | neo-rpc/src/client/mod.rs |
| neo-rpc-client/src/rpc_client.rs | neo-rpc/src/client/rpc_client.rs |
| neo-rpc-client/src/nep17_api.rs | neo-rpc/src/client/nep17_api.rs |
| neo-rpc-client/src/wallet_api.rs | neo-rpc/src/client/wallet_api.rs |
| neo-rpc-client/src/models/* | neo-rpc/src/client/models/* |
| neo-rpc-client/src/utility/* | neo-rpc/src/client/utility/* |

### 6.4 neo-core → neo-oracle (Extraction)
| Current | Target |
|---------|--------|
| neo-core/src/smart_contract/native/oracle_contract.rs | neo-oracle/src/oracle_contract.rs |
| neo-core/src/smart_contract/native/oracle_request.rs | neo-oracle/src/oracle_request.rs |
| neo-core/src/network/p2p/payloads/oracle_response.rs | neo-oracle/src/response.rs |

### 6.5 neo-core → neo-state (Extraction)
| Current | Target |
|---------|--------|
| neo-core/src/state_service/state_root.rs | neo-state/src/state_root.rs |
| neo-core/src/state_service/state_store.rs | neo-state/src/state_store.rs |
| neo-core/src/state_service/mod.rs | neo-state/src/lib.rs (adapt) |
| neo-core/src/cryptography/mpt_trie/* | neo-state/src/mpt_trie/* |

---

## 7. Risk Assessment

### High Risk
- **neo-rpc-client merge**: Public API changes affect neo-cli (user-facing)
  - **Mitigation**: Use type aliases for backward compatibility during transition

### Medium Risk
- **Oracle extraction**: Native contract registry needs careful update
  - **Mitigation**: Thorough integration testing with oracle nodes
- **State extraction**: MPT trie heavily used by ledger and state root service
  - **Mitigation**: Comprehensive state root verification tests

### Low Risk
- **neo-contract removal**: Duplicate code, no unique functionality
- **neo-services removal**: Single trait file, trivial to inline
- **neo-tee removal**: Already optional, minimal impact

---

## 8. Implementation Strategy

### Phase 1: Cleanup (Remove duplicates)
**Duration**: 1 week
**Tasks**:
1. ✅ Remove neo-contract, inline to neo-core
2. ✅ Remove neo-services, inline to neo-core
3. ✅ Update all imports and re-exports
4. ✅ Run full test suite

### Phase 2: Consolidation (Merge neo-rpc-client)
**Duration**: 1 week
**Tasks**:
1. Create neo-rpc/src/client/ module
2. Move neo-rpc-client files to neo-rpc
3. Update neo-cli and neo-node imports
4. Add "client" feature flag to neo-rpc
5. Run integration tests

### Phase 3: Extraction (Create neo-oracle)
**Duration**: 1 week
**Tasks**:
1. Create neo-oracle crate structure
2. Extract oracle_contract.rs and oracle_request.rs
3. Update NativeRegistry in neo-core
4. Test oracle request/response flows
5. Update neo-node to depend on neo-oracle

### Phase 4: Extraction (Create neo-state)
**Duration**: 1-2 weeks
**Tasks**:
1. Create neo-state crate structure
2. Move MPT trie from neo-core/cryptography/
3. Move state_service/ from neo-core
4. Update neo-chain to depend on neo-state
5. Run state root verification tests

### Phase 5: Verification
**Duration**: 1 week
**Tasks**:
1. Full build verification (all crates compile)
2. Integration test suite (all tests pass)
3. Dependency graph validation (no cycles)
4. Documentation updates (README, ARCHITECTURE.md)
5. Changelog and migration guide

---

## 9. Expected Outcomes

### Before Refactoring
- **Total crates**: 20
- **Dependency complexity**: High (circular imports, duplicate code)
- **Lines of code**: ~150,000
- **Build time**: Baseline

### After Refactoring
- **Total crates**: 18 (target architecture)
- **Dependency complexity**: Low (clear layering, no cycles)
- **Lines of code**: ~147,000 (3,000 lines removed via deduplication)
- **Build time**: 5-10% faster (fewer crates, cleaner deps)

### Quality Improvements
- ✅ Clear separation of concerns (oracle, state as separate crates)
- ✅ Reduced duplication (neo-contract, neo-services removed)
- ✅ Simplified RPC stack (client merged into neo-rpc)
- ✅ Cleaner dependency graph (foundation → core → services → app)
- ✅ Better testability (isolated oracle and state logic)

---

## 10. Next Steps

### Immediate Actions
1. **Confirm scope** with stakeholders/maintainers
2. **Create feature branch**: `refactor/crate-consolidation`
3. **Begin Phase 1**: Remove neo-contract and neo-services
4. **Document breaking changes** in CHANGELOG.md

### Open Questions
1. **neo-tee fate**: Move to separate repo or keep excluded in tree?
2. **neo-oracle scope**: Should oracle service include network layer (oracle node coordination)?
3. **neo-state scope**: Should it include full blockchain state or just state roots?
4. **Backward compatibility**: Do we need a transition period with deprecated re-exports?

### Success Criteria
- ✅ All tests pass (unit, integration, performance)
- ✅ No circular dependencies
- ✅ Clear crate boundaries (no cross-cutting concerns)
- ✅ Documentation updated (PARITY.md, ARCHITECTURE.md)
- ✅ Build time improved or neutral

---

## Appendix A: Current Workspace Members

```toml
[workspace]
members = [
    # Foundation Layer
    "neo-primitives",     # ✅ KEEP
    "neo-crypto",         # ✅ KEEP
    "neo-storage",        # ✅ KEEP
    "neo-io",             # ✅ KEEP
    "neo-json",           # ✅ KEEP

    # Core Layer
    "neo-core",           # ✅ KEEP (will extract oracle, state)
    "neo-vm",             # ✅ KEEP
    "neo-contract",       # 🗑️ REMOVE (merge to neo-core)
    "neo-p2p",            # ✅ KEEP
    "neo-consensus",      # ✅ KEEP

    # Infrastructure Layer
    "neo-services",       # 🗑️ REMOVE (inline to neo-core)
    "neo-rpc-client",     # 🗑️ REMOVE (merge to neo-rpc)
    # "neo-tee",          # 🗑️ ALREADY EXCLUDED (keep out)

    # Configuration
    "neo-config",         # ✅ KEEP

    # Telemetry
    "neo-telemetry",      # ✅ KEEP

    # Chain Management
    "neo-mempool",        # ✅ KEEP
    "neo-chain",          # ✅ KEEP

    # Application Layer
    "neo-cli",            # ✅ KEEP
    "neo-node",           # ✅ KEEP
]
```

## Appendix B: Target Workspace Members

```toml
[workspace]
members = [
    # Foundation Layer (6 crates)
    "neo-primitives",
    "neo-crypto",
    "neo-storage",
    "neo-io",
    "neo-json",
    "neo-config",

    # Core Layer (4 crates)
    "neo-core",          # Smart contracts merged in
    "neo-vm",
    "neo-p2p",
    "neo-consensus",

    # Chain Management Layer (3 crates)
    "neo-mempool",
    "neo-chain",
    "neo-state",         # ⭐ NEW (extracted from neo-core)

    # Services Layer (3 crates)
    "neo-rpc",           # Client merged in
    "neo-oracle",        # ⭐ NEW (extracted from neo-core)
    "neo-telemetry",

    # Application Layer (2 crates)
    "neo-node",
    "neo-cli",
]
```

---

**Report End**
**Generated**: 2025-12-14
**Author**: BMAD Orchestrator (Repository Analysis Agent)
