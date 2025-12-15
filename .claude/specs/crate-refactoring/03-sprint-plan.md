# Sprint Plan: Neo-RS Crate Architecture Refactoring

**Sprint Goal**: Reduce workspace from 20 to 18 crates by consolidating redundant modules
**Duration**: MVP (Phase 1 + Phase 2)
**Date**: 2025-12-14

---

## Task Breakdown

### Epic 1: Crate Cleanup (Phase 1)

#### Task 1.1: Merge neo-contract into neo-core
**Priority**: P0
**Estimate**: 30 minutes
**Dependencies**: None

**Subtasks**:
1. [ ] Create neo-core/src/contract/ subdirectories if needed
2. [ ] Move neo-contract source files using git mv:
   - error.rs → contract/error.rs
   - find_options.rs → contract/find_options.rs
   - role.rs → contract/role.rs
   - contract_basic_method.rs → contract/basic_method.rs
   - method_token.rs → contract/method_token.rs
   - trigger_type.rs → contract/trigger_type.rs
   - contract_parameter_type.rs → contract/parameter_type.rs
   - storage_context.rs → contract/storage_context.rs
3. [ ] Update neo-core/src/contract/mod.rs with new modules
4. [ ] Update neo-core/Cargo.toml:
   - Remove neo-contract dependency
   - Add bitflags = "2.4" (from neo-contract)
5. [ ] Update neo-core/src/lib.rs re-exports
6. [ ] Remove neo-contract from root Cargo.toml workspace.members
7. [ ] Delete neo-contract/ directory
8. [ ] Run `cargo build -p neo-core` to verify

**Acceptance**: `cargo build --workspace` passes

---

#### Task 1.2: Inline neo-services into neo-core
**Priority**: P0
**Estimate**: 15 minutes
**Dependencies**: Task 1.1 complete (avoid conflicts)

**Subtasks**:
1. [ ] Copy neo-services trait definitions to neo-core/src/services/traits.rs
2. [ ] Update neo-core/src/services/mod.rs:
   - Add `pub mod traits;`
   - Re-export traits locally
3. [ ] Update neo-core/Cargo.toml: Remove neo-services dependency
4. [ ] Remove neo-services from root Cargo.toml workspace.members
5. [ ] Delete neo-services/ directory
6. [ ] Run `cargo build -p neo-core` to verify

**Acceptance**: `cargo build --workspace` passes

---

### Epic 2: RPC Consolidation (Phase 2)

#### Task 2.1: Create client module structure in neo-rpc
**Priority**: P0
**Estimate**: 15 minutes
**Dependencies**: Phase 1 complete

**Subtasks**:
1. [ ] Create neo-rpc/src/client/ directory
2. [ ] Move neo-rpc-client source files using git mv:
   - rpc_client/ → client/rpc_client/
   - models/ → client/models/
   - utility/ → client/utility/
   - error.rs → client/error.rs
   - lib.rs content → client/mod.rs
   - All API modules (wallet_api, nep17_api, etc.)
3. [ ] Create client/mod.rs with proper exports

**Acceptance**: Files moved, no orphan imports

---

#### Task 2.2: Update neo-rpc Cargo.toml
**Priority**: P0
**Estimate**: 10 minutes
**Dependencies**: Task 2.1 complete

**Subtasks**:
1. [ ] Add `client` feature flag with dependencies:
   - neo-config (already workspace)
   - reqwest 0.11 with json feature
   - regex 1.10
   - anyhow 1.0
2. [ ] Make client dependencies optional
3. [ ] Remove neo-rpc-client from server feature deps
4. [ ] Update lib.rs with `#[cfg(feature = "client")]` gating

**Acceptance**: `cargo build -p neo-rpc --features client` passes

---

#### Task 2.3: Update neo-cli dependencies
**Priority**: P0
**Estimate**: 20 minutes
**Dependencies**: Task 2.2 complete

