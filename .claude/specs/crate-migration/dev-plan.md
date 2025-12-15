# Crate Migration - Development Plan

## Overview
Migration of neo-core storage types to neo-storage crate for improved modularity. After analysis, the original scope (45 payload files + 33 actor files to neo-p2p) was adjusted due to deep dependencies on neo-core internals.

## Final Architecture

### neo-primitives
**Status**: ✅ Complete
- Core primitive types: `UInt160`, `UInt256`
- Protocol enums: `InventoryType`, `WitnessScope`, `TransactionAttributeType`, `Hardfork`
- Verification: `VerifyResult` (migrated from neo-core and neo-p2p)

### neo-storage
**Status**: ✅ Complete (Core Types)
- **Migrated Types**:
  - `StorageKey` - Full C# parity with `UInt160`/`UInt256` support, `get_hash_code()`
  - `StorageItem` - Simplified version for storage traits
  - `SeekDirection` - Forward/Backward iteration
  - `TrackState` - Cache tracking states (None, Added, Changed, Deleted, NotFound)
  - `hash_utils` - xxhash3 implementation for C#-compatible hashing
- **Traits**: `IReadOnlyStore`, `IWriteStore`, `IStore`, `ISnapshot`
- **Test Coverage**: 110 tests passed

### neo-p2p
**Status**: ✅ Complete (Basic Types)
- **Contains**: Basic protocol enums (12 files, 39 tests)
  - `MessageCommand`, `MessageFlags`
  - `InventoryType`, `NodeCapabilityType`
  - `VerifyResult`, `OracleResponseCode`
  - `WitnessConditionType`, `WitnessRuleAction`
  - `ContainsTransactionType`, `TransactionRemovalReason`
- **Architecture Decision**: Payload types (Block, Transaction, Header) remain in neo-core due to deep dependencies on `IVerifiable`, `smart_contract`, `ledger` modules

### neo-core
**Status**: ✅ Re-exports configured
- `persistence/storage_key.rs` → re-exports `neo_storage::StorageKey`
- `persistence/seek_direction.rs` → re-exports `neo_storage::SeekDirection`
- `persistence/track_state.rs` → re-exports `neo_storage::TrackState`
- `ledger/verify_result.rs` → re-exports `neo_primitives::VerifyResult`
- Full payload implementations remain here (Block, Transaction, Header, etc.)

## Completed Tasks

### Task 1: Interface Abstraction Layer ✅
- Migrated `VerifyResult` to neo-primitives as single source of truth
- Created re-export modules in neo-core and neo-p2p

### Task 2: neo-storage Core Types ✅
- Migrated `SeekDirection`, `TrackState` with C# parity
- Migrated `StorageKey` with full functionality (UInt160/UInt256, hash_code)
- Created `hash_utils.rs` with xxhash3 implementation
- Updated `KeyBuilder` tests for new StorageKey behavior

### Task 3: neo-p2p Architecture Evaluation ✅
- Analyzed 45 payload files - most require neo-core internals
- Confirmed basic enum types are already in neo-p2p (12 files)
- Decision: Keep payloads in neo-core, neo-p2p remains lightweight

### Task 4: Integration Validation ✅
- Workspace compilation: All 10+ crates compile successfully
- Test coverage: 150+ tests passed across neo-storage, neo-p2p, neo-primitives
- No circular dependencies introduced

## Key Decisions

1. **StorageItem Dual Architecture**:
   - neo-storage: Simplified (bytes + constant flag)
   - neo-core: Full (BigInt cache, IInteroperable support)

2. **P2P Payloads Stay in neo-core**:
   - Block, Transaction, Header depend on IVerifiable, smart_contract, ledger
   - Moving would create massive code duplication or circular deps

3. **Re-export Pattern**: neo-core re-exports neo-storage/neo-p2p types for backward compatibility

## Test Commands

```bash
# Run all migrated crate tests
cargo test -p neo-storage -p neo-p2p -p neo-primitives

# Verify workspace compilation
cargo check --workspace

# Full workspace test
cargo test --workspace
```

## Files Modified

### New Files
- `neo-storage/src/hash_utils.rs`
- `neo-primitives/src/verify_result.rs`

### Updated Files
- `neo-storage/src/lib.rs` - Added hash_utils module
- `neo-storage/src/types.rs` - Full StorageKey implementation
- `neo-storage/src/key_builder.rs` - Fixed test for new StorageKey
- `neo-storage/Cargo.toml` - Added xxhash-rust, rand
- `neo-primitives/src/lib.rs` - Added verify_result module

### Converted to Re-exports
- `neo-core/src/persistence/storage_key.rs`
- `neo-core/src/persistence/seek_direction.rs`
- `neo-core/src/persistence/track_state.rs`
- `neo-core/src/ledger/verify_result.rs`
- `neo-p2p/src/verify_result.rs`
