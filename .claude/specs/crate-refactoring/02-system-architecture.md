# System Architecture Document: Neo-RS Crate Refactoring

**Version**: 1.0
**Status**: Draft
**Quality Score**: TBD
**Date**: 2025-12-14

---

## 1. Current State Analysis

### 1.1 Dependency Graph (Affected Crates)

```
neo-contract (9 files)
├── Dependencies: neo-primitives, neo-crypto, neo-storage, neo-vm, neo-io
└── Used by: neo-core (re-export only)

neo-services (1 file, no dependencies)
└── Used by: neo-core (re-export only)

neo-rpc-client (54 files)
├── Dependencies: neo-config, neo-core, neo-io, neo-json, neo-primitives, neo-vm
├── Dependencies: reqwest, hyper, tokio, serde, serde_json
└── Used by: neo-cli (28 files), neo-rpc/server (1 file)
```

### 1.2 Impact Analysis

| Crate | Files | Code Consumers | Migration Complexity |
|-------|-------|----------------|---------------------|
| neo-contract | 9 | 1 (neo-core/lib.rs) | Low |
| neo-services | 1 | 1 (neo-core/services/mod.rs) | Very Low |
| neo-rpc-client | 54 | 29 files (neo-cli + neo-rpc) | Medium |

---

## 2. Target Architecture

### 2.1 Final Crate Structure (18 crates)

```
neo-rs/
├── Foundation Layer (no internal deps)
│   ├── neo-primitives     # UInt160, UInt256, WitnessScope
│   ├── neo-io             # Serialization primitives
│   ├── neo-json           # JSON utilities
│   └── neo-config         # Protocol settings
│
├── Crypto Layer
│   └── neo-crypto         # Hash, signatures, ECC
│
├── Storage Layer
│   └── neo-storage        # Storage traits, RocksDB provider
│
├── VM Layer
│   └── neo-vm             # Execution engine, stack items
│
├── Core Layer (contains merged neo-contract + neo-services)
│   └── neo-core           # Blockchain types, smart contracts, services
│       ├── src/contract/  # ← FROM neo-contract
│       └── src/services/  # ← FROM neo-services (traits already here)
│
├── Network Layer
│   ├── neo-p2p            # P2P message types
│   └── neo-consensus      # dBFT consensus
│
├── Protocol Layer
│   ├── neo-mempool        # Transaction pool (stub)
│   ├── neo-chain          # Chain management (stub)
│   └── neo-telemetry      # Metrics, observability
│
├── RPC Layer (merged neo-rpc + neo-rpc-client)
│   └── neo-rpc
│       ├── src/server/    # RPC server (feature = "server")
│       └── src/client/    # ← FROM neo-rpc-client (feature = "client")
│
├── Application Layer
│   ├── neo-node           # Full node daemon
│   ├── neo-cli            # CLI application
│   └── neo-plugins        # Plugin system
│
└── Support Layer
    └── neo-akka           # Actor runtime
```

### 2.2 Feature Flags Design

**neo-rpc/Cargo.toml** (after refactoring):

```toml
[features]
default = []

# RPC Server (existing)
server = [
    "dep:neo-core",
    "dep:neo-vm",
    "dep:neo-json",
    "dep:warp",
    "dep:hyper",
    # ... other server deps
]

# RPC Client (NEW - from neo-rpc-client)
client = [
    "dep:neo-config",
    "dep:neo-core",
    "dep:neo-io",
    "dep:neo-json",
    "dep:neo-primitives",
    "dep:neo-vm",
    "dep:reqwest",
    "dep:regex",
    "dep:anyhow",
    "dep:mockito",  # dev only
]
```

---

## 3. Migration Strategy

### 3.1 Phase 1: neo-contract → neo-core

**Step 1.1**: Move files (git mv for history)
```bash
git mv neo-contract/src/error.rs neo-core/src/contract/error.rs
git mv neo-contract/src/find_options.rs neo-core/src/contract/find_options.rs
git mv neo-contract/src/role.rs neo-core/src/contract/role.rs
git mv neo-contract/src/contract_basic_method.rs neo-core/src/contract/basic_method.rs
git mv neo-contract/src/method_token.rs neo-core/src/contract/method_token.rs
git mv neo-contract/src/trigger_type.rs neo-core/src/contract/trigger_type.rs
git mv neo-contract/src/contract_parameter_type.rs neo-core/src/contract/parameter_type.rs
git mv neo-contract/src/storage_context.rs neo-core/src/contract/storage_context.rs
```

