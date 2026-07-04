# Infrastructure Layer (L1) Architecture Documentation

## Overview

The Infrastructure Layer (L1) in neo-rs consists of 9 crates that sit between the Foundation Layer (L0: `neo-primitives`) and the Protocol Layer (L2: `neo-payloads`, `neo-consensus`, `neo-hsm`).

While originally treated as a single flat layer, dependency analysis reveals three distinct sub-layers with clear directional dependencies. This document describes the sub-layer structure, the rationale for each grouping, and the rules for adding new crates.

---

## Sub-Layer Structure

```
┌─────────────────────────────────────────────────────────────────┐
│ L1c  Cross-Cutting    neo-config · neo-static-files              │
├─────────────────────────────────────────────────────────────────┤
│ L1b  Stateful Infra   neo-crypto · neo-storage · neo-vm          │
│                        neo-serialization · neo-manifest           │
├─────────────────────────────────────────────────────────────────┤
│ L1a  Core Infra       neo-io · neo-error                         │
├─────────────────────────────────────────────────────────────────┤
│ L0  Foundation        neo-primitives                             │
└─────────────────────────────────────────────────────────────────┘
```

**Dependency rule**: Each sub-layer may only depend downward (L1b → L1a → L0), never sideways within the same sub-layer or upward.

---

## L1a: Core Infrastructure

These crates provide fundamental abstractions used by every other crate in the workspace. They depend only on `neo-primitives` (L0).

### Crates

| Crate | Purpose | Dependencies (internal) |
|-------|---------|------------------------|
| `neo-io` | Binary I/O traits, `Codec`, `IReadable`, `IWritable`, error types | `neo-primitives` |
| `neo-error` | `CoreError`, `CoreResult<T>`, error conversion macros | `neo-primitives`, `neo-io` |

### Design Principles

- **Zero stateful dependencies**: These crates contain no storage, no crypto, no VM — only types and traits.
- **Stable API**: Changes here ripple to every crate. Breaking changes require workspace-wide coordination.
- **`neo-error` depends on `neo-io`** for the `impl_error_from!` macro (re-exported at `neo_error::impl_error_from`). This is intentional — the macro is a generic helper, not I/O-specific.

### Why `neo-error` is in L1a (not L0)

`CoreError` depends on `PrimitiveError` (from `neo-primitives`) and `IoError` (from `neo-io`). It sits above both but below everything else, making it the universal error vocabulary for the workspace.

---

## L1b: Stateful Infrastructure

These crates provide concrete implementations of storage, cryptography, virtual machine, and serialization. They depend on L1a and L0, and may depend on each other within the sub-layer (following the dependency chain below).

### Crates

| Crate | Purpose | Dependencies (internal) |
|-------|---------|------------------------|
| `neo-crypto` | Hashing (SHA-256, RIPEMD-160, Keccak), ECC (secp256k1, secp256r1), BLS, signatures | `neo-primitives`, `neo-io`, `neo-error` |
| `neo-storage` | Key-value store backends (RocksDB, MDBX, in-memory), `Store`, `DataCache`, `StoreCache` | `neo-primitives`, `neo-error`, `neo-io` |
| `neo-vm` | NeoVM host (opcodes, stack, execution engine), wraps `neo-vm-rs` | `neo-vm-rs`, `neo-io`, `neo-primitives`, `neo-crypto`, `neo-error` |
| `neo-serialization` | Binary and JSON codecs for Neo types, `ISerializable` | `neo-primitives`, `neo-error`, `neo-storage`, `neo-io`, `neo-vm-rs`, `neo-vm` |
| `neo-manifest` | Contract ABI (`ContractManifest`), NEF format (`NeoFileHeader`) | `neo-primitives`, `neo-error`, `neo-io`, `neo-crypto`, `neo-vm-rs`, `neo-vm`, `neo-serialization` |

### Internal Dependency Chain (L1b)

```
neo-crypto ──────────────────────────────────────────┐
                                                      │
neo-storage ─────────────────────────────────────────┤
                                                      │
neo-vm ─────── (depends on neo-crypto) ──────────────┤
                                                      │
neo-serialization ── (depends on neo-storage, neo-vm)┤
                                                      │
neo-manifest ──────── (depends on neo-crypto, neo-vm,│
                       neo-serialization) ────────────┘
```

