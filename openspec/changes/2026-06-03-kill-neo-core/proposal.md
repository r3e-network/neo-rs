# Kill `neo-core`: split into domain-focused crates ✅ COMPLETE

## Why

The `neo-core` crate was a leftover monolith from the early C# `Neo.csproj` port. After this change, every `neo-*` crate has one clear responsibility, depends only on the layer below, and is independently compilable, testable, and reusable. The workspace now matches the **polkadot-sdk / reth / substrate** convention.

## What Changes

### Foundation (Layer 0)

- **NEW `neo-error`** — authoritative `CoreError` / `CoreResult` for the whole workspace. Replaces the duplicate error types that previously lived inside `neo-core`.
- **NEW `neo-time`** — testable `TimeProvider` / `TimeSource`. Replaces `neo_core::time_provider::*`.

### Protocol (Layer 1)

- **NEW `neo-ledger-types`** — pure ledger / wire data types. Owns `Witness`. The canonical home for `Block` / `Header` / `Transaction` / `Signer` in future slices.

### Service (Layer 2)

- **NEW `neo-chain`** — pure block / chain validation (`BlockValidator`, `BlockValidationError`, `validate_merkle_root`, `validate_witness_scripts`, etc.). Has **zero** dependency on `neo-core`; the public API takes `&[UInt256]` hashes and `&Witness` references rather than concrete `Header` / `Transaction` types.

### Backward compatibility

- `neo-core/src/lib.rs` keeps thin `pub use neo_*::*;` re-exports for one release cycle so the historical `neo_core::Witness` / `neo_core::error::CoreError` / `neo_core::time_provider::TimeProvider` / `neo_core::validation::BlockValidator` import paths still resolve.

## Migration Table

| Old import path | New import path |
|-----------------|-----------------|
| `neo_core::CoreError` / `neo_core::error::CoreError` | `neo_error::CoreError` |
| `neo_core::CoreResult` / `neo_core::error::CoreResult` | `neo_error::CoreResult` |
| `neo_core::Result` | `neo_error::Result` |
| `neo_core::TimeProvider` | `neo_time::TimeProvider` |
| `neo_core::time_provider::*` | `neo_time::*` |
| `neo_core::Witness` | `neo_ledger_types::Witness` |
| `neo_core::witness::*` | `neo_ledger_types::witness::*` |
| `neo_core::validation::*` / `BlockValidator` | `neo_chain::block_validation::*` |

New code should import from the canonical crates. The re-exports in `neo_core::*` are temporary.

## Impact

**Code**: Net deletion of ~1,877 lines from `neo-core` (4 modules: `error.rs` 593, `time_provider.rs` 124, `witness.rs` 522, `validation.rs` 638).

**APIs**: Breaking change for any external consumer importing `neo_core::error::*` / `neo_core::time_provider::*` / `neo_core::witness::*` / `neo_core::validation::*`. Within the workspace, all consumers have been updated.

**Dependencies**: No new external dependencies. All four new crates use only existing workspace dependencies.

**Workspace**:

```
26 → 30 workspace members (added neo-error, neo-time, neo-ledger-types, neo-chain).
```

## Verification

- `cargo check --workspace` — **green** (0 errors).
- `cargo test --workspace --lib` — **2048 passed, 0 failed, 8 ignored** across 27 test suites.
- `neo-error` lib + doc tests: 7 unit + 1 doc — all green.
- `neo-time` lib + doc tests: 1 unit + 1 doc — all green.
- `neo-ledger-types` lib + doc tests: 8 unit + 1 doc — all green.
- `neo-chain` lib tests: 22 unit — all green.

## Internal cleanups (bonus)

- Fixed pre-existing macro bug: `impl_native_contract!` and `neo_native_contract_methods!` in `neo-core` referenced an unresolved `$neo_error::` placeholder left over from a half-finished earlier extraction. Now correctly emits `::neo_error::` paths.
- Moved the orphan-rule-violating `impl From<KeyBuilderError> for CoreError` out of `neo-core` and into `neo-error` (where it belongs).
- Centralized all `From<X> for CoreError` impls in `neo-error` and documented as TODO to move each one into the source crate once those crates are independently versioned (polkadot-sdk pattern).
- Cleaned up dead re-export shims from `neo-core` that no longer need to re-export anything (`error`, `time_provider`, `witness`, `validation` modules replaced by `pub use` re-exports).

## Capabilities

### New capabilities

- `neo-error-crate-authority`: `neo-error` is the single authoritative error type for the workspace.
- `neo-time-crate`: A dedicated Layer 0 crate providing a testable `TimeProvider` abstraction.
- `layered-crate-boundary`: Workspace enforces a strict 4-layer architecture (Foundation → Protocol → Service → Application).

### Modified capabilities

- `core-architecture`: The `neo-core` crate is no longer the catch-all "core" layer. It is now a thin compatibility facade over the four new focused crates.
