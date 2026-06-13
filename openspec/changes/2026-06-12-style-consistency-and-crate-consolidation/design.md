## Context

The neo-rs workspace has 32 crates that evolved organically through a major refactor (kill-neo-core, reth-style service architecture). The refactoring prioritized protocol correctness and architecture, leaving style consistency as a follow-up. This change addresses that follow-up.

**Current State**: Protocol-complete for Neo N3 v3.10.0. 1,110+ passing lib tests. Zero production `todo!()`/`unimplemented!()`. Code style is inconsistent across crates.

**Constraints**: Must not break any existing tests. Must not change protocol behavior. Must maintain the 4-layer architecture (Foundation → Protocol → Service → Application).

## Goals / Non-Goals

**Goals:**
- Uniform lint directives across all 30 library crates + 1 binary crate
- Single canonical `impl_default_via_new!` macro definition
- All internal deps use `workspace = true`
- Clear error handling convention documented and followed
- Remove dead directories and stale references
- Merge `neo-script-builder` into `neo-vm` and `neo-application-logs` into `neo-rpc`
- Reduce workspace from 32 to 30 members (excluding tests/benches)

**Non-Goals:**
- Adding missing doc comments (separate effort)
- Splitting `neo-rpc` (39k lines, too large for this change)
- Changing `neo-vm-rs` external dependency (requires sibling repo coordination)
- Reorganizing test structure (4 patterns can coexist)
- Changing any protocol logic

## Decisions

### D1: Lint directive standardization
**Decision**: All crates get `#![deny(unsafe_code)]` + `#![warn(missing_docs)]`. Crates needing unsafe get `#![allow(unsafe_code)]` with a `// SAFETY:` comment.
**Rationale**: Consistent lint surface. `deny(unsafe_code)` is the safe default; explicit allows document the exception.
**Alternatives**: `warn` instead of `deny` for unsafe (too permissive, defeats the purpose).

### D2: Macro canonical home
**Decision**: Keep `impl_default_via_new!` in `neo-io/src/macros.rs`. Remove duplicates from `neo-primitives/src/lib.rs` and `neo-storage/src/lib.rs`.
**Rationale**: `neo-io` is the macro/utility crate. It's already the lowest-layer crate that all others depend on. The macro is a serialization concern.
**Alternatives**: Move to `neo-primitives` (would require `neo-primitives` to depend on `neo-io` for the `impl_error_from!` re-export, breaking the "zero neo-* deps" rule for primitives).

### D3: Error handling boundaries
**Decision**:
- Cross-crate errors: `CoreError`/`CoreResult` from `neo-error`
- Crate-internal domain errors: per-crate `thiserror` types (e.g., `ConsensusError`, `NetworkError`)
- Application boundary: `anyhow` only in `neo-node/src/main.rs`
- Import style: `use neo_error::{CoreError, CoreResult}` (no aliasing to `Error`/`Result`)
**Rationale**: `CoreError` is the workspace-wide vocabulary. Per-crate `thiserror` types are for domain-specific errors that don't cross the crate boundary. `anyhow` is only for the binary entry point.
**Alternatives**: Single error type everywhere (too rigid). `anyhow` everywhere (loses type safety).

### D4: Merge strategy for neo-script-builder → neo-vm
**Decision**: Move `neo-script-builder/src/` contents into `neo-vm/src/script_builder/`. Add `pub mod script_builder` to `neo-vm/src/lib.rs`. Update all 12 consumers to use `neo_vm::script_builder::*` instead of `neo_script_builder::*`. Delete `neo-script-builder/` directory and remove from workspace.
**Rationale**: `neo-script-builder` depends on `neo-vm-rs` (opcodes, StackValue) and is only 937 lines. All 12 consumers already depend on `neo-vm`. No circular dependency risk.
**Alternatives**: Keep separate (no benefit, adds crate count). Merge into `neo-vm-rs` (would pollute the pure-VM boundary).

### D5: Merge strategy for neo-application-logs → neo-rpc
**Decision**: Move `neo-application-logs/src/` contents into `neo-rpc/src/application_logs/`. Add `pub mod application_logs` to `neo-rpc/src/lib.rs` behind the existing optional dependency. Update `neo-node/Cargo.toml` to reference `neo-rpc` instead of `neo-application-logs`. Delete `neo-application-logs/` directory and remove from workspace.
**Rationale**: `neo-application-logs` is 568 lines, a leaf plugin consumed only by `neo-rpc` (optional) and `neo-node` (optional). It has 16 dependencies for very little logic.
**Alternatives**: Keep separate (no benefit). Merge into `neo-node` (would move logic away from where it's queried).

## Risks / Trade-offs

**[Risk: Merge breaks consumers]** → Mitigation: Run full `cargo test --workspace --lib` after each merge. Each merge is a mechanical path update.

**[Risk: Lint directives surface hidden issues]** → Mitigation: Run `cargo clippy --workspace` after adding directives. Fix or add targeted `#[allow(...)]` with comments.

**[Risk: Import path changes break downstream code]** → Mitigation: Add `pub use` re-exports in old paths as temporary shims if needed, then remove in a follow-up.

**[Trade-off: Merge vs Keep separate]** → Merging reduces crate count and simplifies the dependency graph, but increases the size of `neo-vm` (by ~937 lines) and `neo-rpc` (by ~568 lines). Both target crates are already large, so the relative increase is small.

**[Trade-off: Strict lint vs Gradual adoption]** → Strict lint (`deny`) catches issues immediately but may require more upfront work. Gradual adoption (`warn`) allows incremental fixing but risks accumulating new violations. Decision: strict for new code, existing violations get targeted allows with TODO comments.
