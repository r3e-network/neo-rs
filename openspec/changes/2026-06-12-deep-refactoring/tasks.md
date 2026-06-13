# Tasks — Deep Refactoring

> Each task is a discrete, testable unit of work. Run `cargo test
> --workspace --lib` after each task to verify no regressions.

## 1. Remove unused `once_cell` dependency

- [x] 1.1 Remove `once_cell` from `neo-execution/Cargo.toml`
- [x] 1.2 Remove `once_cell` from `neo-native-contracts/Cargo.toml`
- [x] 1.3 Verify `cargo check --workspace` passes

## 2. Centralize storage prefix constants

- [x] 2.1 Create `neo-native-contracts/src/prefixes.rs` with all storage prefix constants
- [x] 2.2 Add `pub mod prefixes;` to `neo-native-contracts/src/lib.rs`
- [ ] 2.3 Update all native contracts to use centralized constants
- [ ] 2.4 Update `neo-blockchain/src/native_persist.rs` to use centralized constants
- [ ] 2.5 Update `neo-mempool/src/verification.rs` to use centralized constants
- [ ] 2.6 Verify `cargo check --workspace` passes

## 3. Add BinarySerializer default helpers

- [x] 3.1 Add `serialize_default()` and `deserialize_default()` to `neo-serialization`
- [ ] 3.2 Update all call sites in `neo-native-contracts` to use new helpers
- [ ] 3.3 Update all call sites in `neo-blockchain` to use new helpers
- [ ] 3.4 Verify `cargo check --workspace` passes

## 4. Consolidate duplicate NetworkError

- [ ] 4.1 Remove `neo-primitives/src/network_error.rs`
- [ ] 4.2 Update all imports to use `neo-network::NetworkError` or remove unused imports
- [ ] 4.3 Verify `cargo check --workspace` passes

## 5. Migrate RPC models to serde derive (incremental)

- [ ] 5.1 Start with simplest model: `rpc_validate_address_result.rs`
- [ ] 5.2 Migrate `rpc_peers.rs`
- [ ] 5.3 Migrate `rpc_version.rs`
- [ ] 5.4 Migrate `rpc_plugin.rs`
- [ ] 5.5 Verify `cargo test -p neo-rpc` passes after each migration

## 6. Final verification

- [ ] 6.1 `cargo check --workspace` — green, 0 errors
- [ ] 6.2 `cargo test --workspace --lib` — all tests pass
- [ ] 6.3 Verify line count decreased
