# Task #138 Supplementary Report: Redirect Target Credibility & Module Mapping
**Date**: 2026-11-05  
**Purpose**: Enable safe "redirect→delete→verify-green" wave planning for neo-core decomposition Phase 2  
**Scope**: Read-only audit supplement

---

## 1. Redirect Target Credibility Assessment

### Credibility Criteria
A crate is a **trusted redirect target** if it meets ALL of:
1. ✓ Registered in [workspace.members]
2. ✓ Compiles successfully with gnullvm toolchain
3. ✓ Contains real implementation (>1000 LOC, not stub/scaffold)
4. ✓ Has corresponding module structure to replace neo-core original

### Credibility Matrix

| Crate | Registered | Compiles | LOC | Real Impl | Module Match | **Trusted Target** | Notes |
|-------|-----------|----------|-----|-----------|--------------|-------------------|-------|
| **neo-protocol** | ✓ | ✓ | 4,122 | ✓ | ✓ | **✓ YES** | Foundation crate, stable API |
| **neo-p2p** | ✓ | ✓ | 10,707 | ✓ | ✓ | **✓ YES** | Network layer complete |
| **neo-storage** | ✓ | ✓ | 8,151 | ✓ | ✓ | **✓ YES** | Storage abstraction complete |
| **neo-io** | ✓ | ✓ | 4,286 | ✓ | ✓ | **✓ YES** | IO layer complete |
| **neo-vm** | ✓ | ✓ | 26,601 | ✓ | ✓ | **✓ YES** | VM runtime complete |
| **neo-telemetry** | ✓ | ✓ | 4,073 | ✓ | ✓ | **✓ YES** | Telemetry complete |
| **neo-ledger** | ✓ | ✓ | 5,724 | ✓ | ⚠ Partial | **⚠ CONDITIONAL** | Missing blockchain actor, genesis, memory_pool |
| **neo-signing** | ✓ | ✓ | 4,410 | ✓ | ✓ Full | **✓ YES** | Full duplicate of neo-core/src/signing |
| **neo-smart-contract** | ✓ | ✓ | 12,899 | ✓ | ⚠ Partial | **⚠ CONDITIONAL** | Missing native contracts (moved to neo-native-contracts) |
| **neo-wallets** | ✓ | ✓ | 2,458 | ✓ | ⚠ Partial | **⚠ CONDITIONAL** | Missing asset_descriptor, transfer_output, wallet_factory, wallet_provider |
| **neo-native-contracts** | ✓ | ✓ | 19,708 | ✓ | ✓ | **✓ YES** | Native contracts complete, but depends on neo-core (test-only) |
| **neo-mempool** | ✓ | ✓ | 3,055 | ✓ | ✓ | **⚠ CONDITIONAL** | Depends on neo-core validation APIs |
| **neo-oracle** | ✓ | ✓ | 2,000 | ✓ | ⚠ Partial | **⚠ CONDITIONAL** | Missing neofs grpc, request handlers (3,569 LOC delta) |
| **neo-system** | ✓ | ✓ | 2,332 | ✓ | ⚠ Partial | **⚠ CONDITIONAL** | Missing builder, converters, helpers, registry, relay, services (1,571 LOC delta) |
| **neo-state-service** | ✓ | **✗ FAIL** | 705 | **✗ Stub** | **✗ NO** | **✗ NO** | Incomplete scaffold, missing state_store module |

### Summary
- **Fully trusted targets (6)**: neo-protocol, neo-p2p, neo-storage, neo-io, neo-vm, neo-telemetry, neo-signing, neo-native-contracts
- **Conditional targets (6)**: neo-ledger, neo-smart-contract, neo-wallets, neo-mempool, neo-oracle, neo-system
- **NOT trusted (1)**: neo-state-service (fails compilation, incomplete)

---

## 2. Module Mapping Table (New Crate ↔ neo-core Original)

