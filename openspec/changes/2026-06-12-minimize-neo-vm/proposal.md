# Minimize neo-vm: Move Common Logic to neo-vm-rs

## Why

`neo-vm` is the host-layer VM for neo-rs, while `neo-vm-rs` is the common, general-purpose NeoVM implementation designed for use everywhere (PolkaVM, SP1, RISC-V proving runtimes, etc.). Currently `neo-vm` contains logic that is pure VM semantics and could be shared via `neo-vm-rs`, creating duplication and limiting reusability.

**Goal**: Make `neo-vm` as thin as possible — only host-specific concerns (reference counting, GC, interop host callbacks, gas metering) should remain. All pure VM logic should live in `neo-vm-rs`.

## What Changes

### 1. Move `ScriptBuilder` to neo-vm-rs
The `ScriptBuilder` is pure bytecode construction logic. It already depends only on `neo_vm_rs::OpCode` and `StackValue`. Move it to `neo-vm-rs/src/script_builder/` and re-export from `neo-vm`.

### 2. Move `VmError` to neo-vm-rs
The rich error enum (`VmError`) is pure VM error taxonomy. Move it to `neo-vm-rs/src/error.rs` behind a feature gate (`std`), replacing the bare `String` errors currently used in the interpreter.

### 3. Move `Interoperable` trait to neo-vm-rs
The `Interoperable` trait is a pure interface for smart-contract state round-tripping. Move it to `neo-vm-rs`.

### 4. Move `rpc_json` rendering to neo-vm-rs
The JSON-RPC rendering of stack items is pure serialization logic. Refactor to work on `StackValue` instead of `StackItem` and move to `neo-vm-rs`.

### 5. Consolidate exception handling
The try/catch/finally state machine is duplicated between `neo-vm`'s `ExecutionContext` and `neo-vm-rs`'s `VmContext`. Ensure `neo-vm` delegates to `neo-vm-rs`'s implementation.

### 6. Move `encode_integer` usage
Ensure `neo-vm` uses `neo_vm_rs::encode_integer` everywhere instead of any local reimplementation.

## Impact

**neo-vm**: Shrinks by ~1500-2000 lines (ScriptBuilder ~900, VmError ~200, rpc_json ~400, Interoperable ~50)
**neo-vm-rs**: Grows by ~1500-2000 lines but gains widely-useful functionality
**Consumers**: No API changes — `neo-vm` re-exports everything

## Non-goals

- This change does NOT merge neo-vm into neo-vm-rs (neo-vm has host-specific concerns that must stay separate)
- This change does NOT change the ExecutionEngine architecture (that's a larger refactor)
- This change does NOT affect protocol behavior