**Key observations**:
- `neo-crypto` and `neo-storage` are independent of each other (no sideways dependency)
- `neo-vm` depends on `neo-crypto` (for hash/verify opcodes) but NOT on `neo-storage`
- `neo-serialization` depends on both `neo-storage` (for store-backed codecs) and `neo-vm` (for `StackItem` serialization)
- `neo-manifest` is the "heaviest" L1b crate — it depends on crypto, VM, and serialization

### Design Principles

- **Implementation crates**: These contain real algorithms and storage engines, not just traits.
- **Error conversion**: Each crate that defines its own error type provides `impl From<LocalError> for neo_error::CoreError` (see `neo-storage`, `neo-vm`).
- **No upward dependencies**: L1b crates never import from L2+ crates (protocol, domain service, etc.)

---

## L1c: Cross-Cutting Infrastructure

These crates are infrastructure-level but don't fit neatly into the core/stateful split. They serve specialized purposes and have minimal internal dependencies.

### Crates

| Crate | Purpose | Dependencies (internal) |
|-------|---------|------------------------|
| `neo-config` | Configuration types (`ProtocolSettings`, `NetworkSection`, `LoggingSection`) | `neo-primitives`, `neo-crypto` |
| `neo-static-files` | Append-only file storage for cold data (block headers, transactions) | None (standalone) |

### Design Principles

- **`neo-config`** depends on `neo-crypto` for address/hash types used in configuration, but is otherwise standalone. It is consumed by nearly every crate above L1.
- **`neo-static-files`** is the most independent crate in L1 — it has zero internal workspace dependencies. It provides file-based storage for index data that doesn't need a key-value store.

---

## Adding a New L1 Crate

When adding a new infrastructure crate, determine its sub-layer:

1. **L1a (Core Infra)**: If the crate provides fundamental types, traits, or error handling with no stateful logic. Must depend only on `neo-primitives` and/or `neo-io`.
2. **L1b (Stateful Infra)**: If the crate provides concrete implementations (algorithms, storage engines, codecs). May depend on L1a and other L1b crates.
3. **L1c (Cross-cutting)**: If the crate provides configuration, utilities, or specialized storage that doesn't fit the core/stateful dichotomy.

**Checklist for new L1 crates**:
- [ ] No dependencies on L2+ crates (protocol, domain service, node service, composition, application)
- [ ] Error types implement `From<LocalError> for neo_error::CoreError`
- [ ] Uses `workspace = true` for all external dependencies
- [ ] Has `//!` module-level documentation following the standard pattern
- [ ] Respects the dependency direction (only depends downward)

---

## Comparison with reth's Storage Split

reth splits its infrastructure layer into many more crates:

| reth crate | neo-rs equivalent | Notes |
|------------|-------------------|-------|
| `reth-db-api` | (part of `neo-storage`) | reth separates DB traits from implementation |
| `reth-db` | (part of `neo-storage`) | reth separates MDBX backend |
| `reth-storage-api` | (part of `neo-storage`) | reth separates high-level storage traits |
| `reth-provider` | (part of `neo-storage`) | reth separates the provider implementation |
| `reth-codecs` | (part of `neo-serialization`) | reth separates encoding/decoding |
| `reth-errors` | `neo-error` | Similar concept |
| `reth-primitives` | `neo-primitives` | Similar concept |

neo-rs intentionally keeps storage in a single crate (`neo-storage`) for simplicity. The trade-off: easier to maintain with fewer crates, but downstream consumers must depend on the full storage stack rather than just traits. If neo-rs grows significantly, splitting `neo-storage` into `neo-storage-api` (traits) + `neo-storage` (implementation) would be the first recommended step.

---

## Summary

The L1 layer is well-structured with clear dependency direction. The three sub-layers (Core Infra, Stateful Infra, Cross-cutting) make the dependency graph explicit and prevent accidental upward dependencies. The main area for future improvement is potential splitting of `neo-storage` into API and implementation crates, following reth's pattern.
