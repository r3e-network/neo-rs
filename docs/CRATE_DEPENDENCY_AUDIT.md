# Crate Dependency Audit Report

**Generated**: 2024-12-14
**Version**: 0.7.0
**Total Active Crates**: 15

## Complete Dependency Graph

```
                    ┌─────────────────────────────────────────┐
                    │          APPLICATION LAYER              │
                    │                                         │
                    │   neo-cli ────► neo-node                │
                    │      │              │                   │
                    │      ▼              ▼                   │
                    │   neo-rpc      neo-core                 │
                    └──────┼──────────────┼───────────────────┘
                           │              │
┌──────────────────────────┼──────────────┼───────────────────────────────────┐
│                          │    CORE LAYER                                    │
│                          ▼              ▼                                   │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │                        neo-core (25K+ LOC)                          │    │
│  │  Contains: network/p2p, ledger, persistence, smart_contract,        │    │
│  │            wallets, state_service, telemetry, actors                │    │
│  └─────────────────────────┬──────────────────────────────────────────┘    │
│                            │                                               │
│     ┌──────────────────────┼──────────────────────────────────┐           │
│     ▼           ▼          ▼           ▼          ▼          ▼            │
│  neo-vm   neo-consensus  neo-p2p  neo-storage  neo-crypto  neo-io         │
└─────┼───────────┼──────────┼───────────┼──────────┼──────────┼────────────┘
      │           │          │           │          │          │
      └───────────┴──────────┴───────────┴──────────┴──────────┘
                             │
┌────────────────────────────┼────────────────────────────────────────────────┐
│                   FOUNDATION LAYER                                          │
│                            ▼                                                │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                     neo-primitives                                   │   │
│  │           (UInt160, UInt256, BigDecimal, WitnessScope)              │   │
│  └──────────────────────────┬──────────────────────────────────────────┘   │
│                             │                                               │
│                             ▼                                               │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        neo-io                                        │   │
│  │          (Serializable, BinaryWriter, MemoryReader)                 │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐                       │
│  │  neo-json   │   │ neo-config  │   │neo-telemetry│                       │
│  │ (no deps)   │   │             │   │             │                       │
│  └─────────────┘   └─────────────┘   └─────────────┘                       │
└─────────────────────────────────────────────────────────────────────────────┘

                    ┌─────────────────────────────────────────┐
                    │        STANDALONE ALTERNATIVES          │
                    │                                         │
                    │   neo-mempool ◄─── neo-chain            │
                    │   (lightweight)    (standalone state)   │
                    └─────────────────────────────────────────┘
```

## Crate-by-Crate Dependencies

### Foundation Layer (Zero neo-\* dependencies)

| Crate        | Dependencies    | LOC    | Purpose              |
| ------------ | --------------- | ------ | -------------------- |
| **neo-io**   | (external only) | ~1,200 | Serialization traits |
| **neo-json** | (external only) | ~500   | JSON types           |

### Foundation Layer (Minimal dependencies)

| Crate              | neo-\* Dependencies | LOC    | Purpose                      |
| ------------------ | ------------------- | ------ | ---------------------------- |
| **neo-primitives** | neo-io              | ~800   | UInt160, UInt256, BigDecimal |
| **neo-crypto**     | neo-primitives      | ~600   | Hashing, ECC, signatures     |
| **neo-storage**    | neo-primitives      | ~400   | Storage traits               |
| **neo-vm**         | neo-io              | ~8,000 | Virtual machine              |
| **neo-config**     | neo-primitives      | ~600   | Configuration                |

### Protocol Layer

| Crate             | neo-\* Dependencies                | LOC  | Purpose         |
| ----------------- | ---------------------------------- | ---- | --------------- |
| **neo-p2p**       | neo-primitives, neo-crypto, neo-io | ~400 | P2P types only  |
| **neo-consensus** | neo-primitives, neo-crypto, neo-io | ~300 | dBFT types only |

### Infrastructure Layer

| Crate             | neo-\* Dependencies                     | LOC    | Purpose                  |
| ----------------- | --------------------------------------- | ------ | ------------------------ |
| **neo-telemetry** | neo-config                              | ~500   | Production observability |
| **neo-mempool**   | neo-primitives, neo-config              | ~800   | Lightweight mempool      |
| **neo-chain**     | neo-primitives, neo-config, neo-mempool | ~1,500 | Standalone chain state   |