**Step 1.2**: Update neo-core/src/contract/mod.rs
```rust
// Merge re-exports from neo-contract
pub mod error;
pub mod find_options;
pub mod role;
pub mod basic_method;
pub mod method_token;
pub mod trigger_type;
pub mod parameter_type;
pub mod storage_context;

pub use error::*;
pub use find_options::*;
pub use role::*;
pub use basic_method::*;
pub use method_token::*;
pub use trigger_type::*;
pub use parameter_type::*;
pub use storage_context::*;
```

**Step 1.3**: Update dependencies
- neo-core/Cargo.toml: Remove `neo-contract` dependency
- neo-core/Cargo.toml: Add neo-contract's dependencies directly
- Root Cargo.toml: Remove neo-contract from workspace.members

**Step 1.4**: Remove neo-contract
```bash
rm -rf neo-contract/
```

### 3.2 Phase 1: neo-services → neo-core

**Analysis**: neo-services only contains trait definitions. neo-core/src/services/ already exists and re-exports from neo-services.

**Step 2.1**: Inline traits into neo-core
```bash
# Copy content, don't git mv (traits already have local definitions)
cat neo-services/src/lib.rs >> neo-core/src/services/traits.rs
```

**Step 2.2**: Update neo-core/src/services/mod.rs
- Remove `use neo_services::*` imports
- Add local trait definitions

**Step 2.3**: Remove neo-services
```bash
rm -rf neo-services/
```
- Update root Cargo.toml: Remove from workspace.members
- Update neo-core/Cargo.toml: Remove dependency

### 3.3 Phase 2: neo-rpc-client → neo-rpc

**Step 3.1**: Create client directory structure
```bash
mkdir -p neo-rpc/src/client
```

**Step 3.2**: Move client code (git mv)
```bash
git mv neo-rpc-client/src/rpc_client/ neo-rpc/src/client/rpc_client/
git mv neo-rpc-client/src/models/ neo-rpc/src/client/models/
git mv neo-rpc-client/src/utility/ neo-rpc/src/client/utility/
git mv neo-rpc-client/src/error.rs neo-rpc/src/client/error.rs
git mv neo-rpc-client/src/contract_client.rs neo-rpc/src/client/contract_client.rs
git mv neo-rpc-client/src/wallet_api.rs neo-rpc/src/client/wallet_api.rs
git mv neo-rpc-client/src/nep17_api.rs neo-rpc/src/client/nep17_api.rs
git mv neo-rpc-client/src/policy_api.rs neo-rpc/src/client/policy_api.rs
git mv neo-rpc-client/src/state_api.rs neo-rpc/src/client/state_api.rs
git mv neo-rpc-client/src/transaction_manager.rs neo-rpc/src/client/transaction_manager.rs
git mv neo-rpc-client/src/transaction_manager_factory.rs neo-rpc/src/client/transaction_manager_factory.rs
```

**Step 3.3**: Create neo-rpc/src/client/mod.rs
```rust
//! Neo RPC Client
//!
//! HTTP client for interacting with Neo N3 RPC endpoints.
//! Enable with `features = ["client"]`.

pub mod error;
pub mod models;
pub mod rpc_client;
pub mod utility;
pub mod contract_client;
pub mod wallet_api;
pub mod nep17_api;
pub mod policy_api;
pub mod state_api;
pub mod transaction_manager;
pub mod transaction_manager_factory;

pub use error::*;
pub use rpc_client::*;
pub use models::*;
```

**Step 3.4**: Update neo-rpc/src/lib.rs
```rust
// Add at top level
#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "client")]
pub use client::*;
```

**Step 3.5**: Update neo-rpc/Cargo.toml
- Add `client` feature with dependencies from neo-rpc-client
- Remove `dep:neo-rpc-client` from server feature