**Subtasks**:
1. [ ] Update neo-cli/Cargo.toml:
   - Remove `neo-rpc-client`
   - Add `neo-rpc = { features = ["client"] }`
2. [ ] Update all neo-cli imports (28 files):
   - `use neo_rpc_client::` → `use neo_rpc::client::`
3. [ ] Run `cargo build -p neo-cli` to verify

**Acceptance**: `cargo build -p neo-cli` passes

---

#### Task 2.4: Update neo-rpc server dependencies
**Priority**: P0
**Estimate**: 10 minutes
**Dependencies**: Task 2.2 complete

**Subtasks**:
1. [ ] Update neo-rpc/src/server/rpc_server_blockchain.rs:
   - `use neo_rpc_client::` → `use crate::client::`
2. [ ] Update neo-rpc server feature to not depend on neo-rpc-client
3. [ ] Run `cargo build -p neo-rpc --features server` to verify

**Acceptance**: `cargo build -p neo-rpc --features server` passes

---

#### Task 2.5: Remove neo-rpc-client
**Priority**: P0
**Estimate**: 5 minutes
**Dependencies**: Tasks 2.3 and 2.4 complete

**Subtasks**:
1. [ ] Remove neo-rpc-client from root Cargo.toml workspace.members
2. [ ] Delete neo-rpc-client/ directory
3. [ ] Run `cargo build --workspace` to verify

**Acceptance**: `cargo build --workspace` passes

---

### Epic 3: Verification

#### Task 3.1: Full build verification
**Priority**: P0
**Estimate**: 10 minutes
**Dependencies**: All previous tasks complete

**Subtasks**:
1. [ ] `cargo build --workspace`
2. [ ] `cargo build --workspace --all-features`
3. [ ] `cargo build -p neo-rpc --features client`
4. [ ] `cargo build -p neo-rpc --features server`
5. [ ] `cargo build -p neo-rpc --features client,server`

**Acceptance**: All commands succeed with no errors

---

#### Task 3.2: Test verification
**Priority**: P0
**Estimate**: 15 minutes
**Dependencies**: Task 3.1 complete

**Subtasks**:
1. [ ] `cargo test --workspace`
2. [ ] `cargo test --workspace --all-features`
3. [ ] `cargo test -p neo-rpc --features client`
4. [ ] `cargo test -p neo-cli`

**Acceptance**: All tests pass

---

#### Task 3.3: Documentation update
**Priority**: P1
**Estimate**: 10 minutes
**Dependencies**: Task 3.2 complete

**Subtasks**:
1. [ ] Update docs/ARCHITECTURE.md with new crate list
2. [ ] Verify `cargo doc --workspace --no-deps` succeeds

**Acceptance**: Documentation builds without warnings

---

## Execution Order

```
Phase 1: Crate Cleanup
├── Task 1.1: neo-contract → neo-core
└── Task 1.2: neo-services → neo-core (after 1.1)

Phase 2: RPC Consolidation
├── Task 2.1: Create client structure
├── Task 2.2: Update neo-rpc Cargo.toml (after 2.1)
├── Task 2.3: Update neo-cli (after 2.2)
├── Task 2.4: Update neo-rpc server (after 2.2)
└── Task 2.5: Remove neo-rpc-client (after 2.3 + 2.4)

Phase 3: Verification
├── Task 3.1: Build verification
├── Task 3.2: Test verification (after 3.1)
└── Task 3.3: Documentation (after 3.2)
```

---

## Risk Register

| Risk | Mitigation |
|------|------------|
| Import path breakage | Search all `use neo_*` before deleting crates |
| Circular dependency | Verify dependency order before each change |
| Test failures | Run tests incrementally after each task |

---

## Definition of Done

- [ ] Workspace contains exactly 18 crates
- [ ] `cargo build --workspace` succeeds
- [ ] `cargo test --workspace` succeeds
- [ ] No references to removed crates remain
- [ ] Architecture docs updated
