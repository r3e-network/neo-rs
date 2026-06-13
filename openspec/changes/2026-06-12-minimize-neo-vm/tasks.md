# Tasks — Minimize neo-vm

> Each task is a discrete, testable unit of work. Run `cargo test
> --workspace --lib` after each task to verify no regressions.

## 1. Move ScriptBuilder to neo-vm-rs

- [ ] 1.1 Copy `neo-vm/src/script_builder/mod.rs` to `neo-vm-rs/src/script_builder.rs` (adapt imports)
- [ ] 1.2 Copy `neo-vm/src/script_builder/redeem_script.rs` to `neo-vm-rs/src/script_builder/redeem_script.rs`
- [ ] 1.3 Add `pub mod script_builder;` to `neo-vm-rs/src/lib.rs`
- [ ] 1.4 Add necessary deps to `neo-vm-rs/Cargo.toml` (thiserror, num-bigint, num-traits)
- [ ] 1.5 Update `neo-vm/src/script_builder/mod.rs` to re-export from `neo_vm_rs::script_builder`
- [ ] 1.6 Verify `cargo check --workspace` passes
- [ ] 1.7 Verify `cargo test --workspace --lib` passes

## 2. Move VmError to neo-vm-rs

- [ ] 2.1 Create `neo-vm-rs/src/error.rs` with the `VmError` enum (feature-gated behind `std`)
- [ ] 2.2 Add `pub mod error;` to `neo-vm-rs/src/lib.rs`
- [ ] 2.3 Update `neo-vm/src/error.rs` to re-export from `neo_vm_rs::error`
- [ ] 2.4 Verify `cargo check --workspace` passes
- [ ] 2.5 Verify `cargo test --workspace --lib` passes

## 3. Move Interoperable trait to neo-vm-rs

- [ ] 3.1 Create `neo-vm-rs/src/interoperable.rs` with the `Interoperable` trait
- [ ] 3.2 Add `pub mod interoperable;` to `neo-vm-rs/src/lib.rs`
- [ ] 3.3 Update `neo-vm/src/interoperable.rs` to re-export from `neo_vm_rs::interoperable`
- [ ] 3.4 Verify `cargo check --workspace` passes
- [ ] 3.5 Verify `cargo test --workspace --lib` passes

## 4. Move rpc_json to neo-vm-rs

- [ ] 4.1 Refactor `neo-vm/src/rpc_json.rs` to work on `StackValue` instead of `StackItem`
- [ ] 4.2 Move to `neo-vm-rs/src/rpc_json.rs`
- [ ] 4.3 Add `pub mod rpc_json;` to `neo-vm-rs/src/lib.rs`
- [ ] 4.4 Update `neo-vm/src/rpc_json.rs` to re-export from `neo_vm_rs::rpc_json`
- [ ] 4.5 Verify `cargo check --workspace` passes
- [ ] 4.6 Verify `cargo test --workspace --lib` passes

## 5. Final verification

- [ ] 5.1 `cargo check --workspace` — green, 0 errors
- [ ] 5.2 `cargo test --workspace --lib` — all tests pass
- [ ] 5.3 Verify neo-vm line count decreased significantly
- [ ] 5.4 Verify neo-vm-rs line count increased appropriately