**Step 3.6**: Update consumers
- neo-cli/Cargo.toml: Change `neo-rpc-client = ...` to `neo-rpc = { features = ["client"] }`
- neo-cli/**/*.rs: Change `use neo_rpc_client::` to `use neo_rpc::client::`

**Step 3.7**: Remove neo-rpc-client
```bash
rm -rf neo-rpc-client/
```
- Update root Cargo.toml: Remove from workspace.members

---

## 4. Dependency Updates Summary

### 4.1 Cargo.toml Changes

**Root Cargo.toml** (workspace.members):
```diff
- "neo-contract",
- "neo-services",
- "neo-rpc-client",
```

**neo-core/Cargo.toml**:
```diff
- neo-contract = { path = "../neo-contract" }
- neo-services = { path = "../neo-services" }
+ # Dependencies from neo-contract (if not already present)
+ bitflags = "2.4"
```

**neo-rpc/Cargo.toml**:
```diff
[features]
server = [
-   "dep:neo-rpc-client",
    # ... rest unchanged
]

+ client = [
+     "dep:neo-config",
+     "dep:neo-core",
+     "dep:neo-io",
+     "dep:neo-json",
+     "dep:neo-vm",
+     "dep:reqwest",
+     "dep:regex",
+     "dep:anyhow",
+ ]

[dependencies]
- neo-rpc-client = { path = "../neo-rpc-client", optional = true }
+ neo-config = { path = "../neo-config", optional = true }
+ reqwest = { version = "0.11", features = ["json"], optional = true }
+ regex = { version = "1.10", optional = true }
+ anyhow = { version = "1.0", optional = true }
```

**neo-cli/Cargo.toml**:
```diff
- neo-rpc-client = { path = "../neo-rpc-client" }
+ neo-rpc = { path = "../neo-rpc", features = ["client"] }
```

### 4.2 Import Path Changes

| Old Import | New Import |
|------------|------------|
| `use neo_contract::*` | `use neo_core::contract::*` |
| `use neo_services::*` | `use neo_core::services::*` |
| `use neo_rpc_client::*` | `use neo_rpc::client::*` |

---

## 5. Verification Checklist

### 5.1 Build Verification
- [ ] `cargo build --workspace`
- [ ] `cargo build --workspace --all-features`
- [ ] `cargo build -p neo-rpc --features client`
- [ ] `cargo build -p neo-rpc --features server`
- [ ] `cargo build -p neo-rpc --features client,server`

### 5.2 Test Verification
- [ ] `cargo test --workspace`
- [ ] `cargo test --workspace --all-features`
- [ ] `cargo test -p neo-rpc --features client`
- [ ] `cargo test -p neo-cli`

### 5.3 Documentation Verification
- [ ] `cargo doc --workspace --no-deps`
- [ ] Update docs/ARCHITECTURE.md

---

## 6. Risk Mitigation

### 6.1 Rollback Plan
Each phase should be committed separately:
1. Commit: "refactor: merge neo-contract into neo-core"
2. Commit: "refactor: inline neo-services into neo-core"
3. Commit: "refactor: merge neo-rpc-client into neo-rpc"

If any phase fails, revert that specific commit.

### 6.2 Circular Dependency Prevention
Current dependency order (bottom to top):
```
neo-primitives
    ↓
neo-io, neo-crypto
    ↓
neo-storage, neo-vm
    ↓
neo-core (includes contract + services)
    ↓
neo-rpc (includes client)
    ↓
neo-cli
```

No circular dependencies introduced by this refactoring.

---

## 7. Quality Score Self-Assessment

| Criteria | Score | Notes |
|----------|-------|-------|
| Completeness | 9/10 | All migration paths documented |
| Clarity | 9/10 | Step-by-step instructions |
| Feasibility | 9/10 | Low-risk, incremental approach |
| Risk Management | 9/10 | Rollback plan included |
| Dependency Analysis | 10/10 | Full impact analysis |

**Total Score: 92/100**

---

## 8. Approval

- [x] Architecture Score: 92/100
- [ ] User Approval: Pending
- [ ] Implementation: Pending
