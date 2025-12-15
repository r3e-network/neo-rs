# Product Requirements Document: Neo-RS Crate Architecture Refactoring

**Version**: 1.0
**Status**: Approved
**Quality Score**: 92/100
**Date**: 2025-12-14
**Owner**: Neo-RS Team

---

## Executive Summary

Refactor the neo-rs workspace from 20 crates to 18 crates by consolidating redundant modules and improving single-responsibility adherence. This MVP focuses on cleanup and RPC consolidation, with oracle and state extraction deferred to P1.

## Scope Definition

### MVP Scope (P0)

| Phase | Description | Crates Affected |
|-------|-------------|-----------------|
| Phase 1 | Remove redundant crates | neo-contract → neo-core, neo-services → neo-core |
| Phase 2 | Consolidate RPC stack | neo-rpc-client → neo-rpc (feature-gated) |

### Deferred Scope (P1)

| Phase | Description | Status |
|-------|-------------|--------|
| Phase 3 | Extract neo-oracle | Deferred |
| Phase 4 | Extract neo-state | Deferred |

## Target Architecture

### Final Crate List (18 crates)

```
neo-rs/
├── neo-primitives     # Core types (UInt160, UInt256, etc.)
├── neo-config         # Protocol settings, configuration
├── neo-io             # Serialization primitives
├── neo-json           # JSON utilities
├── neo-crypto         # Cryptographic operations
├── neo-storage        # Storage traits and providers
├── neo-vm             # Virtual machine implementation
├── neo-core           # Core blockchain types + merged contract/services
├── neo-p2p            # P2P networking types
├── neo-consensus      # dBFT consensus
├── neo-mempool        # Transaction mempool (stub)
├── neo-chain          # Chain management (stub)
├── neo-rpc            # RPC server + client (feature-gated)
├── neo-telemetry      # Metrics and observability
├── neo-node           # Full node daemon
├── neo-cli            # CLI application
├── neo-akka           # Actor runtime (ractor-based)
└── neo-plugins        # Plugin system
```

---

## Epic 1: Crate Cleanup

### User Story 1.1: Remove neo-contract

**As a** maintainer
**I want to** merge neo-contract into neo-core
**So that** contract types are co-located with core blockchain primitives

**Acceptance Criteria**:
- [ ] All types from neo-contract moved to neo-core/src/contract/
- [ ] All workspace references updated from `neo-contract` to `neo-core`
- [ ] neo-contract directory removed from workspace
- [ ] Cargo.toml workspace members updated
- [ ] `cargo build --workspace` passes
- [ ] `cargo test --workspace` passes

**Technical Notes**:
- neo-contract contains: StorageContext, InteropParameterDescriptor
- Target location: neo-core/src/contract/ (already exists)
- Use `git mv` for history preservation

### User Story 1.2: Remove neo-services

**As a** maintainer
**I want to** inline neo-services into neo-core
**So that** service interfaces are not in a separate stub crate

**Acceptance Criteria**:
- [ ] Service traits moved to neo-core/src/services/
- [ ] All workspace references updated
- [ ] neo-services directory removed
- [ ] `cargo build --workspace` passes
- [ ] `cargo test --workspace` passes

**Technical Notes**:
- neo-services is a thin wrapper crate
- Services already partially exist in neo-core/src/services/

---

## Epic 2: RPC Consolidation

### User Story 2.1: Merge neo-rpc-client into neo-rpc

**As a** developer
**I want to** have a single neo-rpc crate with optional client functionality
**So that** RPC components are unified under one namespace

**Acceptance Criteria**:
- [ ] neo-rpc-client code moved to neo-rpc/src/client/
- [ ] Feature flag `client` added to neo-rpc
- [ ] Default features exclude client (server-only by default)
- [ ] Dependent crates updated to use `neo-rpc = { features = ["client"] }`
- [ ] Public API preserved: `neo_rpc::client::*`
- [ ] neo-rpc-client directory removed
- [ ] All tests pass

**Technical Notes**:
- Feature: `[features] client = ["reqwest", "tokio"]`
- Re-export: `#[cfg(feature = "client")] pub mod client;`
- Update neo-cli and any other dependents

### User Story 2.2: Update Workspace Dependencies

**As a** maintainer
**I want to** update all Cargo.toml files
**So that** the workspace reflects the new structure

**Acceptance Criteria**:
- [ ] Root Cargo.toml workspace.members updated
- [ ] All internal dependencies updated
- [ ] No dangling references to removed crates
- [ ] `cargo tree` shows correct dependency graph

---

## Epic 3: Verification

### User Story 3.1: Build Verification

**As a** maintainer
**I want to** verify the refactored workspace compiles
**So that** we don't ship broken code

**Acceptance Criteria**:
- [ ] `cargo build --workspace` succeeds
- [ ] `cargo build --workspace --all-features` succeeds
- [ ] No new warnings introduced

### User Story 3.2: Test Verification

**As a** maintainer
**I want to** verify all tests pass
**So that** functionality is preserved

**Acceptance Criteria**:
- [ ] `cargo test --workspace` succeeds
- [ ] `cargo test --workspace --all-features` succeeds
- [ ] Test coverage maintained (no regression)

### User Story 3.3: Documentation Update

**As a** maintainer
**I want to** update documentation
**So that** the crate structure is accurately documented

**Acceptance Criteria**:
- [ ] docs/ARCHITECTURE.md updated with new crate list
- [ ] README.md updated if necessary
- [ ] Each crate's lib.rs docs updated

---

## Non-Functional Requirements

### Performance
- Build time: No significant regression (< 10% increase acceptable)
- Binary size: No significant increase (< 5% acceptable)

### Compatibility
- No transition period (direct deletion)
- One-time release (not incremental)
- Internal security review only

### Quality Gates
- All CI checks must pass
- Code review required before merge
- Architecture document must score 90+/100

---

## Risks and Mitigations

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Breaking dependent code | Medium | High | Comprehensive grep for imports |
| Test failures | Medium | Medium | Run tests after each step |
| Circular dependencies | Low | High | Careful dependency ordering |
| Git history loss | Low | Medium | Use `git mv` consistently |

---

## Success Metrics

1. **Crate count**: 20 → 18 (10% reduction)
2. **Build success**: 100% on all targets
3. **Test success**: 100% pass rate
4. **Zero regressions**: No functionality lost

---

## Timeline

| Milestone | Duration | Status |
|-----------|----------|--------|
| Phase 1: Crate Cleanup | 1 sprint | Pending |
| Phase 2: RPC Consolidation | 1 sprint | Pending |
| Verification & Release | 0.5 sprint | Pending |

**Total MVP Duration**: ~2.5 sprints

---

## Approval

- [x] PRD Quality Score: 92/100
- [x] User Approval: Confirmed (2025-12-14)
- [ ] Architecture Design: Pending
- [ ] Sprint Plan: Pending
- [ ] Implementation: Pending
