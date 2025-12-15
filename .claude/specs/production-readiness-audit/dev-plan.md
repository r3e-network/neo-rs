# Production Readiness Fixes - Development Plan

**Date**: 2025-12-14
**Sprint Goal**: Fix all CRITICAL/HIGH issues from production readiness audit

---

## Audit Summary

| Category | Status | Critical | High | Medium | Low |
|----------|--------|----------|------|--------|-----|
| API Compatibility | ✅ PASS | 0 | 0 | 0 | 0 |
| Error Handling | ⚠️ ISSUES | 1 | 3 | 3 | 0 |
| Documentation | ⚠️ GAPS | 0 | 0 | 2 | 2 |
| Test Coverage | ✅ GOOD | 0 | 0 | 0 | 0 |
| Security | ⚠️ ISSUES | 0 | 3 | 3 | 2 |

**Test Counts**: neo-core: 653 tests, neo-rpc client: 90 tests

---

## Task Breakdown

### Task 1: Fix RpcException Naming Collision (CRITICAL)
**Priority**: P0
**Scope**: neo-rpc/src/client/error.rs, neo-rpc/src/lib.rs
**Dependencies**: None

**Problem**: Two different `RpcException` types exist:
- `neo-rpc/src/client/error.rs:8` - Client-specific
- `neo-rpc/src/server/rpc_exception.rs:1` - Re-exports from neo-core

**Fix**:
1. Rename client error to `ClientRpcError` in `neo-rpc/src/client/error.rs`
2. Update all client module imports
3. Update neo-rpc/src/lib.rs re-exports to clarify naming

**Test**: `cargo build -p neo-rpc --features client,server && cargo test -p neo-rpc`

---

### Task 2: Add Missing Documentation (MEDIUM)
**Priority**: P1
**Scope**: neo-core/src/services/mod.rs, neo-rpc/src/client/models/mod.rs
**Dependencies**: None

**Files to Update**:
1. `neo-core/src/services/mod.rs:58` - Add doc for `LockedMempoolService::new()`
2. `neo-rpc/src/client/models/mod.rs:1` - Add module-level documentation
3. `neo-core/src/services/mod.rs:21,35,45,63` - Add impl block docs

**Test**: `cargo doc -p neo-core -p neo-rpc --no-deps 2>&1 | grep -c warning`

---

### Task 3: Fix Credential Memory Safety (MEDIUM)
**Priority**: P1
**Scope**: neo-rpc/src/client/rpc_client/builder.rs
**Dependencies**: None

**Problem**: RPC credentials stored in plain String without zeroizing

**Fix**:
1. Add `zeroize` crate to neo-rpc dependencies
2. Use `Zeroizing<String>` for `rpc_user` and `rpc_pass` in builder
3. Clear sensitive data after build()

**Test**: `cargo test -p neo-rpc --features client`

---

### Task 4: Add Unsafe Code Safety Documentation (HIGH)
**Priority**: P1
**Scope**: neo-rpc/src/server/session.rs, neo-vm/src/execution_engine/
**Dependencies**: None

**Problem**: Multiple unsafe blocks lack SAFETY comments

**Fix**:
1. Add SAFETY comment to `neo-rpc/src/server/session.rs:247-248`
2. Add SAFETY comments to `neo-vm/src/execution_engine/execution.rs:230,257,275`
3. Add SAFETY comments to `neo-vm/src/execution_engine/interop.rs:40,56`
4. Add SAFETY comments to `neo-vm/src/execution_engine/context.rs:22,66`
5. Add SAFETY comment to `neo-core/src/cryptography/crypto_utils.rs:729-734`

**Test**: `cargo clippy --workspace -- -D clippy::undocumented_unsafe_blocks`

---

### Task 5: Fix KeyPair Debug Output (LOW)
**Priority**: P2
**Scope**: neo-core/src/wallets/key_pair.rs
**Dependencies**: None

**Problem**: KeyPair derives Debug which could leak private key in logs

**Fix**:
1. Remove `#[derive(Debug)]` from KeyPair
2. Implement custom Debug that hides private_key field
3. Add test verifying Debug output doesn't contain key material

**Test**: `cargo test -p neo-core key_pair`

---

## Execution Order

```
Parallel Group 1 (Independent):
├── Task 2: Documentation fixes
├── Task 3: Credential safety
└── Task 5: KeyPair Debug fix

Sequential (After Group 1):
├── Task 1: RpcException naming (may affect Task 2)
└── Task 4: Unsafe documentation (standalone)
```

---

## Acceptance Criteria

- [ ] `cargo build --workspace --all-features` succeeds
- [ ] `cargo test --workspace --all-features` passes
- [ ] `cargo clippy --workspace` reports no new warnings
- [ ] `cargo doc --workspace --no-deps` completes without errors
- [ ] No CRITICAL or HIGH severity issues remain

---

## Risk Register

| Risk | Mitigation |
|------|------------|
| RpcException rename breaks API | Use type alias for backward compat |
| Zeroize adds runtime overhead | Only apply to credential fields |
| Unsafe docs don't cover all cases | Review with cargo miri if available |
