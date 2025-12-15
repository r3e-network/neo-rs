# Neo-RS Crate Architecture Audit Report

**Date:** 2025-12-15
**Auditor:** Claude Code
**Scope:** Full workspace audit (18 crates)

---

## Executive Summary

| Category                | Count | Severity |
| ----------------------- | ----- | -------- |
| Layering Violations     | 3     | CRITICAL |
| Duplicate Functionality | 4     | HIGH     |
| Clean Crates            | 14    | OK       |

---

## 1. Workspace Architecture

### Expected Layer Hierarchy

```
Layer 5 (Application):  neo-node, neo-cli, neo-tee
Layer 4 (Services):     neo-rpc, neo-config, neo-telemetry
Layer 3 (State):        neo-state, neo-chain
Layer 2 (Protocol):     neo-p2p, neo-consensus, neo-mempool
Layer 1 (Core):         neo-core, neo-vm
Layer 0 (Foundation):   neo-primitives, neo-crypto, neo-io, neo-json, neo-storage
```

**Rule:** Lower layers MUST NOT depend on higher layers.

---

## 2. Layering Violations

### 2.1 CRITICAL: neo-core depends on Layer 2 crates

**Issue:** `neo-core/Cargo.toml` imports `neo-p2p` and `neo-consensus`

```toml
# neo-core/Cargo.toml
neo-p2p = { workspace = true }
neo-consensus = { workspace = true }
```

**Impact:**

- Creates circular dependency risk
- Violates single responsibility principle
- Makes neo-core impossible to use without protocol layer

**Fix:** Extract protocol integration into neo-node or create trait abstractions.

---

### 2.2 HIGH: neo-mempool depends on neo-config (Layer 4)

**Issue:** `neo-mempool/Cargo.toml` imports `neo-config`

**Impact:**

- Layer 2 depends on Layer 4
- Violates dependency inversion principle
- Configuration should be injected, not imported

**Fix:** Accept `MempoolConfig` as constructor parameter instead of importing neo-config.

---

### 2.3 HIGH: neo-chain depends on neo-config (Layer 4)

**Issue:** `neo-chain/Cargo.toml` imports `neo-config`

**Impact:**

- Layer 3 depends on Layer 4
- Same violation pattern as neo-mempool

**Fix:** Accept configuration as constructor parameter.

---

## 3. Duplicate Functionality

### 3.1 CRITICAL: Hash Functions (3 locations)

| Function    | neo-crypto/hash.rs | neo-crypto/crypto_utils.rs | neo-core/lib.rs |
| ----------- | ------------------ | -------------------------- | --------------- |
| sha256()    | ✓ Crypto::sha256   | ✓ NeoHash::sha256          | ✓ inline        |
| ripemd160() | ✓                  | ✓                          | ✓               |
| hash160()   | ✓                  | ✓                          | -               |
| hash256()   | ✓                  | ✓                          | -               |

**Canonical:** `neo-crypto/src/crypto_utils.rs` (NeoHash)

**Fix:**

1. Remove `Crypto` struct from hash.rs (keep only NeoHash)
2. Remove inline implementations from neo-core/lib.rs
3. Re-export NeoHash as primary API

---

### 3.2 HIGH: Storage Traits (2 locations)

| Trait          | neo-storage/traits.rs | neo-core/persistence/ |
| -------------- | --------------------- | --------------------- |
| IReadOnlyStore | ✓                     | ✓ (separate file)     |
| IWriteStore    | ✓                     | ✓ (separate file)     |
| IStore         | ✓                     | ✓ (separate file)     |
| ISnapshot      | ✓                     | ✓ (separate file)     |

**Canonical:** `neo-storage/src/traits.rs`

**Fix:**

1. Remove trait definitions from neo-core/persistence/
2. Re-export from neo-storage in neo-core

---

### 3.3 MEDIUM: Binary I/O Split

| Component    | Location                         |
| ------------ | -------------------------------- |
| BinaryWriter | neo-io/src/binary_writer.rs      |
| BinaryReader | neo-core/src/io/binary_reader.rs |

**Fix:** Move BinaryReader to neo-io for consistency.

