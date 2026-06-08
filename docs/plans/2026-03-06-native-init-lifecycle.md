# Native Contract Init Lifecycle Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove duplicate native-contract initialization during `OnPersist` so policy hardfork initialization follows Neo N3 semantics exactly once per initialize block.

**Architecture:** Keep eager native initialization for `Application`, `Verification`, and `PostPersist` engines so read-only/default-backed native calls still work on fresh snapshots. Skip eager `initialize()` during `OnPersist` engine construction and let `ContractManagement::on_persist` remain the single source of native initialization side effects for initialize blocks/hardfork activations.

**Tech Stack:** Rust, Cargo test, `neo-core` native contracts and application engine.

### Task 1: Add a failing regression test for Faun lifecycle

**Files:**
- Modify: `neo-core/src/smart_contract/native/policy_contract/tests.rs`
- Check: `neo-core/src/smart_contract/application_engine/witness_and_misc.rs`

**Step 1: Write the failing test**

Add a regression test that constructs an `OnPersist` engine at a Faun activation height and asserts the stored exec-fee factor is scaled exactly once after `native_on_persist()`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p neo-core faun -- --nocapture`
Expected: FAIL because `register_native_contracts()` eagerly initializes `PolicyContract` before `ContractManagement::on_persist()` initializes it again.

### Task 2: Remove eager OnPersist initialization

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/witness_and_misc.rs`
- Reference: `neo-core/src/smart_contract/native/contract_management/native_impl.rs`

**Step 1: Implement the minimal fix**

Update `register_native_contracts()` so `OnPersist` engines still register native metadata in memory but skip calling `initialize()` eagerly. Preserve current behavior for all other trigger types.

**Step 2: Keep logging behavior coherent**

Avoid emitting initialization-error logs from the skipped branch; `ContractManagement::on_persist()` remains responsible for initialization-time failures during `OnPersist`.

### Task 3: Verify targeted protocol behavior

**Files:**
- Test: `neo-core/src/smart_contract/native/policy_contract/tests.rs`

**Step 1: Re-run the regression test**

Run: `cargo test -p neo-core faun -- --nocapture`
Expected: PASS with exec-fee factor scaled once.

**Step 2: Re-run focused policy/native tests**

Run: `cargo test -p neo-core policy_contract -- --nocapture`
Expected: PASS.

**Step 3: Re-run engine-trigger coverage if needed**

Run: `cargo test -p neo-core contract_management -- --nocapture`
Expected: PASS for native contract registration/persistence behavior.
