# Neo-RS Refactoring Plan

## Executive Summary

This document identifies code duplication and optimization opportunities in the neo-rs codebase.
The analysis reveals approximately **3,000+ lines** of duplicate code that can be consolidated.

## Codebase Statistics

- **Total Lines**: ~150,000+ lines of Rust code
- **Crates**: 15+ crates in workspace
- **Identified Duplication**: ~3,000+ lines (2% of codebase)

---

## 1. Critical Duplication: UInt160/UInt256 (Priority: HIGH)

### Problem
Two separate implementations of UInt160 and UInt256 exist:

| File | Lines |
|------|-------|
| `neo-core/src/uint160.rs` | 631 |
| `neo-core/src/uint256.rs` | 587 |
| `neo-primitives/src/uint160.rs` | 490 |
| `neo-primitives/src/uint256.rs` | 437 |
| **Total Duplication** | **2,145 lines** |

### Root Cause
- `neo-primitives` was created as a lightweight crate for basic types
- `neo-core` has its own implementations with additional features
- Both are used inconsistently across the codebase

### Refactoring Plan
1. **Consolidate to `neo-primitives`** - Keep UInt types in primitives only
2. **Remove `neo-core/src/uint160.rs` and `neo-core/src/uint256.rs`**
3. **Update all imports** to use `neo_primitives::{UInt160, UInt256}`
4. **Merge any unique functionality** from neo-core versions into neo-primitives

### Estimated Savings
- **~1,200 lines removed** (after merging unique features)
- Reduced maintenance burden
- Single source of truth

---

## 2. Critical Duplication: WitnessScope (Priority: HIGH)

### Problem
Two nearly identical WitnessScope implementations:

| File | Lines |
|------|-------|
| `neo-core/src/witness_scope.rs` | 392 |
| `neo-p2p/src/witness_scope.rs` | 411 |
| **Total Duplication** | **803 lines** |

### Root Cause
- `neo-p2p` needed WitnessScope for message parsing
- Instead of depending on `neo-core`, a copy was made
- Both implementations have diverged slightly

### Refactoring Plan
1. **Keep WitnessScope in `neo-primitives`** (or create `neo-types` crate)
2. **Remove both existing implementations**
3. **Update dependencies** - both neo-core and neo-p2p depend on neo-primitives

### Estimated Savings
- **~400 lines removed**
- Consistent behavior across crates

---

## 3. Medium Duplication: Error Types (Priority: MEDIUM)

### Problem
15+ separate error type definitions across crates:

```
neo-vm/src/error.rs
neo-tee/src/error.rs
neo-storage/src/error.rs
neo-rpc/src/error.rs
neo-primitives/src/error.rs
neo-p2p/src/error.rs
neo-json/src/error.rs
neo-io/src/error.rs
neo-crypto/src/error.rs
neo-core/src/error.rs
neo-core/src/network/error.rs
neo-core/src/extensions/error.rs
neo-contract/src/error.rs
neo-consensus/src/error.rs
```

### Root Cause
- Each crate defines its own error types
- No unified error handling strategy
- Many errors are similar (IoError, SerializationError, etc.)

### Refactoring Plan
1. **Create `neo-error` crate** with common error types
2. **Define base error traits** that crates can extend
3. **Use `thiserror` derive macros** consistently
4. **Implement `From` conversions** for error chaining

### Estimated Savings
- **~200-300 lines** of boilerplate removed
- Better error messages
- Consistent error handling

---

## 4. Medium Duplication: KeyBuilder (Priority: MEDIUM)

### Problem
Multiple KeyBuilder implementations:

| File | Purpose |
|------|---------|
| `neo-storage/src/key_builder.rs` | Storage key construction |
| `neo-core/src/smart_contract/key_builder.rs` | Contract storage keys |

### Refactoring Plan
1. **Consolidate into `neo-storage`**
2. **Create trait-based abstraction** for different key types
3. **Remove duplicate implementations**

### Estimated Savings
- **~100-150 lines removed**

---

## 5. Medium Duplication: Enum Types (Priority: MEDIUM)

### Problem
Similar enum definitions across crates:

| Enum | Locations |
|------|-----------|
| `InventoryType` | neo-core, neo-p2p |
| `ContractParameterType` | neo-core, neo-contract |
| `OracleResponseCode` | neo-core, neo-p2p |
| `TransactionAttributeType` | neo-core, neo-p2p |

### Refactoring Plan
1. **Move shared enums to `neo-primitives`**
2. **Update imports across crates**
3. **Remove duplicate definitions**

### Estimated Savings
- **~300-400 lines removed**

---

## 6. Low Priority: Macro Consolidation (Priority: LOW)

### Problem
Similar derive macros and implementations scattered across crates.

### Refactoring Plan
1. **Create `neo-macros` crate** for procedural macros
2. **Consolidate common derive implementations**

---

## Implementation Order

### Phase 1: Foundation (Week 1)
1. ✅ Consolidate UInt160/UInt256 to neo-primitives
2. ✅ Move WitnessScope to neo-primitives
3. ✅ Update all imports

### Phase 2: Error Handling (Week 2)
1. Create neo-error crate
2. Define common error types
3. Migrate crates to use neo-error

### Phase 3: Cleanup (Week 3)
1. Consolidate KeyBuilder
2. Move shared enums to neo-primitives
3. Remove dead code

---

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| Breaking API changes | Use type aliases for backward compatibility |
| Circular dependencies | Careful dependency ordering |
| Test failures | Run full test suite after each change |
| Performance regression | Benchmark critical paths |

---

## Expected Outcomes

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| Total Lines | ~150,000 | ~147,000 | -2% |
| Duplicate Code | ~3,000 | ~500 | -83% |
| Crate Dependencies | Complex | Simplified | Cleaner |
| Build Time | Baseline | -5-10% | Faster |

---

## Files to Delete After Refactoring

```
neo-core/src/uint160.rs          # 631 lines
neo-core/src/uint256.rs          # 587 lines
neo-p2p/src/witness_scope.rs     # 411 lines
```

## Files to Modify

```
neo-primitives/src/uint160.rs    # Merge features from neo-core
neo-primitives/src/uint256.rs    # Merge features from neo-core
neo-primitives/src/lib.rs        # Add WitnessScope export
neo-core/src/lib.rs              # Update re-exports
neo-p2p/src/lib.rs               # Update imports
```

---

## Conclusion

This refactoring plan will:
1. **Remove ~2,500+ lines** of duplicate code
2. **Simplify the dependency graph**
3. **Improve maintainability**
4. **Reduce cognitive load** for contributors

The changes are low-risk as they primarily involve moving code, not changing logic.
