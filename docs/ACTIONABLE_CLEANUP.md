# Actionable Cleanup Tasks

**Generated**: 2024-12-14
**Status**: Ready for execution

## Completed in This Session

### 1. ✅ neo-telemetry Warnings Fixed

**Files modified:**

- `neo-telemetry/src/metrics.rs` - Removed unused imports
- `neo-telemetry/src/health.rs` - Added `#[allow(dead_code)]` for public API types

### 2. ✅ Documentation Added

**Files created/updated:**

- `docs/CRATE_DEPENDENCY_AUDIT.md` - Complete dependency graph analysis
- `docs/ARCHITECTURE_GAP_ANALYSIS.md` - Updated with final status

## Remaining Low-Priority Tasks

### Task A: Fix Clippy MSRV Warnings (LOW PRIORITY)

Several crates use features from Rust versions newer than the declared MSRV 1.75.0.

**Affected crates:**

- neo-vm
- neo-core

**Options:**

1. Update MSRV to 1.82+ in `Cargo.toml`
2. Rewrite code to use older APIs

**Recommendation:** Update MSRV since production deployments will use newer Rust.

### Task B: Minor Clippy Suggestions (LOW PRIORITY)

```bash
# Auto-fix simple suggestions
cargo clippy --fix --lib -p neo-config
cargo clippy --fix --lib -p neo-mempool
cargo clippy --fix --lib -p neo-chain
```

**Types of issues:**

- `map_or` simplifications
- Unnecessary type casts
- `or_insert_with` patterns

### Task C: Consider neo-io + neo-json Consolidation (DEFERRED)

**Current state:**

- `neo-io` (1,200 LOC) - Serialization traits, caching
- `neo-json` (500 LOC) - JSON types only

**Analysis:**

- Both have zero neo-\* dependencies
- Could be merged into neo-primitives
- Low impact, not urgent

**Decision:** DEFERRED - Not worth the churn

## Architecture Health Summary

| Area           | Status   | Notes                          |
| -------------- | -------- | ------------------------------ |
| Build          | ✅ PASS  | Zero errors                    |
| Tests          | ✅ PASS  | 118 tests pass                 |
| Warnings       | ✅ FIXED | All critical warnings resolved |
| TODO/FIXME     | ✅ CLEAN | No outstanding markers         |
| unimplemented! | ✅ CLEAN | No placeholder code            |
| Circular deps  | ✅ NONE  | Clean dependency DAG           |

## Next Steps for Feature Work

With the architecture stabilized, focus should shift to:

1. **Feature completion** - Implement missing C# parity features
2. **Integration tests** - Add end-to-end testing
3. **Documentation** - API docs and user guides
4. **Performance** - Benchmarks and optimization

## Files Modified in This Session

```
neo-telemetry/src/metrics.rs        - Fixed unused imports
neo-telemetry/src/health.rs         - Added dead_code allow
docs/CRATE_DEPENDENCY_AUDIT.md      - NEW: Full dependency analysis
docs/ARCHITECTURE_GAP_ANALYSIS.md   - Updated status
docs/ACTIONABLE_CLEANUP.md          - NEW: This file
```
