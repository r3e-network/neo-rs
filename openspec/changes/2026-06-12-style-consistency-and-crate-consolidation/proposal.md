# Style Consistency & Crate Consolidation

## Why

The neo-rs codebase is protocol-complete for Neo N3 v3.10.0 (all 11 native contracts, all hardforks, all P2P messages, dBFT consensus, NEP-17, serialization parity). However, the codebase has accumulated style inconsistencies across its 32 workspace members that make it look like different projects stitched together. This change brings uniformity and reduces unnecessary crate count.

**Current problems:**
- 15 of 30 crates have no lint directives at all; 2 crates actively suppress warnings; 1 crate has contradictory `warn`/`allow` for `missing_docs`
- The `impl_default_via_new!` macro is defined identically in 3 separate crates with mixed import paths across 17+ call sites
- 24 internal dependencies across 6 Cargo.toml files use hardcoded `path =` instead of `workspace = true`
- 3 competing error strategies (`CoreError`, per-crate `thiserror`, `anyhow`) with no clear boundary rules
- 4 different test organization patterns with no standard
- 3 dead empty directories (`neo-data-cache`, `neo-redeem-script`, `neo-events`) left from earlier refactors
- `fuzz/Cargo.toml` references stale version `0.7.1` while workspace is at `0.7.2`
- `neo-vm/Cargo.toml` is missing standard package metadata fields

## What Changes

### 1. Lint directive standardization
Every crate gets `#![deny(unsafe_code)]` + `#![warn(missing_docs)]` in its `lib.rs` (or `main.rs` for `neo-node`). Crates that genuinely need unsafe get an explicit `#![allow(unsafe_code)]` with a comment explaining why.

### 2. Macro deduplication
Remove duplicate `impl_default_via_new!` definitions from `neo-primitives` and `neo-storage`. Keep the canonical definition in `neo-io/src/macros.rs`. Update all 17+ call sites to use `neo_io::impl_default_via_new!`.

### 3. Dependency style uniformity
Convert all hardcoded `path =` internal dependencies to `workspace = true` across 6 Cargo.toml files. Add missing internal crates to `[workspace.dependencies]` if not already present.

### 4. Error handling convention
Document and enforce: `CoreError`/`CoreResult` for cross-crate errors, per-crate `thiserror` for domain errors that don't cross boundaries, `anyhow` only in `neo-node`. Standardize the import alias pattern to `use neo_error::{CoreError, CoreResult}` (no aliasing).

### 5. Dead directory cleanup
Delete `neo-data-cache/`, `neo-redeem-script/`, `neo-events/` directories.

### 6. Stale version fix
Update `fuzz/Cargo.toml` from `0.7.1` to `0.7.2`.

### 7. Package metadata completeness
Add missing `homepage`, `repository`, `documentation`, `keywords`, `categories`, `readme` to `neo-vm/Cargo.toml`.

### 8. Crate consolidation (2 merges)
- Merge `neo-script-builder` (937 lines) into `neo-vm` — natural home, all 12 consumers already depend on `neo-vm`, zero circular dependency risk
- Merge `neo-application-logs` (568 lines) into `neo-rpc` — tiny leaf plugin, only consumed by `neo-rpc` and `neo-node`, zero circular dependency risk

## Impact

**Codebase**: ~30 files modified across lint directives, Cargo.toml fixes, and macro imports. 2 crates absorbed (net -2 workspace members). 3 dead directories deleted.

**APIs**: No behavioral changes. Public API surface unchanged except:
- `neo_script_builder` path becomes `neo_vm::script_builder`
- `neo_application_logs` path becomes `neo_rpc::application_logs`
- `impl_default_via_new!` import path unified to `neo_io::impl_default_via_new!`

**Dependencies**: No new external dependencies. Internal dependency graph simplified by 2 crates.

**Testing**: All existing tests must continue to pass. No new tests needed (this is a style/organization change).

**Documentation**: Error handling convention documented in `CONTRIBUTING.md` or similar.

## Capabilities

### New Capabilities
- `style-guide`: documented lint directives, error handling conventions, and dependency style rules for all workspace crates

### Modified Capabilities
- `neo-vm`: absorbs `neo-script-builder` module
- `neo-rpc`: absorbs `neo-application-logs` module

## Non-goals

- This change does NOT add missing documentation (that's a separate effort)
- This change does NOT refactor `neo-rpc` into smaller crates (it's 39k lines but splitting is a larger effort)
- This change does NOT address the `neo-vm-rs` external sibling dependency
- This change does NOT change any protocol logic or behavior
- This change does NOT add new tests (existing tests verify correctness)
