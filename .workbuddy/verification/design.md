# Neo N3 Rust-vs-C# Semantic Verification Plan

## Goal
Verify 100% protocol compatibility between Rust node (`neo-rs`) and C# reference node (v3.10.0), crate-by-crate, file-by-file, struct-by-struct, method-by-method.

## Methodology
For each crate layer, verify 4 dimensions:
1. **Struct definitions** — fields, types, defaults match C# exactly
2. **Method behavior** — return values, side effects, error conditions match C#
3. **Serialization** — wire format byte-exact (for consensus-critical types)
4. **Boundary conditions** — edge cases, error paths, hardfork gating

## Verification Layers (bottom-up)

### Layer 1: Foundation (neo-primitives, neo-io, neo-error, neo-config)
- UInt160/UInt256: byte layout, hash_code(), arithmetic
- Hardfork enum: variant order, FromStr, activation heights
- IO traits: serialization helpers, varint encoding
- Config: protocol settings match C# config.json

### Layer 2: Crypto (neo-crypto)
- Hash functions: SHA256, RIPEMD160, hash160, hash256
- ECC: ECPoint encode/decode, ECDSA sign/verify
- BLS12-381: key operations
- MPT: trie insert/delete/root hash
- Bloom filter: seed derivation

### Layer 3: Infrastructure (neo-storage, neo-vm, neo-serialization, neo-manifest)
- Storage: DataCache, CloneCache, StorageItem serialization
- VM: opcode execution, stack operations, gas metering
- Serialization: BinarySerializer, JSON serializer, codecs
- Manifest: NEF file format, ContractManifest validation

### Layer 4: Protocol (neo-payloads, neo-consensus, neo-hsm)
- Block/Header/Transaction: hash computation, serialization
- Signers/Witnesses: script format, verification
- dBFT 2.0: prepare/commit/change-view messages
- ExtensiblePayload: category-based payloads

### Layer 5: Domain Service (neo-execution, neo-native-contracts, neo-state-service, neo-runtime, neo-mempool)
- ApplicationEngine: execution pipeline, gas metering, triggers
- Native Contracts: all 11 contracts, hardfork gating, storage layout
- State Root: MPT computation, state service, commit handlers
- Mempool: transaction admission, conflict detection, fee ordering

### Layer 6: Node Service (neo-blockchain, neo-network, neo-wallets, neo-indexer)
- Blockchain: native persist pipeline, block import, state transitions
- Network: P2P message format, handshake, block relay
- Wallets: NEP-2/NEP-6, key derivation, signing
- Indexer: event persistence, query interface

### Layer 7: Application (neo-node, neo-rpc, neo-system, neo-oracle-service)
- Node: daemon startup, service composition, telemetry
- RPC: JSON-RPC methods, response format (55 methods)
- System: service wiring, configuration loading
- Oracle: HTTPS requests, NeoFS, response verification

## Agent Assignment
- Agent A: Layers 1-2 (Foundation + Crypto) — ~15 files
- Agent B: Layer 3 (Infrastructure) — ~12 files
- Agent C: Layer 4 (Protocol) — ~10 files
- Agent D: Layer 5 (Domain Service) — ~20 files
- Agent E: Layers 6-7 (Node/App Service) — ~15 files

Each agent produces: verified_items, divergences (critical/high/medium/low), untestable_items.

## Verification Criteria
- **CRITICAL**: byte-exact serialization mismatch, consensus divergence, state root difference
- **HIGH**: missing C# method/field, incorrect hardfork gating, wrong error behavior
- **MEDIUM**: performance difference, error handling gap, naming inconsistency
- **LOW**: documentation gap, code organization, cosmetic

## Reference Sources
- claudedocs/REVIEW-2026-07-03-v0.9.0.md — verified-known state
- claudedocs/spec-v3100-parity-findings.md — 116 spec divergences
- claudedocs/node-readiness-audit-2026-06-11.md — node readiness
- docs/protocol-compatibility.md — parity claims
- neo_csharp/ — C# reference JSON configs/test cases
