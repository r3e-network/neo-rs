# Codebase Simplification and Reuse Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reduce duplication and maintenance burden by reusing existing project modules and stable ecosystem crates while preserving Neo compatibility and behavior.

**Architecture:** Prioritize internal reuse first (shared helpers/modules), then replace bespoke patterns with existing Rust/std or already-adopted crates. Execute in small, verifiable slices with strict no-regression checks after each task.

**Tech Stack:** Rust 2024, Cargo workspace, std (`LazyLock`/`OnceLock` where applicable), existing workspace crates (`neo-core`, `neo-vm`, `neo-rpc`), `serde`, `lz4_flex`, Clippy, rustfmt.

### Task 1: Compression Path Consolidation

**Files:**
- Modify: `neo-core/src/persistence/serialization.rs`
- Reference: `neo-core/src/compression/mod.rs`
- Test: existing persistence serialization tests

**Step 1:** Route persistence LZ4 calls through `crate::compression::{compress_lz4, decompress_lz4}`.
**Step 2:** Keep error mapping and empty-input behavior consistent.
**Step 3:** Verify with `cargo test -p neo-core -q` and `cargo clippy --workspace --all-targets --profile test -- -D warnings`.

### Task 2: Interoperable Deserialization Dedup

**Files:**
- Modify: `neo-core/src/smart_contract/manifest/contract_event_descriptor.rs`
- Modify: `neo-core/src/smart_contract/manifest/contract_parameter_definition.rs`
- Modify: `neo-core/src/smart_contract/manifest/contract_method_descriptor.rs`
- Modify: `neo-core/src/smart_contract/native/policy_contract/mod.rs`
- Modify: `neo-core/src/smart_contract/native/hash_index_state.rs`
- Modify: `neo-core/src/smart_contract/native/notary.rs`
- Modify: `neo-core/src/smart_contract/native/transaction_state.rs`

**Step 1:** Replace nested `if let` chains with Rust 2024 chained `if let`.
**Step 2:** Use `let-else` for early-return parsing branches where clearer.
**Step 3:** Keep stack-item semantics unchanged.
**Step 4:** Verify with `cargo test -p neo-core -q`.

### Task 3: Shared Stack-Item Extraction Helpers

**Files:**
- Create: `neo-core/src/smart_contract/stack_item_extract.rs`
- Modify: `neo-core/src/smart_contract/mod.rs`
- Modify: selected `IInteroperable::from_stack_item` implementations in `neo-core/src/smart_contract/**`

**Step 1:** Add helper functions for common extraction patterns:
- `string`, `u8`, `u32`, `i32`, `i64`, `bytes`.
**Step 2:** Replace repeated parsing blocks in high-duplication files.
**Step 3:** Ensure error/default behavior is unchanged per type.
**Step 4:** Run targeted tests for touched smart-contract modules.

### Task 4: Std Primitive Adoption (`once_cell`/`lazy_static` Reduction)

**Files:**
- Modify: targeted files under `neo-core/src/**`, `neo-rpc/src/**`, `neo-node/src/**` currently using `once_cell::sync::Lazy` / `lazy_static!`
- Modify: workspace manifests if removals become possible

**Step 1:** Replace straightforward globals with `std::sync::LazyLock`.
**Step 2:** Keep `OnceCell` only where late initialization semantics are required.
**Step 3:** Remove dependency entries only after full migration and build verification.

### Task 5: P2P Message/Compression Duplication Pass

**Files:**
- Modify: `neo-core/src/network/p2p/message.rs`
- Modify: `neo-core/src/network/p2p/messages.rs`
- Modify: `neo-core/src/network/p2p/remote_node/routing.rs`
- Reference: `neo-core/src/compression/mod.rs`

**Step 1:** Consolidate repeated compression/decompression decision logic into reusable helper(s).
**Step 2:** Keep wire-format behavior exactly compatible.
**Step 3:** Verify with P2P-related tests and workspace check.

### Task 6: Workspace-Wide Verification Gate

**Step 1:** `cargo fmt --all --check`
**Step 2:** `cargo clippy --workspace --all-targets --profile test -- -D warnings`
**Step 3:** `cargo test --workspace -q`
**Step 4:** Summarize behavior-equivalence and dependency reductions.

