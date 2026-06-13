# Deep Refactoring: Reduce Duplication & Leverage Rust Ecosystem

## Why

The codebase has accumulated significant duplication and custom implementations that can be replaced with mature third-party crates or Rust built-in features. This refactoring targets the highest-impact areas to reduce code volume, improve consistency, and lower maintenance burden.

## What Changes

### 1. Centralize Storage Prefix Constants (HIGH impact)
40+ storage prefix constants are redefined across 8 files. Move them to a single location (`neo-native-contracts/src/prefixes.rs`) and re-export.

### 2. Add BinarySerializer Default Helpers (HIGH impact)
`BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())` is repeated hundreds of times. Add `serialize_default()` and `deserialize_default()` helpers.

### 3. Migrate RPC Models to Serde Derive (MEDIUM impact)
26 RPC model structs have hand-written `to_json()`/`from_json()` methods (~2000 lines). Migrate to `#[derive(Serialize, Deserialize)]`.

### 4. Remove Unused `once_cell` Dependency (LOW impact)
`once_cell` is listed in 2 Cargo.toml files but never imported. Remove it.

### 5. Unify Error Return Types (MEDIUM impact)
36+ `from_json` methods return `Result<T, String>` instead of `CoreResult<T>`. Unify to use typed errors.

### 6. Consolidate Duplicate Error Types (LOW impact)
`NetworkError` is defined in both `neo-primitives` and `neo-network`. Remove the `neo-primitives` version.

### 7. Migrate Manifest JSON to Serde Derive (MEDIUM impact)
7 manifest files have hand-written JSON serialization (~500 lines). Migrate to serde derive.

## Impact

**Codebase**: ~3000-4000 lines of boilerplate removed
**APIs**: RPC models gain serde compatibility; no behavioral changes
**Dependencies**: Remove `once_cell`; no new dependencies needed
**Testing**: All existing tests must continue to pass

## Non-goals

- This change does NOT change protocol behavior
- This change does NOT restructure the crate hierarchy
- This change does NOT modify the VM execution model
