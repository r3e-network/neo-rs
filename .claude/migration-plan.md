# Neo-RS Full Refactoring Migration Plan

## Executive Summary

This document outlines the complete refactoring plan to transform the current Neo-RS project into a production-grade Neo N3 full node implementation following professional Rust architecture patterns.

## Current State Analysis

### Existing Crate Structure (18 crates)
```
neo-primitives    - Foundation: UInt160, UInt256, BigDecimal
neo-crypto        - Cryptography: ECDSA, hashing
neo-io            - IO serialization
neo-json          - JSON handling
neo-storage       - Storage abstractions
neo-vm            - NeoVM implementation
neo-core          - Core functionality (massive, needs splitting)
neo-contract      - Smart contract execution
neo-p2p           - P2P networking types
neo-consensus     - dBFT consensus types
neo-akka          - Custom actor runtime (TO BE REPLACED)
neo-services      - Service traits
neo-rpc           - RPC types
neo-rpc-client    - RPC client
neo-plugins       - Plugin system (TO BE REMOVED)
neo-tee           - TEE support (OPTIONAL)
neo-cli           - CLI client
neo-node          - Node daemon
```

### Key Issues Identified
1. **neo-akka**: Self-implemented actor runtime - needs professional replacement
2. **neo-plugins**: Plugin system adds complexity - needs inlining
3. **neo-core**: Monolithic, contains too much - needs splitting
4. **Circular dependencies**: Some crates have tight coupling
5. **Missing crates**: No dedicated mempool, chain, config, telemetry

---

## Target Architecture

### Layer 1: Foundation (No async runtime dependencies)
```
neo-primitives   - UInt160, UInt256, BigDecimal, basic types
neo-crypto       - ECDSA, sha256, ripemd160, KeyPair
neo-io           - VarInt, VarString, VarBytes, Serializable trait
```

### Layer 2: Storage
```
neo-storage      - RocksDB wrapper, BlockStore, ChainStore, StateStore traits
```

### Layer 3: Core (Data structures, no tokio)
```
neo-vm           - NeoVM: opcode, stack, context, engine, syscall, gas
neo-core         - Block, Transaction, Script, Witness, native contracts
```

### Layer 4: Network & Consensus (async)
```
neo-p2p          - Protocol messages, codec, peer management, discovery
neo-consensus    - dBFT algorithm, consensus messages, validator set
```

### Layer 5: Chain Management (async)
```
neo-mempool      - NEW: Transaction pool, fee policy, validation queue
neo-chain        - NEW: Chain controller, block validation, fork choice, reorg
```

### Layer 6: Services (async)
```
neo-rpc          - Axum-based JSON-RPC server (inline from plugins)
neo-telemetry    - NEW: Logging, metrics, health checks
```

### Layer 7: Configuration & Application
```
neo-config       - NEW: Settings, network params, genesis loader
neo-node         - Main binary, CLI entry, app assembly
```

---

## Migration Actions

### CRATES TO CREATE (NEW)
| Crate | Purpose | Dependencies |
|-------|---------|--------------|
| neo-mempool | Transaction pool management | neo-core, neo-storage |
| neo-chain | Blockchain state machine | neo-core, neo-storage, neo-mempool, neo-consensus |
| neo-config | Configuration management | neo-primitives, serde, toml |
| neo-telemetry | Observability stack | tracing, prometheus |

### CRATES TO KEEP (WITH MODIFICATIONS)
| Crate | Modifications |
|-------|---------------|
| neo-primitives | Clean up, ensure no async dependencies |
| neo-crypto | Consolidate all crypto, remove from neo-core |
| neo-io | Ensure pure serialization, no side effects |
| neo-storage | Add RocksDB CF support, improve StateStore |
| neo-vm | Keep as-is, minimal changes |
| neo-core | MAJOR: Split out services, remove akka |
| neo-p2p | Add peer discovery, improve codec |
| neo-consensus | Integrate dBFT from plugins |
| neo-rpc | Integrate RpcServer from plugins, use axum |
| neo-node | Assemble all components |

### CRATES TO REMOVE
| Crate | Reason | Migration |
|-------|--------|-----------|
| neo-akka | Replace with ractor | neo-core/src/akka/ already inlined, replace with ractor |
| neo-plugins | Inline all functionality | Move to respective service crates |
| neo-json | Merge into neo-io | serde_json is sufficient |
| neo-contract | Merge into neo-core | Too thin, consolidate |
| neo-services | Inline into specific crates | Abstractions moved to consumers |
| neo-tee | Optional, exclude | Can be re-added later |
| neo-rpc-client | Keep or deprecate | May keep for testing |
| neo-cli | Merge into neo-node | Single binary strategy |