---

### 3.4 LOW: MessageCommand (Intentional)

| Location                                    | Purpose                        |
| ------------------------------------------- | ------------------------------ |
| neo-p2p/src/message_command.rs              | Lightweight for external tools |
| neo-core/src/network/p2p/message_command.rs | Full-featured with docs        |

**Status:** Acceptable - intentional separation for dependency management.

---

## 4. Crate Status Summary

| Crate          | Layer | Status       | Issues                    |
| -------------- | ----- | ------------ | ------------------------- |
| neo-primitives | 0     | ✅ CLEAN     | -                         |
| neo-io         | 0     | ✅ CLEAN     | -                         |
| neo-json       | 0     | ✅ CLEAN     | -                         |
| neo-crypto     | 0     | ⚠️ DUPLICATE | Hash function duplication |
| neo-storage    | 0     | ✅ CLEAN     | -                         |
| neo-vm         | 1     | ✅ CLEAN     | -                         |
| neo-core       | 1     | ❌ VIOLATION | Depends on Layer 2        |
| neo-p2p        | 2     | ✅ CLEAN     | -                         |
| neo-consensus  | 2     | ✅ CLEAN     | -                         |
| neo-mempool    | 2     | ⚠️ VIOLATION | Depends on Layer 4        |
| neo-state      | 3     | ✅ CLEAN     | -                         |
| neo-chain      | 3     | ⚠️ VIOLATION | Depends on Layer 4        |
| neo-config     | 4     | ✅ CLEAN     | -                         |
| neo-rpc        | 4     | ✅ CLEAN     | Feature-gated deps OK     |
| neo-telemetry  | 4     | ✅ CLEAN     | -                         |
| neo-node       | 5     | ✅ CLEAN     | -                         |
| neo-cli        | 5     | ✅ CLEAN     | -                         |
| neo-tee        | 5     | ✅ CLEAN     | -                         |

---

## 5. Recommended Fixes (Priority Order)

### P0 - Critical (Block Release)

1. **Remove neo-p2p/neo-consensus from neo-core dependencies**
    - Extract NeoSystem to neo-node
    - Use trait abstractions for protocol integration

2. **Consolidate hash functions**
    - Single implementation in neo-crypto
    - Remove duplicates from neo-core

### P1 - High (Next Sprint)

3. **Remove neo-config from neo-mempool**
    - Accept MempoolConfig as parameter
    - Inject configuration at runtime

4. **Remove neo-config from neo-chain**
    - Same pattern as neo-mempool

5. **Consolidate storage traits**
    - neo-storage is canonical
    - neo-core re-exports only

### P2 - Medium (Backlog)

6. **Move BinaryReader to neo-io**
    - Consolidate serialization in one crate

7. **Document intentional duplications**
    - MessageCommand separation rationale

---

## 6. Verification Commands

```bash
# Check for circular dependencies
cargo tree --workspace -d | grep -E "neo-.*neo-"

# Verify no Layer 0 crate depends on higher layers
cargo tree -p neo-primitives --no-dedupe | grep -E "neo-(core|vm|p2p|consensus)"

# Run all tests after fixes
cargo test --workspace
```

---

## Appendix: Dependency Graph

```
neo-node
├── neo-core
│   ├── neo-primitives
│   ├── neo-crypto
│   │   ├── neo-primitives
│   │   └── neo-io
│   ├── neo-io
│   ├── neo-storage
│   │   └── neo-primitives
│   ├── neo-p2p          ← VIOLATION (should not be here)
│   │   ├── neo-primitives
│   │   ├── neo-crypto
│   │   └── neo-io
│   └── neo-consensus    ← VIOLATION (should not be here)
│       ├── neo-primitives
│       ├── neo-crypto
│       └── neo-io
├── neo-chain
│   ├── neo-primitives
│   ├── neo-config       ← VIOLATION
│   └── neo-mempool
│       ├── neo-primitives
│       └── neo-config   ← VIOLATION
├── neo-state
│   ├── neo-primitives
│   └── neo-storage
└── neo-rpc
    ├── neo-primitives
    └── neo-crypto
```