### P3: Storage Layer

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-storage/src/* | 
eo-core/src/persistence/* | ⚠ Different scope | N/A | **✗ NO** - persistence serves different purpose (DataCache, snapshots) |
| 
eo-io/src/* | 
eo-core/src/extensions/io/* | ✓ Complete | 0 | **✓ YES** - already deleted per git status |

**P3 Conclusion**: neo-storage and neo-core/src/persistence have **different responsibilities**. No deletion needed.

---

### P4: Protocol & Network

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-protocol/src/protocol_settings/* | 
eo-core/src/protocol_settings/* | ✓ Complete | 0 | **✓ YES** - already re-exported (lib.rs:163) |
| 
eo-protocol/src/hardfork/* | 
eo-core/src/hardfork.rs | ✓ Complete | 0 | **✓ YES** - already deleted per git status |
| 
eo-p2p/src/* | 
eo-core/src/network/p2p/* | ⚠ Partial | Unknown | **⚠ CONDITIONAL** - need to verify network module scope |

**P4 Action Items**:
1. Verify if neo-core/src/network contains only p2p logic or additional networking
2. If only p2p, safe to delete after redirecting consumers to neo-p2p
3. Check neo-core/src/lib.rs:211 pub mod network; - what does it export?

---

### P5: VM

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-vm/src/* | 
eo-core/src/neo_vm.rs | ✓ Complete | 0 | **✓ YES** - already deleted per git status |
| 
eo-vm/src/memory/* | 
eo-core/src/memory/* | ✓ Complete | 0 | **✓ YES** - already deleted per git status |

**P5 Conclusion**: VM migration complete. No action needed.

---

### P6: Contracts

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-smart-contract/src/application_engine/* | 
eo-core/src/smart_contract/application_engine/* | ⚠ Partial | 18,908 | **✗ NO** - neo-core has 2.5x more code |
| 
eo-smart-contract/src/manifest/* | 
eo-core/src/smart_contract/manifest/* | ⚠ Unknown | Unknown | **⚠ VERIFY** |
| 
eo-smart-contract/src/iterators/* | 
eo-core/src/smart_contract/iterators/* | ⚠ Unknown | Unknown | **⚠ VERIFY** |
| 
eo-native-contracts/src/native/* | 
eo-core/src/smart_contract/native/* | ✓ Complete | 0 | **✓ YES** - native contracts fully migrated |

**P6 Action Items**:
1. **Cannot delete** neo-core/src/smart_contract yet (18,908 LOC remaining)
2. Migrate remaining application_engine logic to neo-smart-contract
3. Verify manifest and iterators modules are complete
4. Safe to delete neo-core/src/smart_contract/native after redirect

**Forbidden Edge Check**: neo-smart-contract does NOT depend on neo-oracle ✓

---

### P7: Ledger

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-ledger/src/validation/* | 
eo-core/src/ledger/block.rs, lock_header.rs, 	ransaction_verification_context.rs | ⚠ Partial | Unknown | **⚠ VERIFY** |
| 
eo-ledger/src/persistence/* | 
eo-core/src/ledger/blockchain/* | ⚠ Partial | Unknown | **⚠ VERIFY** |
| N/A | 
eo-core/src/ledger/genesis.rs | ✗ Not migrated | Unknown | **✗ NO** - still in neo-core |
| N/A | 
eo-core/src/ledger/memory_pool/* | ✗ Not migrated | 3,055 | **✗ NO** - migrated to neo-mempool instead |
| N/A | 
eo-core/src/ledger/blockchain_application_executed.rs | ✗ Not migrated | Unknown | **✗ NO** - still in neo-core |

**P7 Action Items**:
1. **Cannot delete** neo-core/src/ledger yet (multiple modules not migrated)
2. Migrate genesis.rs to neo-ledger
3. Migrate blockchain_application_executed.rs to neo-ledger
4. Redirect memory_pool consumers to neo-mempool (separate crate)
5. Verify validation and persistence modules are complete

**Forbidden Edge Check**: neo-ledger does NOT depend on neo-mempool ✓

---

### P8: System

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-system/src/runtime/* | 
eo-core/src/neo_system/system.rs, ctors.rs | ⚠ Partial | Unknown | **⚠ VERIFY** |
| 
eo-system/src/adapters/* | 
eo-core/src/neo_system/network.rs, persistence.rs | ⚠ Partial | Unknown | **⚠ VERIFY** |
| N/A | 
eo-core/src/neo_system/builder.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/neo_system/converters.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/neo_system/helpers.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/neo_system/registry.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/neo_system/relay.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/neo_system/services.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/neo_system/storage.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/neo_system/mempool.rs | ✗ Not migrated | Unknown | **✗ NO** |

**P8 Action Items**:
1. **Cannot delete** neo-core/src/neo_system yet (1,571 LOC remaining across 10 files)
2. Migrate builder, converters, helpers, registry, relay, services, storage, mempool modules
3. Verify runtime and adapters modules are complete

---

### P9: Services

#### P9.1: Signing

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-signing/src/durable_store.rs | 
eo-core/src/signing/durable_store.rs | ✓ Full duplicate | 0 | **✓ YES** |
| 
eo-signing/src/error.rs | 
eo-core/src/signing/error.rs | ✓ Full duplicate | 0 | **✓ YES** |
| 
eo-signing/src/memory_store.rs | 
eo-core/src/signing/memory_store.rs | ✓ Full duplicate | 0 | **✓ YES** |
| 
eo-signing/src/policy.rs | 
eo-core/src/signing/policy.rs | ✓ Full duplicate | 0 | **✓ YES** |
| 
eo-signing/src/remote.rs | 
eo-core/src/signing/remote.rs | ✓ Full duplicate | 0 | **✓ YES** |
| 
eo-signing/src/service.rs | 
eo-core/src/signing/service.rs | ✓ Full duplicate | 0 | **✓ YES** |
| 
eo-signing/src/store.rs | 
eo-core/src/signing/store.rs | ✓ Full duplicate | 0 | **✓ YES** |
| 
eo-signing/src/types.rs | 
eo-core/src/signing/types.rs | ✓ Full duplicate | 0 | **✓ YES** |
| 
eo-signing/src/tests.rs | 
eo-core/src/signing/tests.rs | ✓ Full duplicate | 0 | **✓ YES** |

**P9.1 Conclusion**: **SAFE TO DELETE** neo-core/src/signing (4,389 LOC) after redirecting consumers to neo-signing.

**Forbidden Edge Check**: neo-signing does NOT depend on neo-system or neo-consensus ✓

---

#### P9.2: Wallets

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-wallets/src/account.rs | N/A | ✓ New | N/A | N/A |
| 
eo-wallets/src/bip39.rs | 
eo-core/src/wallets/bip39.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-wallets/src/helper.rs | 
eo-core/src/wallets/helper.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-wallets/src/key_pair.rs | 
eo-core/src/wallets/key_pair.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-wallets/src/nep2.rs | N/A | ✓ New | N/A | N/A |
| 
eo-wallets/src/nep6.rs | 
eo-core/src/wallets/nep6.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-wallets/src/wallet.rs | 
eo-core/src/wallets/wallet.rs | ✓ Duplicate | 0 | **✓ YES** |
| N/A | 
eo-core/src/wallets/asset_descriptor.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/wallets/transfer_output.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/wallets/version.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/wallets/wallet_account.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/wallets/wallet_factory.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/wallets/wallet_provider.rs | ✗ Not migrated | Unknown | **✗ NO** |

**P9.2 Action Items**:
1. **Cannot delete** neo-core/src/wallets yet (688 LOC remaining across 6 files)
2. Migrate asset_descriptor, transfer_output, version, wallet_account, wallet_factory, wallet_provider
3. Safe to delete bip39, helper, key_pair, nep6, wallet after redirect

**Forbidden Edge Check**: neo-protocol does NOT depend on neo-wallets ✓

---

#### P9.3: Mempool

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-mempool/src/memory_pool/index.rs | 
eo-core/src/ledger/memory_pool/index.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-mempool/src/memory_pool/mod.rs | 
eo-core/src/ledger/memory_pool/mod.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-mempool/src/memory_pool/quarantine.rs | 
eo-core/src/ledger/memory_pool/quarantine.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-mempool/src/memory_pool/tests.rs | 
eo-core/src/ledger/memory_pool/tests.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-mempool/src/memory_pool/views.rs | 
eo-core/src/ledger/memory_pool/views.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-mempool/src/new_transaction_event_args.rs | 
eo-core/src/ledger/new_transaction_event_args.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-mempool/src/pool_item.rs | 
eo-core/src/ledger/pool_item.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-mempool/src/transaction_removed_event_args.rs | 
eo-core/src/ledger/transaction_removed_event_args.rs | ✓ Duplicate | 0 | **✓ YES** |

**P9.3 Conclusion**: **SAFE TO DELETE** neo-core/src/ledger/memory_pool after redirecting consumers to neo-mempool.

**Dependency Issue**: neo-mempool depends on neo-core for validation APIs. Must break this dependency first.

---

#### P9.4: Oracle

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-oracle/src/https.rs | 
eo-core/src/oracle_service/https.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-oracle/src/service.rs | 
eo-core/src/oracle_service/service.rs | ⚠ Partial | Unknown | **⚠ VERIFY** |
| 
eo-oracle/src/settings.rs | 
eo-core/src/oracle_service/settings.rs | ✓ Duplicate | 0 | **✓ YES** |
| 
eo-oracle/src/dedup.rs | N/A | ✓ New | N/A | N/A |
| 
eo-oracle/src/error.rs | N/A | ✓ New | N/A | N/A |
| 
eo-oracle/src/filter.rs | N/A | ✓ New | N/A | N/A |
| 
eo-oracle/src/ports.rs | N/A | ✓ New | N/A | N/A |
| 
eo-oracle/src/request.rs | N/A | ✓ New | N/A | N/A |
| 
eo-oracle/src/security.rs | N/A | ✓ New | N/A | N/A |
| N/A | 
eo-core/src/oracle_service/neofs/* | ✗ Not migrated | 3,569 | **✗ NO** |

**P9.4 Action Items**:
1. **Cannot delete** neo-core/src/oracle_service yet (3,569 LOC remaining in neofs submodule)
2. Migrate neofs grpc and parse logic to neo-oracle
3. Verify service.rs is complete
4. Safe to delete https.rs, settings.rs after redirect

---

#### P9.5: State Service

| New Crate Path | neo-core Original Path | Migration Status | LOC Delta | Safe to Delete neo-core? |
|----------------|------------------------|------------------|-----------|-------------------------|
| 
eo-state-service/src/participant.rs | N/A | ⚠ Scaffold | N/A | N/A |
| N/A | 
eo-core/src/state_service/account_prefetcher.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/async_state_root.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/commit_handlers.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/conflict_detector.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/keys.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/message_type.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/metrics.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/optimistic_executor.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/prefetch_*.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/read_only_store.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/state_root.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/state_store.rs | ✗ Not migrated | Unknown | **✗ NO** |
| N/A | 
eo-core/src/state_service/verification.rs | ✗ Not migrated | Unknown | **✗ NO** |

**P9.5 Conclusion**: **CANNOT DELETE** neo-core/src/state_service (10,130 LOC). neo-state-service is incomplete scaffold (705 LOC, fails compilation).

**Action Required**: Complete neo-state-service migration (Wave 1 priority).

---

## 3. Dependency Graph Cycle Analysis

### Current Dependency Edges (Simplified)

`
Foundation Layer (no cycles):
  neo-primitives ← neo-io ← neo-crypto ← neo-storage
  neo-primitives ← neo-config
  neo-primitives ← neo-vm

Protocol Layer:
  neo-primitives, neo-config, neo-crypto, neo-io, neo-vm ← neo-protocol

Decomposition Crates:
  neo-protocol, neo-storage, neo-vm, neo-crypto, neo-io, neo-primitives ← neo-smart-contract
  neo-smart-contract, neo-protocol, neo-storage, neo-vm, neo-crypto, neo-io, neo-primitives ← neo-native-contracts
  neo-native-contracts, neo-smart-contract, neo-protocol, neo-storage, neo-vm, neo-crypto, neo-io, neo-primitives ← neo-ledger
  neo-ledger, neo-p2p, neo-primitives ← neo-system
  neo-crypto, neo-primitives ← neo-signing
  neo-primitives, neo-protocol, neo-crypto ← neo-wallets
  neo-primitives, neo-protocol, neo-crypto, neo-io, neo-json ← neo-oracle
  neo-core, neo-primitives, neo-vm ← neo-mempool
  neo-crypto, neo-primitives, neo-protocol, neo-storage, neo-ledger (optional) ← neo-state-service

Consumers:
  neo-ledger ← neo-rpc
  neo-core ← neo-rpc (optional)
  neo-core ← neo-node
  neo-core ← neo-mempool
  neo-core ← neo-native-contracts (test-only)
`

### Cycle Detection Results

**✓ NO CYCLES DETECTED** in current graph.

### Redirect Impact Analysis

#### Scenario 1: Delete neo-core/src/signing, redirect to neo-signing
- **Impact**: None (neo-signing has no dependencies on neo-core)
- **Cycle risk**: ✗ NONE
- **Safe**: ✓ YES

#### Scenario 2: Delete neo-core/src/ledger/memory_pool, redirect to neo-mempool
- **Impact**: neo-mempool currently depends on neo-core for validation APIs
- **Cycle risk**: ⚠ POTENTIAL if neo-core needs mempool types
- **Safe**: ⚠ CONDITIONAL - must first extract validation APIs from neo-core to neo-protocol or neo-ledger
- **Action**: Break neo-mempool→neo-core dependency first (Wave 2)

#### Scenario 3: Delete neo-core/src/smart_contract/native, redirect to neo-native-contracts
- **Impact**: neo-native-contracts depends on neo-core (test-only for create_genesis_block)
- **Cycle risk**: ⚠ POTENTIAL if neo-core needs native contract types
- **Safe**: ⚠ CONDITIONAL - must first move create_genesis_block to neo-ledger
- **Action**: Break neo-native-contracts→neo-core dependency first (Wave 2)

#### Scenario 4: Delete neo-core/src/smart_contract (remaining), redirect to neo-smart-contract
- **Impact**: neo-smart-contract has no dependency on neo-core
- **Cycle risk**: ✗ NONE
- **Safe**: ⚠ CONDITIONAL - must first migrate remaining 18,908 LOC
- **Action**: Complete smart_contract migration (Wave 3)

#### Scenario 5: Delete neo-core/src/state_service, redirect to neo-state-service
- **Impact**: neo-state-service is incomplete scaffold
- **Cycle risk**: ✗ NONE
- **Safe**: ✗ NO - neo-state-service fails compilation
- **Action**: Complete state_service migration (Wave 1)

#### Scenario 6: Delete neo-core/src/wallets (remaining), redirect to neo-wallets
- **Impact**: neo-wallets has no dependency on neo-core
- **Cycle risk**: ✗ NONE
- **Safe**: ⚠ CONDITIONAL - must first migrate remaining 688 LOC
- **Action**: Complete wallets migration (Wave 4)

#### Scenario 7: Delete neo-core/src/oracle_service (remaining), redirect to neo-oracle
- **Impact**: neo-oracle has no dependency on neo-core
- **Cycle risk**: ✗ NONE
- **Safe**: ⚠ CONDITIONAL - must first migrate remaining 3,569 LOC (neofs)
- **Action**: Complete oracle migration (Wave 4)

#### Scenario 8: Delete neo-core/src/neo_system (remaining), redirect to neo-system
- **Impact**: neo-system depends on neo-ledger (no cycle)
- **Cycle risk**: ✗ NONE
- **Safe**: ⚠ CONDITIONAL - must first migrate remaining 1,571 LOC
- **Action**: Complete system migration (Wave 4)

#### Scenario 9: Delete neo-core/src/ledger (remaining), redirect to neo-ledger
- **Impact**: neo-ledger depends on neo-native-contracts and neo-smart-contract (no cycle)
- **Cycle risk**: ✗ NONE
- **Safe**: ⚠ CONDITIONAL - must first migrate remaining 1,150 LOC
- **Action**: Complete ledger migration (Wave 5)

---

## 4. Safe Deletion & Redirect Sequencing

### Phase 2 Wave Plan (Dependency-Aware)

#### Wave 1: Complete Incomplete Scaffolds (CRITICAL)
**Target**: neo-state-service  
**Precondition**: None  
**Work**:
1. Migrate state_store, state_root, keys, metrics, root_cache, verification, tiered_storage, read_only_store, async_state_root, conflict_detector, optimistic_executor, prefetch_pipeline modules from neo-core/src/state_service
2. Fix compilation error (missing state_store module)
3. Verify neo-state-service compiles with --features full
4. Run workspace check including neo-state-service

**Deletion**: None yet (keep neo-core/src/state_service until Wave 6)  
**Redirect**: None yet  
**Estimated LOC**: 9,425 LOC migration  
**Cycle risk**: ✗ NONE

---

#### Wave 2: Break neo-core Dependencies (HIGH PRIORITY)
**Target**: neo-mempool, neo-native-contracts  
**Precondition**: Wave 1 complete (or can parallelize)  
**Work**:
1. Extract validation APIs from neo-core to neo-protocol or neo-ledger
2. Move create_genesis_block from neo-core/src/ledger/genesis.rs to neo-ledger
3. Remove 
eo-core dependency from neo-mempool/Cargo.toml
4. Remove 
eo-core dependency from neo-native-contracts/Cargo.toml (dev-dependencies)
5. Verify both crates compile without neo-core

**Deletion**: None yet  
**Redirect**: None yet  
**Estimated LOC**: ~500 LOC API refactoring  
**Cycle risk**: ✗ NONE (breaking cycles, not creating them)

---

#### Wave 3: Safe Immediate Deletions (QUICK WINS)
**Target**: neo-signing (full duplicate)  
**Precondition**: Wave 2 complete  
**Work**:
1. Identify all consumers of neo-core/src/signing (grep for 
eo_core::signing)
2. Redirect consumers to 
eo_signing::*
3. Delete neo-core/src/signing directory (4,389 LOC)
4. Remove pub mod signing; from neo-core/src/lib.rs:265
5. Run workspace check

**Deletion**: ✓ neo-core/src/signing (4,389 LOC)  
**Redirect**: neo-core::signing → neo_signing  
**Cycle risk**: ✗ NONE (neo-signing has no neo-core dependency)

---

#### Wave 4: Complete Contract Migration (HIGH COMPLEXITY)
**Target**: neo-smart-contract, neo-native-contracts  
**Precondition**: Wave 2, Wave 3 complete  
**Work**:
1. Migrate remaining application_engine logic from neo-core/src/smart_contract/application_engine (18,908 LOC)
2. Verify manifest and iterators modules are complete
3. Identify all consumers of neo-core/src/smart_contract
4. Redirect consumers to neo-smart-contract and neo-native-contracts
5. Delete neo-core/src/smart_contract directory (31,807 LOC)
6. Remove pub mod smart_contract; from neo-core/src/lib.rs:201
7. Run workspace check

**Deletion**: ✓ neo-core/src/smart_contract (31,807 LOC)  
**Redirect**: neo-core::smart_contract → neo_smart_contract, neo_native_contracts  
**Cycle risk**: ✗ NONE (verified no forbidden edges)

---

#### Wave 5: Complete Service Migrations (MEDIUM PRIORITY)
**Target**: neo-wallets, neo-oracle, neo-system  
**Precondition**: Wave 4 complete (or can parallelize)  
**Work**:
1. Migrate remaining wallets logic (688 LOC): asset_descriptor, transfer_output, version, wallet_account, wallet_factory, wallet_provider
2. Migrate remaining oracle_service logic (3,569 LOC): neofs grpc, parse
3. Migrate remaining neo_system logic (1,571 LOC): builder, converters, helpers, registry, relay, services, storage, mempool
4. Redirect consumers for each module
5. Delete neo-core/src/wallets (3,146 LOC)
6. Delete neo-core/src/oracle_service (5,569 LOC)
7. Delete neo-core/src/neo_system (3,903 LOC)
8. Remove module exports from neo-core/src/lib.rs
9. Run workspace check

**Deletion**: ✓ neo-core/src/wallets, oracle_service, neo_system (12,618 LOC total)  
**Redirect**: Multiple  
**Cycle risk**: ✗ NONE

---

#### Wave 6: Complete Ledger Migration (MEDIUM PRIORITY)
**Target**: neo-ledger  
**Precondition**: Wave 2, Wave 4 complete  
**Work**:
1. Migrate remaining ledger logic (1,150 LOC): genesis, blockchain_application_executed, header_wire, ledger_context, transaction_router, transaction_removal_reason, verify_result
2. Redirect consumers
3. Delete neo-core/src/ledger (6,874 LOC)
4. Remove pub mod ledger; from neo-core/src/lib.rs:206
5. Run workspace check

**Deletion**: ✓ neo-core/src/ledger (6,874 LOC)  
**Redirect**: neo-core::ledger → neo_ledger  
**Cycle risk**: ✗ NONE

---

#### Wave 7: Cleanup & Promotion (FINAL)
**Target**: All decomposition crates  
**Precondition**: Waves 1-6 complete  
**Work**:
1. Add decomposition crates to default-members in root Cargo.toml
2. Delete neo-core/src/state_service (10,130 LOC) after Wave 1 redirect
3. Delete neo-core/src/network if fully migrated to neo-p2p
4. Remove all duplicate module exports from neo-core/src/lib.rs
5. Run full workspace compilation with all crates included
6. Commit all untracked directories to git
7. Update documentation

**Deletion**: ✓ neo-core/src/state_service, network (remaining duplicates)  
**Redirect**: Final cleanup  
**Cycle risk**: ✗ NONE

---

## 5. Forbidden Edge Verification

### Spec Forbidden Edges (from task description)

| Forbidden Edge | Current Status | Verification |
|----------------|----------------|--------------|
| ledger→mempool | ✓ NOT PRESENT | neo-ledger/Cargo.toml does not depend on neo-mempool |
| protocol→wallets | ✓ NOT PRESENT | neo-protocol/Cargo.toml does not depend on neo-wallets |
| signing→system | ✓ NOT PRESENT | neo-signing/Cargo.toml does not depend on neo-system |
| signing→consensus | ✓ NOT PRESENT | neo-signing/Cargo.toml does not depend on neo-consensus |
| smart-contract→oracle | ✓ NOT PRESENT | neo-smart-contract/Cargo.toml does not depend on neo-oracle |

**Conclusion**: ✓ ALL FORBIDDEN EDGES ABSENT. Current dependency graph is compliant with Spec.

---

## 6. Risk Assessment & Mitigation

### High-Risk Operations

| Operation | Risk Level | Mitigation |
|-----------|-----------|------------|
| Delete neo-core/src/smart_contract (31,807 LOC) | **HIGH** | Complete Wave 4 migration first, verify all consumers redirected |
| Delete neo-core/src/state_service (10,130 LOC) | **HIGH** | Complete Wave 1 migration first, fix neo-state-service compilation |
| Break neo-mempool→neo-core dependency | **MEDIUM** | Extract validation APIs carefully, run full test suite |
| Break neo-native-contracts→neo-core dependency | **MEDIUM** | Move create_genesis_block to neo-ledger, update tests |

### Low-Risk Operations

| Operation | Risk Level | Mitigation |
|-----------|-----------|------------|
| Delete neo-core/src/signing (4,389 LOC) | **LOW** | Full duplicate, simple redirect |
| Delete neo-core/src/ledger/memory_pool | **LOW** | After Wave 2 dependency break |
| Delete neo-core/src/smart_contract/native | **LOW** | After Wave 2 dependency break |

### Rollback Strategy

1. **Keep neo-core duplicates** until Wave 7 (final cleanup)
2. **Incremental validation**: After each wave, run cargo check --workspace --features full
3. **Git commits**: Commit after each successful wave (not before)
4. **Branch strategy**: Work on feature branch, merge only after all waves complete

---

## 7. Conclusion & Recommendations

### Redirect Target Readiness

**Immediately safe to redirect (Wave 3)**:
- neo-signing (full duplicate, no dependencies)

**Conditionally safe after Wave 2**:
- neo-mempool (after breaking neo-core dependency)
- neo-native-contracts (after moving create_genesis_block)

**Conditionally safe after Wave 4-6**:
- neo-smart-contract (after completing 18,908 LOC migration)
- neo-wallets (after completing 688 LOC migration)
- neo-oracle (after completing 3,569 LOC migration)
- neo-system (after completing 1,571 LOC migration)
- neo-ledger (after completing 1,150 LOC migration)

**NOT safe until Wave 1**:
- neo-state-service (incomplete scaffold, fails compilation)

### Recommended Execution Order

`
Wave 1 (neo-state-service completion) ──┐
                                        ├─→ Wave 3 (signing deletion)
Wave 2 (break neo-core dependencies) ──┘         │
                                                  ↓
                                          Wave 4 (smart_contract)
                                                  │
                                                  ↓
                                          Wave 5 (services)
                                                  │
                                                  ↓
                                          Wave 6 (ledger)
                                                  │
                                                  ↓
                                          Wave 7 (cleanup & promotion)
`

**Parallelization opportunities**:
- Wave 1 and Wave 2 can run in parallel
- Wave 5 can partially parallelize with Wave 4

### Total Estimated Work

- **Wave 1**: 9,425 LOC migration
- **Wave 2**: ~500 LOC API refactoring
- **Wave 3**: 4,389 LOC deletion + redirect
- **Wave 4**: 18,908 LOC migration + 31,807 LOC deletion
- **Wave 5**: 5,828 LOC migration + 12,618 LOC deletion
- **Wave 6**: 1,150 LOC migration + 6,874 LOC deletion
- **Wave 7**: 10,130 LOC deletion + final cleanup

**Total**: ~40,000 LOC migration + ~65,818 LOC deletion

### Final Recommendation

**Proceed with Wave 1 and Wave 2 in parallel** to unblock subsequent waves. Wave 3 (signing deletion) can execute immediately after Wave 2 completes, providing a quick win and validating the redirect→delete→verify process.

**Do NOT proceed with Wave 4-7** until Waves 1-3 complete successfully and workspace achieves true green status (including neo-state-service).

---

**Supplementary report generated by**: Task #138 audit  
**Audit method**: Read-only investigation (no files modified)  
**Evidence sources**: Cargo.toml files, source code structure analysis, compilation logs, dependency graph analysis