### CRATES TO MERGE
| Source | Target | Rationale |
|--------|--------|-----------|
| neo-json | neo-io | Both handle serialization |
| neo-contract | neo-core | Natural fit |
| neo-cli | neo-node | Single entry point |

---

## Actor System Replacement Strategy

### Current: neo-akka (custom implementation)
- Located in: `neo-core/src/akka/`
- Components: Actor, ActorRef, ActorSystem, Mailbox, Props, Supervisor

### Target: ractor
- Professional Rust actor framework
- Excellent tokio integration
- Supervision trees support
- Type-safe message passing

### Migration Steps
1. Add `ractor = "0.9"` to workspace dependencies
2. Create adapter layer: `neo-core/src/actors/mod.rs`
3. Define actor traits compatible with current interface
4. Migrate actors one-by-one:
   - LocalNode → PeerManager actor
   - TaskManager → TaskCoordinator actor
   - ConsensusService → ConsensusActor
5. Remove `neo-core/src/akka/` after full migration

---

## Plugin Inlining Strategy

### Plugin: ApplicationLogs
- Current location: `neo-plugins/src/application_logs/`
- Target: `neo-core/src/services/application_logs/`
- Changes: Direct integration, no plugin interface

### Plugin: TokensTracker
- Current location: `neo-plugins/src/tokens_tracker/`
- Target: `neo-core/src/services/token_tracker/`
- Changes: NEP-11/NEP-17 tracking integrated into chain events

### Plugin: RpcServer
- Current location: `neo-plugins/src/rpc_server/`
- Target: `neo-rpc/src/` (entire crate)
- Changes: Convert to standalone axum service

### Plugin: dBFT (ConsensusService)
- Current location: `neo-plugins/src/dbft_plugin/`
- Target: `neo-consensus/src/`
- Changes: Full integration into consensus crate

### Plugin: RocksDBStore
- Current location: `neo-plugins/src/rocksdb_store/`
- Target: `neo-storage/src/providers/rocksdb/`
- Changes: Make RocksDB the primary storage backend

---

## Dependency Ordering for Safe Migration

### Phase 1: Foundation (no breaking changes)
1. Clean neo-primitives
2. Clean neo-crypto
3. Clean neo-io
4. Enhance neo-storage with RocksDB CF

### Phase 2: Core (internal refactoring)
1. Merge neo-contract into neo-core
2. Remove neo-json (use serde_json directly)
3. Split services out of neo-core

### Phase 3: New Crates
1. Create neo-config
2. Create neo-telemetry
3. Create neo-mempool
4. Create neo-chain

### Phase 4: Service Migration
1. Move RpcServer to neo-rpc
2. Move dBFT to neo-consensus
3. Move TokensTracker to neo-core
4. Move ApplicationLogs to neo-core

### Phase 5: Actor Replacement
1. Add ractor dependency
2. Create actor adapters
3. Migrate actors incrementally
4. Remove neo-akka code

### Phase 6: Assembly
1. Update neo-node to use new structure
2. Merge neo-cli into neo-node
3. Remove neo-plugins, neo-akka crates

---

## Workspace Cargo.toml Target State

```toml
[workspace]
resolver = "2"
members = [
    # Foundation Layer
    "neo-primitives",
    "neo-crypto",
    "neo-io",
    "neo-storage",

    # Core Layer
    "neo-vm",
    "neo-core",

    # Network Layer
    "neo-p2p",
    "neo-consensus",

    # Chain Management
    "neo-mempool",
    "neo-chain",

    # Services
    "neo-rpc",
    "neo-telemetry",

    # Configuration
    "neo-config",

    # Application
    "neo-node",
]
```

---

## Risk Assessment

### High Risk
- Actor system replacement: Complex migration, potential runtime bugs
- Plugin removal: Features may break during transition

### Medium Risk
- neo-core splitting: May introduce temporary compilation errors
- Storage changes: Data migration considerations

### Low Risk
- New crate creation: Additive changes
- Dependency cleanup: Straightforward removal

---

## Success Criteria

1. All tests pass after each phase
2. No regression in blockchain synchronization
3. RPC API compatibility with Neo N3 C# reference
4. Performance: Block processing < 100ms
5. Clean dependency graph with no cycles
6. All plugin functionality available without plugin system

---

## Timeline Estimate

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| Phase 1 | 2-3 days | None |
| Phase 2 | 3-4 days | Phase 1 |
| Phase 3 | 4-5 days | Phase 2 |
| Phase 4 | 5-7 days | Phase 3 |
| Phase 5 | 5-7 days | Phase 4 |
| Phase 6 | 2-3 days | Phase 5 |
| **Total** | **21-29 days** | Sequential |

---

## Next Steps

1. Review and approve this migration plan
2. Create feature branch: `refactor/architecture-v2`
3. Begin Phase 1: Foundation layer cleanup
4. Establish CI gates for each phase completion
