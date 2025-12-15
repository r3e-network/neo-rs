# NEO-RS Complete Refactoring Plan

## Executive Summary

Complete refactoring of neo-rs full node to match production-grade architecture:
- **Target**: 14 crates (neo-xxx naming convention)
- **Plugin System**: Remove entirely, inline all functionality
- **Storage**: RocksDB only (keep MemoryStore for tests)
- **Test Coverage**: ≥90%
- **Total LOC**: ~150,000 lines to refactor

---

## 1. Current State Analysis

### 1.1 Existing Crates (18 total, ~150K LOC)

| Crate | Rust Files | LOC | Status |
|-------|-----------|-----|--------|
| neo-core | 374 | 74,131 | Major refactor |
| neo-vm | 120 | 27,462 | Refactor |
| neo-plugins | 101 | 18,470 | **DELETE** (inline) |
| neo-rpc-client | 55 | 8,909 | Merge into neo-rpc |
| neo-io | 28 | 3,919 | Keep |
| neo-primitives | 11 | 2,711 | Keep |
| neo-tee | 15 | 2,540 | **DELETE** (optional) |
| neo-akka | 14 | 2,293 | Merge into neo-core |
| neo-node | 5 | 2,178 | Refactor |
| neo-json | 22 | 1,877 | Keep |
| neo-p2p | 12 | 1,562 | Refactor |
| neo-cli | 29 | 1,546 | Refactor |
| neo-crypto | 5 | 1,349 | Keep |
| neo-contract | 9 | 1,072 | Merge into neo-vm |
| neo-storage | 5 | 773 | Refactor |
| neo-consensus | 4 | 374 | Major refactor |
| neo-rpc | 3 | 365 | Major refactor |
| neo-services | 1 | 150 | **DELETE** (inline) |

### 1.2 Plugin Functionality to Inline

| Plugin | LOC | Target Crate | Priority |
|--------|-----|--------------|----------|
| dbft_plugin | ~3,000 | neo-consensus | Critical |
| rpc_server | ~5,000 | neo-rpc | Critical |
| application_logs | ~2,000 | neo-chain | Medium |
| tokens_tracker | ~3,000 | neo-chain | Medium |
| sqlite_wallet | ~500 | neo-core/wallets | Low |
| rocksdb_store | ~100 | **DELETE** (no-op) | N/A |

---

## 2. Target Architecture

### 2.1 Target Crates (14 total)

```
neo-rs/
├── Cargo.toml                    # Workspace
├── rust-toolchain.toml           # Rust 1.80+
│
├── neo-primitives/               # UInt160, UInt256, BigDecimal (KEEP)
├── neo-crypto/                   # Cryptography (KEEP)
├── neo-io/                       # Serialization (KEEP)
├── neo-storage/                  # Storage traits + RocksDB impl (REFACTOR)
│
├── neo-core/                     # Core protocol (MAJOR REFACTOR)
├── neo-vm/                       # NeoVM (REFACTOR, absorb neo-contract)
│
├── neo-state/                    # World state (NEW, extract from neo-core)
├── neo-mempool/                  # Transaction pool (NEW, extract from neo-core)
├── neo-chain/                    # Blockchain logic (NEW, extract from neo-core)
│
├── neo-p2p/                      # P2P networking (REFACTOR)
├── neo-consensus/                # dBFT consensus (MAJOR REFACTOR)
├── neo-rpc/                      # RPC server (MAJOR REFACTOR)
│
├── neo-config/                   # Configuration (NEW)
├── neo-telemetry/                # Logging, metrics (NEW)
├── neo-node/                     # Node daemon (REFACTOR)
└── neo-cli/                      # CLI tools (REFACTOR)
```

### 2.2 Dependency Graph

```
Layer 0 (Foundation - no neo-* deps):
  neo-primitives
  neo-crypto
  neo-io

Layer 1 (Storage):
  neo-storage → neo-primitives

Layer 2 (Core):
  neo-vm → neo-primitives, neo-crypto, neo-io
  neo-core → neo-primitives, neo-crypto, neo-io, neo-storage, neo-vm

Layer 3 (State):
  neo-state → neo-core, neo-storage
  neo-mempool → neo-core, neo-state
  neo-chain → neo-core, neo-state, neo-mempool, neo-vm

Layer 4 (Network):
  neo-p2p → neo-core, neo-io
  neo-consensus → neo-core, neo-chain, neo-p2p

Layer 5 (Services):
  neo-rpc → neo-core, neo-chain, neo-vm
  neo-config → neo-primitives
  neo-telemetry → (minimal deps)

Layer 6 (Application):
  neo-node → ALL
  neo-cli → neo-core, neo-rpc, neo-config
```

