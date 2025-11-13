# Neo VM Parity Checklist (Rust vs. C#)

## Summary

The current `neo-vm` crate models only a minimal subset of the Neo N3 Virtual Machine. The reference C# implementation (`src/Neo.VM`) exposes a comprehensive opcode set, execution engine infrastructure, and host interaction surface. This document captures the gaps and the parity work required to bring the Rust VM in line with the C# runtime while keeping an idiomatic Rust design.

## Components to Map

1. **Opcode Surface (`OpCode.cs`)**
   - Constants push (`PUSHINT8`…`PUSH32`, `PUSHDATA*`, `PUSH*`).
   - Flow control (`JMP`, `JMPIF`, `TRY`, `ENDTRY`, `CALL`, `RET`, etc.).
   - Stack manipulation (`DUP`, `SWAP`, `REVERSE*`, `ROT`, `SAME`, etc.).
   - Splice and bitwise operators (`CAT`, `SUBSTR`, `LEFT`, `RIGHT`, `MEMCPY`, `NOT`, `AND`, `OR`, `XOR`).
   - Arithmetic (`INC`, `DEC`, `ADD`, `SUB`, `MUL`, `MOD`, `POW`, `SQRT`, etc.), including BigInteger/decimal support.
   - Comparisons and boolean logic (`LOCALT`, `WITHIN`, `NUMEQUAL`, etc.).
   - Compound types (`NEWARRAY`, `NEWSTRUCT`, `PACK`, `UNPACK`, `APPEND`, `REMOVE`, `HASKEY`, `ITER*` family).
   - Type conversion (`ISTYPE`, `CONVERT`, `ISNULL`, `INSTANCEOF`).
   - Context/pointer opcodes (`PUSHA`, `CALLA`, `JMP*L`, pointer arithmetic).
   - Exception handling (`TRY`, `ENDTRY`, `ENDFINALLY`, `THROW`, `ASSERT`).
   - Syscall alias opcodes present in the C# VM.

2. **Execution Engine (`ExecutionEngine.cs`)**
   - **Stacks:** Invocation stack, evaluation stack, alternate stack, slot management, `StackItem` hierarchy.
   - **Gas accounting:** per opcode cost, interop fees, checks against gas limit, per-instruction deduction.
   - **Triggers and state:** engine state machine (HALT/FAULT/BREAK), breakpoints, notifications.
   - **Interop service table:** registration of syscalls, `InteropDescriptor`, `ApplicationEngine` integration.
   - **Script container:** access to transaction/block/contract context, `IScriptContainer`.
   - **Call flags and script hashes:** call flag propagation, `CurrentContext`, `CallingContext`.
   - **Exception handling:** TRY/CATCH/FINALLY semantics, error propagation.

3. **Stack Item Types (`StackItem.cs` family)**
   - Primitive types (Boolean, Integer, ByteString, Buffer).
  - Composite types (Array, Struct, Map, Buffer, Pointer, InteropInterface, Iterator).
   - Reference semantics (mutable structs, enumerators, snapshots).
   - Serialization/deserialization and JSON/Binary conversion.

4. **Interop Services (C# `ApplicationEngine` / `InteropService.cs`)**
   - Built-in syscall IDs and descriptors.
   - Native contract interop registration.
   - Invocation fee calculation, permission checks.

## Current Rust Implementation Gaps

- **Opcode coverage:** `neo-vm/src/instruction.rs` defines only `PushInt/Bool/Bytes`, arithmetic (`Add/Sub/Mul/Div/Mod/Negate/Inc/Dec/Sign/Abs`), logic (`And/Or/Not/Xor`), simple stack ops (`Store/Load/Dup/Swap/Drop/Over/Pick/Roll`), comparisons, shifts, type conversions, `Syscall`, `Jump`, `JumpIfFalse`, `CallNative`, and `Return`.
- **No gas accounting:** execution loop never charges gas or checks limits.
- **Single evaluation stack:** no alternate stack, invocation stack, or context frames.
- **No exception handling:** TRY/CATCH opcodes absent; errors result in immediate `VmError`.
- **No interop service table:** `Syscall` dispatch is placeholder; there is no descriptor registry or call flags.
- **No pointer/persisted context:** `PUSHA`, pointer arithmetic, context switching missing.
- **No iterator/collection opcodes:** arrays, maps, iterators unsupported.
- **No contract triggers or script container interaction.**
- **No `StackItem` abstraction:** `VmValue` is limited to `Int`, `Bool`, `Bytes`, `Null`.

## Rust Implementation Plan

1. **Opcode Expansion**
   - Generate a Rust enum mirroring `OpCode.cs` (consider code generation to stay in sync).
   - Implement decoding helpers for operand sizes (Size, SizePrefix) akin to C# attributes.
   - Introduce feature flags for debugging/breakpoints similar to `neo-vm`.

2. **StackItem Hierarchy**
   - Design `StackItem` trait with conversions to/from `VmValue`.
   - Implement array/map/struct/pointer/iterator types with reference semantics (shared ownership via `Rc<RefCell<...>>` or arena).

3. **Execution Engine Core**
   - Introduce `ExecutionContext`/`CallFrame` structure storing instruction pointer, script, locals.
   - Maintain invocation stack and alternate stack; expose API similar to `ExecutionEngine`.
   - Add gas accounting: assign per opcode cost, track `gas_consumed`, enforce `gas_limit`.
   - Implement try/catch/finally control flow, HALT/FAULT states, breakpoints.
   - Support pointer operations (`PUSHA` + relative addressing).

4. **Interop Service Table**
   - Define `InteropDescriptor` struct (name, handler, price, allowed triggers/call flags).
   - Build registry and dispatch mechanism; integrate with gas accounting.
   - Provide hooks for native contract registration from `neo-contract`.

5. **Script Container & Trigger Support**
   - Add `ScriptContainer` trait (transaction, block, oracle request, etc.).
   - Model trigger enum (Application, Verification, etc.) and propagate call flags.
   - Ensure `ApplicationEngine`-like wrapper to mediate runtime operations.

6. **Collections & Iterators**
   - Implement collection opcodes using new StackItem types.
   - Provide iterators and enumerators with lazy evaluation semantics.

7. **Testing & Parity Validation**
   - Port C# opcode unit tests (golden input/output) to Rust.
   - Build execution trace comparison harness against C# NeoVM.
   - Add gas consumption tests to ensure parity.

## Deliverables

- Updated `neo-vm` crate with full opcode support and execution engine parity.
- Shared interop interfaces usable by `neo-contract`, `neo-runtime`, and `neo-node`.
- Automated parity tests comparing Rust outputs to C# golden vectors.
- Documentation of any intentional deviations (e.g., idiomatic error handling or ownership semantics).

## Status Snapshot

- ✅ `System.Storage.Find/Next` now dispatches through the VM’s syscall layer into the host with deterministic iterator ordering, and iterator values are surfaced as `VmValue::Array` pairs so contracts can manipulate them without custom decoding.
- ⚠️ Full StackItem hierarchy (struct/map/iterator) plus BinarySerializer-backed deserialization is still pending; iterator values are limited to byte-array pairs for now.

## Follow-up

This checklist is part of the broader parity plan. Similar documents are required for consensus, runtime, networking, wallet, contract, and crypto crates. Once specs are agreed, feature development can proceed in tracked milestones. 
