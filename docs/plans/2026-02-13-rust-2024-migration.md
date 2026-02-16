# Rust 2024 Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Upgrade the neo-rs workspace from Rust 2021 to Rust 2024 with passing compile/lint/test gates and aligned docs/tooling.

**Architecture:** Use a two-phase migration: compiler-assisted rewrites with `cargo fix --edition`, then manual cleanup for safety-sensitive and lint-sensitive areas. Keep behavior unchanged while upgrading edition/MSRV and tightening CI parity.

**Tech Stack:** Rust stable toolchain, Cargo workspace, Clippy, rustfmt, GitHub Actions.

### Task 1: Baseline Verification

**Files:**
- Modify: none
- Test: workspace compile and lint baselines

**Step 1: Verify clean tree**

Run: `git status --short`  
Expected: no output

**Step 2: Verify baseline compile**

Run: `cargo check --workspace`  
Expected: success on current 2021 manifests

**Step 3: Baseline lint snapshot**

Run: `cargo clippy --workspace --all-targets --profile test -- -D warnings`  
Expected: capture current warning/error baseline for comparison

### Task 2: Apply Edition 2024 Mechanical Migration

**Files:**
- Modify: `neo-core/**`, `neo-rpc/**`, `neo-node/**`, `neo-vm/**` (compiler-assisted edits)
- Test: workspace all targets

**Step 1: Run edition migration helper**

Run: `cargo fix --edition --workspace --all-targets --allow-dirty --allow-staged`  
Expected: automatic compatibility rewrites applied

**Step 2: Review generated diff**

Run: `git diff --stat`  
Expected: primarily mechanical rewrites, especially tail-expression scope-preserving transforms

### Task 3: Flip Workspace Manifests and MSRV

**Files:**
- Modify: `Cargo.toml`
- Modify: `tests/Cargo.toml`

**Step 1: Update root edition/MSRV**

Change in `Cargo.toml`:
- `edition = "2024"`
- `rust-version = "1.85"`
- `[workspace.metadata].msrv = "1.85.0"`

**Step 2: Update tests crate edition**

Change in `tests/Cargo.toml`:
- `edition = "2024"`

**Step 3: Validate compile**

Run: `cargo check --workspace`  
Expected: compile succeeds or surfaces explicit migration blockers

### Task 4: Fix Rust 2024 Hard Errors

**Files:**
- Modify: `neo-core/build.rs`

**Step 1: Fix `std::env::set_var` safety requirement**

Wrap `env::set_var("PROTOC", path)` in an `unsafe` block with a concise safety comment.

**Step 2: Re-run compile gate**

Run: `cargo check --workspace`  
Expected: full workspace success in 2024 mode

### Task 5: Restore Clippy `-D warnings`

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine_helper.rs`
- Modify: `neo-core/src/smart_contract/native/oracle_request.rs`
- Modify: `neo-core/src/ledger/blockchain/transaction.rs`
- Modify: `neo-core/src/ledger/memory_pool.rs`
- Modify: `neo-crypto/src/mpt_trie/trie.rs`

**Step 1: Replace `map_or(true, ...)` where applicable**

Use `.is_none_or(...)` for optional predicates.

**Step 2: Replace single-pattern `match` with `if let` where semantics are unchanged**

Target the compiler-generated rewrites that trigger `clippy::single_match`.

**Step 3: Remove `let-and-return` patterns**

Return expressions directly when no extra logic exists.

**Step 4: Re-run clippy gate**

Run: `cargo clippy --workspace --all-targets --profile test -- -D warnings`  
Expected: pass or produce a finite remaining list for iterative cleanup

### Task 6: Complete Test Compile and Runtime Spot Checks

**Files:**
- Modify: none (unless test compile reveals blockers)
- Test: `neo-node`, `neo-rpc`, `neo-core`, `neo-vm`

**Step 1: Compile all test targets**

Run: `cargo test --workspace --no-run`  
Expected: test target compilation succeeds

**Step 2: Run targeted high-value suites**

Run:
- `cargo test -p neo-node -- --test-threads=1`
- `cargo test -p neo-rpc`
- `cargo test -p neo-vm --test vm_integration_tests`

Expected: runtime regressions surfaced early in networking/VM-sensitive paths

### Task 7: Align Tooling, CI, and Documentation

**Files:**
- Modify: `rustfmt.toml`
- Modify: `docs/DEPLOYMENT.md`
- Modify: `docs/STYLE.md`
- Modify: `fuzz/Cargo.toml` (if fuzz target should also be 2024)

**Step 1: Update formatting edition**

Set `rustfmt.toml` `edition = "2024"`.

**Step 2: Update published MSRV/edition docs**

Update Rust version requirements from 1.75 to 1.85+ in docs.

**Step 3: Re-run CI-equivalent local commands**

Run:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --profile test -- -D warnings`
- `cargo test --workspace`

Expected: parity with `.github/workflows/ci.yml`.

### Task 8: Post-Migration Modernization (Optional, Controlled)

**Files:**
- Modify: selected modules only where readability/perf improve

**Step 1: Introduce modern idioms incrementally**

Examples: `let-else`, `if-let` chains, `is_none_or`, reduced temporary allocations.

**Step 2: Enforce behavior-preserving refactors**

Each modernization PR must keep scope small and pass full gates.

**Step 3: Commit and document**

Run:
```bash
git add .
git commit -m "chore(rust): migrate workspace to edition 2024 and align MSRV/tooling"
```