### Core Layer

| Crate        | neo-\* Dependencies                                                             | LOC     | Purpose                                      |
| ------------ | ------------------------------------------------------------------------------- | ------- | -------------------------------------------- |
| **neo-core** | neo-vm, neo-io, neo-primitives, neo-crypto, neo-storage, neo-p2p, neo-consensus | ~25,000 | **MONOLITHIC** - All C# parity functionality |

### Application Layer

| Crate        | neo-\* Dependencies                                                                    | LOC    | Purpose           |
| ------------ | -------------------------------------------------------------------------------------- | ------ | ----------------- |
| **neo-rpc**  | neo-primitives, neo-crypto, neo-io, (optional: neo-core, neo-vm, neo-json, neo-config) | ~5,000 | RPC server/client |
| **neo-cli**  | neo-rpc, neo-primitives, neo-json                                                      | ~400   | CLI client        |
| **neo-node** | neo-core                                                                               | ~600   | Node daemon       |

## Key Findings

### 1. No Circular Dependencies ✅

Cargo tree analysis confirms no circular dependencies exist in the workspace.

### 2. Clean Foundation Layer ✅

The foundation crates (neo-io, neo-json, neo-primitives, neo-crypto, neo-storage, neo-vm) form a clean DAG with no problematic dependencies.

### 3. neo-core is Monolithic (CRITICAL)

**neo-core** contains 25,000+ LOC across 370+ files including:

- Full P2P implementation (82 files, ~5,000 LOC)
- Ledger/blockchain (20+ files)
- Persistence implementations (RocksDB, Memory)
- Smart contract engine (40+ files)
- State service (10+ files)
- Wallets (10+ files)
- Telemetry (5+ files)
- Actor runtime (actors/akka)

### 4. P2P Split is Types-Only

**neo-p2p** crate (400 LOC) only contains type definitions:

- MessageCommand
- InventoryType
- MessageFlags
- NodeCapabilityType
- etc.

The actual P2P implementation (LocalNode, RemoteNode, message handling) remains in neo-core/src/network/.

### 5. neo-consensus is Types-Only

Similar to neo-p2p, **neo-consensus** only contains dBFT message types, not the consensus implementation (which is in neo-plugins/dbft_plugin).

### 6. Standalone Alternatives Work Correctly ✅

**neo-mempool** and **neo-chain** are correctly isolated lightweight alternatives that don't depend on neo-core.

### 7. Feature Gating Works ✅

**neo-rpc** uses feature flags effectively:

- `client` feature: Adds neo-config, neo-core, neo-json, neo-vm, reqwest
- `server` feature: Adds warp, hyper, additional crypto

## Risk Assessment for P2P Extraction

**Analysis**: P2P implementation in neo-core depends on:

| Dependency                                           | Files Affected | Extraction Risk |
| ---------------------------------------------------- | -------------- | --------------- |
| cryptography (BloomFilter)                           | 5+             | MEDIUM          |
| ledger (Blockchain, HeaderCache, MemoryPool)         | 10+            | VERY HIGH       |
| persistence (DataCache, StoreCache)                  | 10+            | HIGH            |
| smart_contract (ApplicationEngine, native contracts) | 15+            | VERY HIGH       |
| neo_system (NeoSystemContext)                        | 10+            | VERY HIGH       |
| akka (Actor framework)                               | 20+            | HIGH            |
| protocol_settings                                    | 5+             | MEDIUM          |

**Conclusion**: P2P extraction would require extracting 50%+ of neo-core first. **NOT RECOMMENDED** at this time.

## Recommendations

### Phase 1: Documentation (COMPLETED ✅)

- Document dual implementations (mempool, telemetry, chain)
- Clarify when to use each crate

### Phase 2: Minor Cleanup

1. Consider consolidating neo-io + neo-json into neo-primitives
2. Move remaining type definitions from neo-core to neo-p2p/neo-consensus

### Phase 3: Long-term (Future)

1. Extract traits from neo-core for better testability
2. Consider trait-based abstractions for P2P if C# compatibility allows