---

## 3. Refactoring Phases

### Phase 1: Foundation Layer (Week 1)
**Goal**: Stabilize foundation crates, no breaking changes

| Task ID | Description | Files | Test Command |
|---------|-------------|-------|--------------|
| F1.1 | Keep neo-primitives as-is | neo-primitives/* | `cargo test -p neo-primitives` |
| F1.2 | Keep neo-crypto as-is | neo-crypto/* | `cargo test -p neo-crypto` |
| F1.3 | Keep neo-io as-is | neo-io/* | `cargo test -p neo-io` |
| F1.4 | Refactor neo-storage: RocksDB only | neo-storage/* | `cargo test -p neo-storage` |

**Deliverables**:
- [ ] neo-storage with RocksDB as default, MemoryStore for tests
- [ ] Remove sled dependency
- [ ] ≥90% coverage on storage traits

---

### Phase 2: Core Layer (Week 2-3)
**Goal**: Refactor neo-core, merge neo-akka, absorb neo-contract into neo-vm

| Task ID | Description | Files | Test Command |
|---------|-------------|-------|--------------|
| C2.1 | Merge neo-akka actor system into neo-core | neo-core/src/actor/* | `cargo test -p neo-core` |
| C2.2 | Remove plugin system from neo-core | neo-core/src/extensions/* | `cargo test -p neo-core` |
| C2.3 | Merge neo-contract into neo-vm | neo-vm/src/contract/* | `cargo test -p neo-vm` |
| C2.4 | Refactor neo-vm execution engine | neo-vm/src/* | `cargo test -p neo-vm` |

**Deliverables**:
- [ ] neo-core without plugin system
- [ ] neo-vm with contract execution
- [ ] Actor system integrated
- [ ] ≥90% coverage

---

### Phase 3: State Extraction (Week 4)
**Goal**: Extract state management into dedicated crates

| Task ID | Description | Files | Test Command |
|---------|-------------|-------|--------------|
| S3.1 | Create neo-state crate | neo-state/* | `cargo test -p neo-state` |
| S3.2 | Extract mempool to neo-mempool | neo-mempool/* | `cargo test -p neo-mempool` |
| S3.3 | Create neo-chain crate | neo-chain/* | `cargo test -p neo-chain` |
| S3.4 | Inline application_logs into neo-chain | neo-chain/src/logs/* | `cargo test -p neo-chain` |
| S3.5 | Inline tokens_tracker into neo-chain | neo-chain/src/tokens/* | `cargo test -p neo-chain` |

**Deliverables**:
- [ ] neo-state: world state, snapshots, rollbacks
- [ ] neo-mempool: transaction pool
- [ ] neo-chain: blockchain logic, logs, token tracking
- [ ] ≥90% coverage

---

### Phase 4: Network Layer (Week 5)
**Goal**: Refactor P2P and consensus

| Task ID | Description | Files | Test Command |
|---------|-------------|-------|--------------|
| N4.1 | Refactor neo-p2p | neo-p2p/* | `cargo test -p neo-p2p` |
| N4.2 | Inline dbft_plugin into neo-consensus | neo-consensus/* | `cargo test -p neo-consensus` |
| N4.3 | Integrate consensus with chain | neo-consensus/src/* | `cargo test -p neo-consensus` |

**Deliverables**:
- [ ] neo-p2p: peer management, message handling
- [ ] neo-consensus: full dBFT implementation
- [ ] ≥90% coverage

---

### Phase 5: Services Layer (Week 6)
**Goal**: Refactor RPC, create config and telemetry crates

| Task ID | Description | Files | Test Command |
|---------|-------------|-------|--------------|
| R5.1 | Inline rpc_server into neo-rpc | neo-rpc/* | `cargo test -p neo-rpc` |
| R5.2 | Merge neo-rpc-client into neo-rpc | neo-rpc/src/client/* | `cargo test -p neo-rpc` |
| R5.3 | Create neo-config crate | neo-config/* | `cargo test -p neo-config` |
| R5.4 | Create neo-telemetry crate | neo-telemetry/* | `cargo test -p neo-telemetry` |

**Deliverables**:
- [ ] neo-rpc: server + client
- [ ] neo-config: configuration parsing
- [ ] neo-telemetry: logging, metrics, health
- [ ] ≥90% coverage

---

### Phase 6: Application Layer (Week 7)
**Goal**: Refactor node and CLI

| Task ID | Description | Files | Test Command |
|---------|-------------|-------|--------------|
| A6.1 | Refactor neo-node | neo-node/* | `cargo test -p neo-node` |
| A6.2 | Refactor neo-cli | neo-cli/* | `cargo test -p neo-cli` |
| A6.3 | Remove neo-plugins crate | DELETE | N/A |
| A6.4 | Remove neo-services crate | DELETE | N/A |
| A6.5 | Remove neo-tee crate (optional) | DELETE | N/A |

**Deliverables**:
- [ ] neo-node: standalone daemon
- [ ] neo-cli: command-line tools
- [ ] All deprecated crates removed
- [ ] ≥90% coverage

---

### Phase 7: Integration & Cleanup (Week 8)
**Goal**: Final integration, documentation, coverage validation

| Task ID | Description | Files | Test Command |
|---------|-------------|-------|--------------|
| I7.1 | Integration tests | tests/* | `cargo test --all` |
| I7.2 | Coverage validation | - | `cargo llvm-cov --all` |
| I7.3 | Update documentation | docs/* | N/A |
| I7.4 | Update Cargo.toml workspace | Cargo.toml | `cargo build --all` |

**Deliverables**:
- [ ] All integration tests passing
- [ ] ≥90% coverage verified
- [ ] Documentation updated
- [ ] Clean build with no warnings

---

## 4. Files to Delete

### 4.1 Entire Crates to Remove
```
neo-plugins/           # Inline all functionality
neo-services/          # Inline into neo-core
neo-tee/               # Optional, remove for now
neo-akka/              # Merge into neo-core
neo-contract/          # Merge into neo-vm
neo-rpc-client/        # Merge into neo-rpc
neo-json/              # Merge into neo-io (optional)
```

### 4.2 Files to Delete in neo-core
```
neo-core/src/extensions/plugin.rs      # Plugin system
neo-core/src/extensions/mod.rs         # Plugin exports
neo-core/src/plugins/                  # Plugin re-exports
```

### 4.3 Files Already Deleted (from git status)
```
neo-core/src/ledger/blockchain.rs
neo-core/src/network/p2p/local_node.rs
neo-core/src/network/p2p/payloads/transaction.rs
neo-core/src/network/u_pn_p.rs
neo-core/src/persistence/rocksdb_store.rs
neo-core/src/smart_contract/application_engine.rs
neo-core/src/smart_contract/native/contract_management.rs
neo-core/src/smart_contract/native/neo_token.rs
neo-core/src/smart_contract/native/policy_contract.rs
neo-plugins/src/rpc_server/rpc_server_smart_contract.rs
neo-plugins/src/tokens_tracker/stub.rs
neo-plugins/src/tokens_tracker/tokens_tracker.rs
neo-rpc-client/src/rpc_client.rs
neo-vm/src/execution_engine.rs
```

---

## 5. Risk Assessment

### 5.1 High Risk Areas
1. **Plugin System Removal**: Many components depend on PluginEvent
2. **Actor System Migration**: neo-akka used extensively in P2P
3. **Consensus Integration**: dBFT plugin is complex

### 5.2 Mitigation Strategies
1. Replace PluginEvent with direct callbacks/traits
2. Keep actor abstractions, just move code location
3. Thorough testing of consensus state machine

---

## 6. Success Criteria

- [ ] All 14 target crates compile without errors
- [ ] No circular dependencies
- [ ] ≥90% test coverage on all crates
- [ ] All existing tests pass
- [ ] Node can sync blocks from testnet
- [ ] RPC server responds to standard queries
- [ ] Documentation updated

---

## 7. Estimated Effort

| Phase | Duration | Parallelizable Tasks |
|-------|----------|---------------------|
| Phase 1: Foundation | 1 week | 4 |
| Phase 2: Core | 2 weeks | 4 |
| Phase 3: State | 1 week | 5 |
| Phase 4: Network | 1 week | 3 |
| Phase 5: Services | 1 week | 4 |
| Phase 6: Application | 1 week | 5 |
| Phase 7: Integration | 1 week | 4 |
| **Total** | **8 weeks** | **29 tasks** |

---

## 8. Next Steps

1. **User Confirmation**: Review and approve this plan
2. **Phase 1 Execution**: Start with foundation layer
3. **Parallel Development**: Execute independent tasks concurrently
4. **Continuous Integration**: Run tests after each phase
