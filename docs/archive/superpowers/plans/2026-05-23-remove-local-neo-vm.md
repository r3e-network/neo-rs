# Remove Local NeoVM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove duplicated `neo-core/src/neo_vm` VM ownership from `neo-rs` and make VM-facing code use `neo-vm-rs` types and semantics directly.

**Architecture:** Keep blockchain/application host behavior in `neo-core::smart_contract`, but move bytecode, value, final state, opcode, syscall hash, and interpreter semantics to `neo-vm-rs`. Local adapters may remain only outside the `neo_vm` tree while `neo-vm-rs` lacks public APIs for Neo N3 host-only concerns such as gas pricing, debug state, object identity, and native contract dispatch.

**Tech Stack:** Rust 2021 workspace, `neo-core`, `neo-rpc`, `neo-tests`, sibling path dependency `../neo-vm-rs` while both repositories are migrated together locally.

**Current Status (2026-05-23):**
- `neo-rs` is cloned at `D:/Git/neo-rs`; `neo-vm-rs` is cloned at `D:/Git/neo-vm-rs`.
- `neo-vm-rs` now owns the historical Integer/Boolean `SIZE` and `PICKITEM` semantics, full `VmState` and `StackItemType` byte mappings, the canonical `ExecutionEngineLimits` defaults, the shared exception handling context/state shape, the insertion-ordered `VmOrderedDictionary` map backing type, the shared `Instruction`/operand parser, shared script validation/jump target helpers, Tarjan graph traversal, compound stack item ID allocation, and shallow stack-item view classification used by migrated opcode paths.
- `neo-rs` routes the touched primitive collection paths through `neo_vm_rs::semantics::collections`, builds NeoToken committee payloads as `neo_vm_rs::StackValue`, re-exports standalone script validation from `neo-vm-rs`, no longer keeps the local primitive/compound view shims, graph/id compatibility facades, `VMState`, `ExecutionEngineLimits`, exception handling frame, `StackItemType`, `Instruction`, or `VmOrderedDictionary` facades, and imports shared VM types directly from `neo-vm-rs` outside the local runtime tree where practical.
- Focused checks pass: `cargo test` in `neo-vm-rs`, `cargo check -p neo-core`, `cargo check -p neo-rpc`, and `cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture`.
- Full deletion of `neo-core/src/neo_vm` is still blocked by local stateful runtime surfaces not yet provided by `neo-vm-rs`: execution contexts, gas hooks, step/debug state, reference-counted object identity, local `StackItem` compound identity, local `Script` caching/validation ownership, and ApplicationEngine host/native-contract dispatch.

---

### Task 1: Fix Current Migration Sentinel Failures

**Files:**
- Modify: `neo-core/src/neo_vm/jump_table/compound.rs`
- Modify: `neo-core/src/smart_contract/native/neo_token/governance.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Run the failing sentinel**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
```

Observed: four Windows-sensitive/source-boundary sentinel failures before implementation.

- [x] **Step 2: Normalize source-inspection sentinels for Windows line endings**

Updated `tests/tests/no_local_neo_vm_dependency.rs` to normalize CRLF/LF before substring checks.

- [x] **Step 3: Serialize NeoToken committee/candidate ABI results through `StackValue`**

Updated committee, candidate, and validator result helpers in `neo_token/governance.rs` so ABI arrays are built as `neo_vm_rs::StackValue` before serialization.

- [x] **Step 4: Route primitive `SIZE`/`PICKITEM` through `neo-vm-rs`**

Updated `neo-core/src/neo_vm/jump_table/compound.rs` so local primitive Integer/Boolean collection behavior delegates to `neo_vm_rs::semantics::collections`.

- [x] **Step 5: Verify sentinel passes**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
```

Result: all 100 sentinel tests pass.

### Task 2: Add Missing `neo-vm-rs` Regression Coverage From `neo-rs` History

**Files:**
- Modify: `D:/Git/neo-vm-rs/tests/interpreter_smoke.rs`
- Modify: `D:/Git/neo-vm-rs/tests/shared_semantics.rs`

- [x] **Step 1: Add tests for historical SIZE/PICKITEM bug coverage**

Add explicit tests covering `SIZE` on Integer/Boolean and `PICKITEM` on Integer/Boolean byte spans, including negative and out-of-range primitive byte indexes. These guard bug #11 and b4f8bbb6 when `neo-rs` relies on external VM behavior.

- [x] **Step 2: Run `neo-vm-rs` tests**

```powershell
cargo test
```

Expected: all `neo-vm-rs` tests pass.

Result: all `neo-vm-rs` tests pass.

### Task 2.5: Move Shallow VM Type Mappings Into `neo-vm-rs`

**Files:**
- Modify: `D:/Git/neo-vm-rs/src/abi/execution.rs`
- Modify: `D:/Git/neo-vm-rs/src/abi/stack_value.rs`
- Modify: `D:/Git/neo-vm-rs/src/abi/mod.rs`
- Modify: `D:/Git/neo-vm-rs/src/lib.rs`
- Modify: `neo-core/src/neo_vm/vm_state.rs`
- Modify: `neo-core/src/neo_vm/stack_item/stack_item_type.rs`
- Modify: `neo-rpc/src/client/models/vm_state_utils.rs`
- Modify: `neo-rpc/src/server/smart_contract/helpers.rs`
- Test: `D:/Git/neo-vm-rs/tests/shared_semantics.rs`
- Test: `D:/Git/neo-vm-rs/tests/boundary_codecs.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Promote full `VmState` byte mapping to `neo-vm-rs`**

Added `None`, `Halt`, `Fault`, and `Break` state support, C#-compatible uppercase associated constants, byte mapping helpers, final-state helpers, and coverage.

- [x] **Step 2: Promote `StackItemType` byte mapping to `neo-vm-rs`**

Added the canonical stack item type enum and re-exported it from `neo-vm-rs`.

- [x] **Step 3: Replace local neo-rs mapping files with re-exports**

`neo-core/src/neo_vm/vm_state.rs` and `neo-core/src/neo_vm/stack_item/stack_item_type.rs` now re-export `neo-vm-rs` types.

- [x] **Step 4: Keep RPC state formatting on final states only**

RPC helpers now use `VmState::final_name()` instead of local conversion code.

### Task 2.6: Move Execution Limit Defaults Into `neo-vm-rs`

**Files:**
- Modify: `D:/Git/neo-vm-rs/src/vm/limits.rs`
- Modify: `D:/Git/neo-vm-rs/src/vm/mod.rs`
- Modify: `D:/Git/neo-vm-rs/src/lib.rs`
- Modify: `neo-core/src/neo_vm/execution_engine_limits.rs`
- Modify: `neo-core/src/neo_vm/jump_table/numeric.rs`
- Test: `D:/Git/neo-vm-rs/tests/shared_semantics.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Promote `ExecutionEngineLimits` to `neo-vm-rs`**

Added the canonical NeoVM execution limit struct, defaults, and validation helpers to `neo-vm-rs`, preserving the Neo N3 `u16::MAX` item/comparable-size limits.

- [x] **Step 2: Replace local neo-rs limit implementation with a re-export**

`neo-core/src/neo_vm/execution_engine_limits.rs` now re-exports `neo_vm_rs::ExecutionEngineLimits`. Numeric opcodes translate shared limit errors into local `VmError` at the boundary.

- [x] **Step 3: Cover the new boundary with focused tests**

Added `neo-vm-rs` default/error coverage and updated the `neo-rs` migration sentinel to reject a local copied limits struct.

### Task 2.7: Move Exception Handling Frame Shape Into `neo-vm-rs`

**Files:**
- Create: `D:/Git/neo-vm-rs/src/vm/exception_handling.rs`
- Modify: `D:/Git/neo-vm-rs/src/vm/mod.rs`
- Modify: `D:/Git/neo-vm-rs/src/lib.rs`
- Modify: `neo-core/src/neo_vm/exception_handling_context.rs`
- Modify: `neo-core/src/neo_vm/exception_handling_state.rs`
- Test: `D:/Git/neo-vm-rs/tests/shared_semantics.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Promote exception handling context/state to `neo-vm-rs`**

Added `ExceptionHandlingContext` and `ExceptionHandlingState` to `neo-vm-rs` with the same try/catch/finally state shape used by the local execution context.

- [x] **Step 2: Replace local neo-rs exception type implementations with re-exports**

`neo-core/src/neo_vm/exception_handling_context.rs` and `neo-core/src/neo_vm/exception_handling_state.rs` now re-export `neo-vm-rs` types.

- [x] **Step 3: Guard the boundary**

Added `neo-vm-rs` shape coverage and a `neo-rs` migration sentinel that rejects local exception handling type copies.

### Task 2.8: Move Ordered Dictionary Map Backing Type Into `neo-vm-rs`

**Files:**
- Create: `D:/Git/neo-vm-rs/src/vm/ordered_dictionary.rs`
- Modify: `D:/Git/neo-vm-rs/src/vm/mod.rs`
- Modify: `D:/Git/neo-vm-rs/src/lib.rs`
- Modify: `neo-core/src/neo_vm/collections/ordered_dictionary.rs`
- Modify: `neo-core/src/neo_vm/stack_item/map.rs`
- Modify: `neo-core/src/neo_vm/stack_item/stack_item.rs`
- Modify: `neo-core/src/smart_contract/application_engine_helper.rs`
- Modify: `neo-core/src/smart_contract/binary_serializer.rs`
- Test: `D:/Git/neo-vm-rs/tests/shared_semantics.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Promote `VmOrderedDictionary` to `neo-vm-rs`**

Moved the insertion-ordered map backing type into `neo-vm-rs` using `alloc`/`core` imports so it remains compatible with the shared VM crate's `no_std + alloc` shape.

- [x] **Step 2: Replace the local neo-rs ordered dictionary implementation with a re-export**

`neo-core/src/neo_vm/collections/ordered_dictionary.rs` now re-exports `neo_vm_rs::VmOrderedDictionary`. Local map and smart-contract helper code import `neo_vm_rs::VmOrderedDictionary` directly where practical.

- [x] **Step 3: Guard insertion order and the direct boundary**

Added `neo-vm-rs` insertion-order coverage and a `neo-rs` sentinel that rejects a copied local ordered dictionary implementation.

### Task 2.9: Move Instruction Parsing Into `neo-vm-rs`

**Files:**
- Create: `D:/Git/neo-vm-rs/src/vm/instruction.rs`
- Modify: `D:/Git/neo-vm-rs/src/vm/mod.rs`
- Modify: `D:/Git/neo-vm-rs/src/lib.rs`
- Modify: `neo-core/src/neo_vm/instruction.rs`
- Modify: `neo-core/src/neo_vm/script.rs`
- Modify: `neo-core/src/neo_vm/error.rs`
- Modify: `neo-core/Cargo.toml`
- Test: `D:/Git/neo-vm-rs/tests/shared_semantics.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Promote `Instruction` and operand decoding to `neo-vm-rs`**

Added shared `Instruction`, `FromOperand`, `InstructionError`, typed `InstructionErrorKind`, parser support for fixed and prefixed operands, token helpers, and operand readers.

- [x] **Step 2: Replace the local neo-rs instruction implementation with a re-export**

`neo-core/src/neo_vm/instruction.rs` now re-exports the shared instruction types. `Script` parses bytecode through `neo_vm_rs::Instruction::parse`, and `neo-core` maps shared parse errors into `VmError::parse` while operand decode errors become `VmError::invalid_operand_msg`.

- [x] **Step 3: Remove the no-longer-needed direct smallvec dependency**

`neo-core` no longer depends directly on `smallvec` for instruction operands after the local instruction implementation was removed.

- [x] **Step 4: Guard parser ownership and focused behavior**

Added `neo-vm-rs` parser coverage and a `neo-rs` sentinel that rejects local operand metadata/parser copies.

- [x] **Step 5: Verify focused checks**

```powershell
cargo test
cargo check --no-default-features
cargo test -p neo-core --lib neo_vm::script::tests -- --nocapture
cargo test -p neo-core --lib script_validation::tests::exposes_instruction_metadata_for_disassembly_tools -- --nocapture
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo check -p neo-core
cargo check -p neo-rpc
```

Result: all listed checks pass. `git diff --check` reports only CRLF normalization warnings from Git on Windows.

### Task 2.10: Move Standalone Script Validation Into `neo-vm-rs`

**Files:**
- Create: `D:/Git/neo-vm-rs/src/vm/script_validation.rs`
- Modify: `D:/Git/neo-vm-rs/src/vm/mod.rs`
- Modify: `D:/Git/neo-vm-rs/src/lib.rs`
- Modify: `neo-core/src/script_validation.rs`
- Modify: `neo-core/src/neo_vm/script.rs`
- Test: `D:/Git/neo-vm-rs/tests/shared_semantics.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Promote script validation and target helpers to `neo-vm-rs`**

Added `parse_script_instructions`, `validate_script`, `validate_strict_script`, `ValidatedScript`, `ScriptInstruction`, `instruction_jump_target`, and `instruction_try_targets` to `neo-vm-rs`.

- [x] **Step 2: Use shared stack item tags for strict type validation**

Strict `NEWARRAY_T`/`ISTYPE`/`CONVERT` checks use `neo_vm_rs::StackItemType::from_byte` and `StackItemType::Any.to_byte()` rather than local byte lists.

- [x] **Step 3: Replace local validation with a compatibility re-export**

`neo-core/src/script_validation.rs` now re-exports the shared validator API. Local `Script::validate_strict`, `Script::get_jump_target`, and `Script::get_try_offsets` delegate to `neo-vm-rs`, so diagnostics now use the same next-instruction-relative jump target calculation as execution.

- [x] **Step 4: Guard the standalone validation boundary**

Updated the migration sentinel so standalone script validation must re-export `neo-vm-rs` validation and must not reintroduce local instruction parsing or opcode operand metadata parsing.

- [x] **Step 5: Verify focused checks**

```powershell
cargo test
cargo check --no-default-features
cargo test -p neo-core --lib script_validation::tests -- --nocapture
cargo test -p neo-core --lib neo_vm::script::tests -- --nocapture
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo check -p neo-core
cargo check -p neo-rpc
cargo fmt --check
```

Result: all listed checks pass. `git diff --check` reports only CRLF normalization warnings from Git on Windows.

### Task 2.11: Move Reference Graph Helpers Into `neo-vm-rs`

**Files:**
- Create: `D:/Git/neo-vm-rs/src/vm/graph.rs`
- Modify: `D:/Git/neo-vm-rs/src/vm/mod.rs`
- Modify: `D:/Git/neo-vm-rs/src/lib.rs`
- Modify: `neo-core/src/neo_vm/strongly_connected_components/tarjan.rs`
- Modify: `neo-core/src/neo_vm/stack_item/stack_item_vertex.rs`
- Modify: `neo-core/src/neo_vm/reference_counter.rs`
- Modify: `neo-core/src/neo_vm/stack_item/array.rs`
- Modify: `neo-core/src/neo_vm/stack_item/buffer.rs`
- Modify: `neo-core/src/neo_vm/stack_item/map.rs`
- Modify: `neo-core/src/neo_vm/stack_item/struct_item.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Promote graph and ID helpers to `neo-vm-rs`**

Added `Tarjan<T>` and `next_stack_item_id()` to `neo-vm-rs`. The Tarjan implementation is `no_std + alloc` friendly and uses `Vec`-backed state instead of `std::collections`.

- [x] **Step 2: Replace local helper implementations with re-exports**

`neo-core/src/neo_vm/strongly_connected_components/tarjan.rs` and `neo-core/src/neo_vm/stack_item/stack_item_vertex.rs` were first reduced to re-exports of the shared `neo-vm-rs` helpers, then removed in Task 2.12 after direct imports were in place.

- [x] **Step 3: Use shared helpers directly where practical**

`ReferenceCounter` imports `neo_vm_rs::Tarjan`; array, buffer, map, and struct stack item implementations import `neo_vm_rs::next_stack_item_id`.

- [x] **Step 4: Verify focused checks**

```powershell
cargo test graph -- --nocapture
cargo check --no-default-features
cargo test -p neo-core --lib neo_vm::reference_counter::tests -- --nocapture
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo check -p neo-core
cargo check -p neo-rpc
cargo fmt --check
```

Result: all listed checks pass. `git diff --check` reports only CRLF normalization warnings from Git on Windows.

### Task 2.12: Remove Unused Stack-Item View Shims And Graph/ID Facades

**Files:**
- Delete: `neo-core/src/neo_vm/stack_item/primitive_type.rs`
- Delete: `neo-core/src/neo_vm/stack_item/compound_type.rs`
- Delete: `neo-core/src/neo_vm/stack_item/stack_item_vertex.rs`
- Delete: `neo-core/src/neo_vm/strongly_connected_components/mod.rs`
- Delete: `neo-core/src/neo_vm/strongly_connected_components/tarjan.rs`
- Modify: `neo-core/src/neo_vm/stack_item/mod.rs`
- Modify: `neo-core/src/neo_vm/mod.rs`
- Modify: `neo-core/src/neo_vm/jump_table/compound.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Remove local primitive/compound view helper modules**

Deleted `primitive_type.rs` and `compound_type.rs` after replacing their only remaining opcode use with direct `StackItem::get_integer()` handling at the boundary.

- [x] **Step 2: Remove local graph and stack item ID facade modules**

Deleted `stack_item_vertex.rs` and the `strongly_connected_components` module after `ReferenceCounter` and compound stack item constructors imported `neo_vm_rs::{Tarjan, next_stack_item_id}` directly.

- [x] **Step 3: Tighten migration sentinels**

Updated `tests/tests/no_local_neo_vm_dependency.rs` to assert the deleted modules and facade directory do not exist, that direct `neo-vm-rs` imports remain in place, and that primitive `PICKITEM` invalid indexes fault through the shared boundary.

- [x] **Step 4: Verify focused checks**

```powershell
cargo check -p neo-core
cargo check -p neo-rpc
cargo test -p neo-core --lib neo_vm::reference_counter::tests -- --nocapture
cargo test -p neo-core --lib neo_vm::stack_item::stack_item::tests -- --nocapture
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo test
cargo fmt --check
git diff --check
```

Result: all listed checks pass in their respective repositories, with no local primitive/compound/graph/id compatibility facades left under `neo-core/src/neo_vm`. `git diff --check` reports only Git's Windows LF-to-CRLF normalization warnings.

### Task 2.13: Remove Ordered Dictionary Facade And Direct Shared Scalar Imports

**Files:**
- Delete: `neo-core/src/neo_vm/collections/mod.rs`
- Delete: `neo-core/src/neo_vm/collections/ordered_dictionary.rs`
- Modify: `neo-core/src/neo_vm/mod.rs`
- Modify: non-VM `neo-core` and `neo-rpc` callers of `VMState` and `ExecutionEngineLimits`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Remove the unused ordered dictionary compatibility module**

Deleted the local `neo_vm::collections` module and `OrderedDictionary` re-export after all runtime map code and smart-contract helpers used `neo_vm_rs::VmOrderedDictionary` directly.

- [x] **Step 2: Move non-runtime scalar callers to `neo-vm-rs`**

Changed non-`neo_vm` layers to import `neo_vm_rs::VmState` and `neo_vm_rs::ExecutionEngineLimits` directly instead of going through `crate::neo_vm` or `neo_core::neo_vm` compatibility paths.

- [x] **Step 3: Tighten migration sentinels**

Added a sentinel that rejects local `VMState` and `ExecutionEngineLimits` imports outside `neo-core/src/neo_vm`, and updated the ordered dictionary sentinel to require the local facade files and module export to be absent.

- [x] **Step 4: Verify focused checks**

```powershell
cargo fmt --check
cargo check -p neo-core
cargo check -p neo-rpc
cargo check -p neo-rpc --features server
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo test -p neo-rpc --features server --test rpc_blockchain_getrawtransaction_vmstate -- --nocapture
```

Result: listed checks pass, with any remaining local scalar imports confined to the still-local runtime tree. `git diff --check` reports only Git's Windows LF-to-CRLF normalization warnings.

### Task 2.14: Remove StackItemType Facade

**Files:**
- Delete: `neo-core/src/neo_vm/stack_item/stack_item_type.rs`
- Modify: `neo-core/src/neo_vm/stack_item/mod.rs`
- Modify: `neo-core/src/neo_vm/mod.rs`
- Modify: `neo-core` callers of `StackItemType`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Move callers to the canonical enum**

Changed local runtime code, smart-contract serializers, application log conversion, and runtime tests to import `neo_vm_rs::StackItemType` directly.

- [x] **Step 2: Remove the compatibility facade**

Deleted `neo-core/src/neo_vm/stack_item/stack_item_type.rs` and removed the `StackItemType` re-exports from `neo_vm::stack_item` and `neo_vm`.

- [x] **Step 3: Tighten migration sentinels**

Updated the shared scalar sentinel to reject local `StackItemType` imports outside `neo-core/src/neo_vm`, and changed the byte-tag sentinel to require the local facade file and module exports to be absent.

- [x] **Step 4: Verify focused checks**

```powershell
cargo fmt --check
cargo check -p neo-core
cargo check -p neo-rpc
cargo check -p neo-rpc --features server
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo test -p neo-core --lib neo_vm::stack_item::stack_item::tests -- --nocapture
cargo test -p neo-rpc --features server --test rpc_blockchain_getrawtransaction_vmstate -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.15: Remove Instruction Facade

**Files:**
- Delete: `neo-core/src/neo_vm/instruction.rs`
- Modify: `neo-core/src/neo_vm/mod.rs`
- Modify: local runtime, diagnostic, and helper callers of `Instruction`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Move callers to the canonical instruction type**

Changed local VM runtime modules, jump tables, interop service hooks, smart-contract diagnostics, RPC diagnostics, and helper tests to import `neo_vm_rs::Instruction` directly.

- [x] **Step 2: Remove the compatibility facade**

Deleted `neo-core/src/neo_vm/instruction.rs` and removed `neo_vm::instruction` plus the top-level `neo_vm::Instruction` re-export.

- [x] **Step 3: Tighten migration sentinels**

Updated the instruction migration sentinel to require the local facade file and module exports to be absent, and to reject `neo_vm::instruction::Instruction` compatibility imports.

- [x] **Step 4: Verify focused checks**

```powershell
cargo fmt --check
cargo check -p neo-core
cargo check -p neo-rpc
cargo check -p neo-rpc --features server
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo test -p neo-core --lib neo_vm::script::tests -- --nocapture
cargo test -p neo-core --test smart_contract_helper_tests -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.16: Remove VMState, Limits, And Exception Frame Facades

**Files:**
- Delete: `neo-core/src/neo_vm/vm_state.rs`
- Delete: `neo-core/src/neo_vm/execution_engine_limits.rs`
- Delete: `neo-core/src/neo_vm/exception_handling_context.rs`
- Delete: `neo-core/src/neo_vm/exception_handling_state.rs`
- Modify: `neo-core/src/neo_vm/mod.rs`
- Modify: local runtime and test callers of these shared VM types
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Move callers to the canonical shared types**

Changed runtime internals, package tests, RPC paths, smart-contract code, and docs to import `neo_vm_rs::{VmState, ExecutionEngineLimits, ExceptionHandlingContext, ExceptionHandlingState}` directly instead of local `neo_vm` compatibility paths.

- [x] **Step 2: Remove the compatibility facades**

Deleted the local `vm_state`, `execution_engine_limits`, `exception_handling_context`, and `exception_handling_state` files, plus their module declarations and top-level `neo_vm` re-exports.

- [x] **Step 3: Tighten migration sentinels**

Updated source-inspection sentinels to require those facade files and exports to be absent, and expanded non-VM-layer checks to reject local imports from package tests as well as source/RPC code.

- [x] **Step 4: Verify focused checks**

```powershell
cargo fmt --check
cargo check -p neo-core
cargo check -p neo-rpc
cargo check -p neo-rpc --features server
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo test -p neo-core --lib neo_vm::execution_context::tests -- --nocapture
cargo test -p neo-core --test notary_contract_tests --no-run
cargo test -p neo-core --features runtime --test tokens_tracker_nep17_csharp_parity --no-run
cargo test -p neo-rpc --features server --test rpc_blockchain_getrawtransaction_vmstate -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.17: Add A Direct `neo-vm-rs` ApplicationEngine Execution Boundary

**Files:**
- Create: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Modify: `neo-core/src/smart_contract/application_engine/mod.rs`
- Modify: `neo-core/src/smart_contract/application_engine/load_execute_storage.rs`
- Modify: `tests/tests/no_local_neo_vm_dependency.rs`
- Modify: sibling `../neo-vm-rs/tests/interpreter_smoke.rs`

- [x] **Step 1: Route safe ApplicationEngine scripts into `neo-vm-rs`**

Added a narrow direct interpreter path for syscall-free single-context scripts. The adapter prepares the script and result-count limit, charges opcode fees through `ApplicationEngine`, runs `neo_vm_rs::interpret_with_stack_and_syscalls_at*`, and maps `ExecutionResult` back into the existing VM state/result stack. Scripts needing host syscalls, `CALLT`, diagnostics, native pending calls, or pre-seeded evaluation-stack values still fall back to the local engine path.

- [x] **Step 2: Add migration and historical regression coverage**

Added a `neo-rs` sentinel/runtime test proving syscall-free `ApplicationEngine` execution crosses the `neo-vm-rs` interpreter boundary, and added a `neo-vm-rs` interpreter regression for strict `EQUAL` / `NOTEQUAL` primitive typing (`Integer(1)` vs `ByteString([1])`).

- [x] **Step 3: Verify focused checks**

```powershell
cargo fmt --check
cargo check -p neo-core
cargo check -p neo-rpc
cargo check -p neo-rpc --features server
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo test -p neo-core --test application_engine_runtime_tests -- --nocapture
cargo test -p neo-core --test application_engine_contract_tests -- --nocapture
cargo test -p neo-core --test verify_witnesses_tests -- --nocapture
git diff --check

# In ../neo-vm-rs:
cargo fmt --check
cargo test --test interpreter_smoke -- --nocapture
git diff --check
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core` and Git LF-to-CRLF warnings from the Windows checkout during `git diff --check`.

### Task 2.18: Bridge Simple Runtime Syscalls Through The Direct `neo-vm-rs` Boundary

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Modify: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing sentinel/runtime test**

Added `application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs`, which first failed because the external VM host rejected every `SYSCALL`.

- [x] **Step 2: Support read-only no-argument runtime syscalls in the external host**

Added a small `SyscallProvider` bridge for `System.Runtime.Platform`, `GetTrigger`, `GetNetwork`, `GetAddressVersion`, `GetTime`, and `GasLeft`. The bridge charges the registered syscall fee through `ApplicationEngine`, pushes values directly as `neo_vm_rs::StackValue`, and keeps unsupported syscalls, `CALLT`, diagnostics, native pending calls, and broader host/runtime behavior on the local fallback path.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs -- --nocapture
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_executes_syscall_free_scripts_through_neo_vm_rs -- --nocapture
cargo fmt --check
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.19: Remove Unused Primitive Stack Item Wrapper Modules

**Files:**
- Delete: `neo-core/src/neo_vm/stack_item/boolean.rs`
- Delete: `neo-core/src/neo_vm/stack_item/integer.rs`
- Delete: `neo-core/src/neo_vm/stack_item/null.rs`
- Modify: `neo-core/src/neo_vm/stack_item/mod.rs`
- Modify: `neo-core/src/neo_vm/stack_item/stack_item.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing deletion sentinel**

Added `unused_primitive_stack_item_wrappers_are_removed`, which failed while the Boolean/Integer/Null wrapper files and re-exports were still present.

- [x] **Step 2: Delete unused primitive wrappers**

Removed the Boolean, Integer, and Null wrapper modules and their stack-item facade re-exports. Replaced the only production dependency, `Integer::MAX_SIZE`, with a local `VM_INTEGER_MAX_SIZE` constant inside `stack_item.rs`; primitive values continue to use the main `StackItem` enum or `neo_vm_rs::StackValue`.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency unused_primitive_stack_item_wrappers_are_removed -- --nocapture
cargo test -p neo-core --lib neo_vm::stack_item -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.20: Remove The Local `ByteString` Wrapper Module

**Files:**
- Delete: `neo-core/src/neo_vm/stack_item/byte_string.rs`
- Modify: `neo-core/src/neo_vm/stack_item/mod.rs`
- Modify: `neo-core/src/tokens_tracker/trackers/nep_11/mod.rs`
- Modify: `neo-core/src/tokens_tracker/trackers/nep_11/nep11_balance_key.rs`
- Modify: `neo-core/src/tokens_tracker/trackers/nep_11/nep11_transfer_key.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing deletion and caller sentinel**

Added `nep11_token_ordering_does_not_use_local_bytestring_wrapper`, which failed while `byte_string.rs`, its module re-export, and NEP-11 `ByteString::new` callers were still present.

- [x] **Step 2: Replace wrapper use with direct signed little-endian integer decoding**

Removed the local `ByteString` module and re-export. NEP-11 balance/transfer token ordering now uses a shared `token_id_integer` helper built on `BigInt::from_signed_bytes_le`, preserving full-width signed little-endian token ids without routing through a local VM wrapper.

- [x] **Step 3: Verify focused checks**

```powershell
cargo check -p neo-core --features runtime
cargo test -p neo-core --lib neo_vm::stack_item -- --nocapture
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo fmt --check
git diff --check
```

Result: listed checks pass. The stack-item module suite now has 18 tests after deleting the unused primitive/ByteString wrapper modules; the sentinel suite has 109 passing tests. `git diff --check` reports only existing LF-to-CRLF warnings from the Windows checkout.

### Task 2.21: Delegate Local `Script` Bulk Parsing To `neo-vm-rs`

**Files:**
- Modify: `neo-core/src/neo_vm/script.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing parsing-ownership sentinel**

Tightened `instruction_parsing_uses_neo_vm_rs_opcode_operand_metadata_directly` so it failed while local `Script` kept its own `position` loop for full-script parsing and validation.

- [x] **Step 2: Use `parse_script_instructions` for bulk parsing**

Changed `Script::parse_all_instructions` and relaxed validation to call `neo_vm_rs::parse_script_instructions`, then build the local eager lookup map from returned instruction pointers. The local `Script` shell still owns script identity, byte slicing, hash caching, and lazy single-instruction lookup required by the old execution context.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency instruction_parsing_uses_neo_vm_rs_opcode_operand_metadata_directly -- --nocapture
cargo test -p neo-core --lib neo_vm::script -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.22: Add Invocation Counter To The Direct Runtime Syscall Bridge

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing runtime syscall sentinel**

Extended `application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs` so it failed until `System.Runtime.GetInvocationCounter` was allowed through the direct `neo-vm-rs` syscall host.

- [x] **Step 2: Bridge invocation counter from `ApplicationEngine` state**

Added `GetInvocationCounter` to the external runtime syscall table. The bridge reads the current script hash from `ApplicationEngine`, obtains the existing invocation counter, and pushes the result as `neo_vm_rs::StackValue::Integer`.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_get_invocation_counter_returns_one -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.23: Remove The Unused `InteropInterfaceItem` Wrapper Module

**Files:**
- Delete: `neo-core/src/neo_vm/stack_item/interop_interface.rs`
- Modify: `neo-core/src/neo_vm/stack_item/mod.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing deletion sentinel**

Added `unused_interop_interface_stack_item_wrapper_is_removed`, which failed while the unused `InteropInterfaceItem` module was still declared. The sentinel keeps the active `StackItem::InteropInterface` trait surface guarded.

- [x] **Step 2: Delete only the dead wrapper module**

Removed `interop_interface.rs` and its `pub mod interop_interface` declaration. The live `InteropInterface` trait and stack item variant remain in `stack_item.rs` for host/runtime objects.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency unused_interop_interface_stack_item_wrapper_is_removed -- --nocapture
cargo test -p neo-core --lib neo_vm::stack_item -- --nocapture
```

Result: listed checks pass. The stack-item module suite now has 14 tests after deleting unused wrapper-only modules.

### Task 2.24: Use `neo-vm-rs` Parsing For External VM Eligibility Scans

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing parser-ownership sentinel**

Extended `application_engine_executes_syscall_free_scripts_through_neo_vm_rs` so it failed while the external VM eligibility scanner kept a local `position` loop over `Instruction::parse`.

- [x] **Step 2: Delegate eligibility parsing to `neo-vm-rs`**

Changed `script_uses_application_engine_host` to call `neo_vm_rs::parse_script_instructions`. Parse errors still force conservative fallback to the local engine; supported runtime syscalls and `CALLT` filtering keep the same behavior.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_executes_syscall_free_scripts_through_neo_vm_rs -- --nocapture
```

Result: listed check passes, with existing unused/dead-code warnings in `neo-core`.

### Task 2.25: Bridge Script-Hash Runtime Syscalls Through The Direct VM Host

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing script-hash syscall sentinel**

Extended `application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs` so it failed until `System.Runtime.GetExecutingScriptHash`, `GetEntryScriptHash`, and `GetCallingScriptHash` were supported by the external `neo-vm-rs` syscall host.

- [x] **Step 2: Push hashes as direct `StackValue` results**

Added direct handlers that push `StackValue::ByteString(hash.to_bytes())` for available current/entry/calling script hashes and `StackValue::Null` when the calling script hash is absent for the entry context.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_get_executing_and_entry_script_hash_match_entry -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_get_calling_script_hash_returns_null_for_entry_context -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.26: Bridge `System.Runtime.BurnGas` Through The Direct VM Host

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing stack-consuming syscall sentinel**

Extended `application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs` so it failed until `System.Runtime.BurnGas` was allowed through the direct `neo-vm-rs` syscall host and decoded its argument from a `neo_vm_rs::StackValue`.

- [x] **Step 2: Decode one integer argument and delegate fee accounting**

Added `BurnGas` to the external runtime syscall table. The handler pops a value from the `neo-vm-rs` stack, decodes it with `neo_vm_rs::stack_value_as_i64`, and delegates positive-amount validation plus execution-fee charging to `ApplicationEngine::runtime_burn_gas`.

- [x] **Step 3: Verify focused checks**

```powershell
cargo check -p neo-core
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_get_invocation_counter_returns_one -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.27: Bridge `System.Runtime.CheckWitness` Through The Direct VM Host

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing byte-argument syscall sentinel**

Extended `application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs` so it failed until `System.Runtime.CheckWitness` was admitted into the direct `neo-vm-rs` syscall host and decoded its byte argument from a `neo_vm_rs::StackValue`.

- [x] **Step 2: Decode bytes and delegate witness evaluation**

Added `CheckWitness` to the external runtime syscall table. The handler pops a byte-convertible `StackValue`, validates 20-byte script hashes and 33-byte public keys, reuses `ApplicationEngine::pubkey_to_hash` and `check_witness_hash`, and pushes the boolean result back as `StackValue::Boolean`.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_check_witness_returns_false_without_container -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_check_witness_accepts_valid_signer -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_check_witness_returns_false_without_matching_signer -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.28: Bridge `System.Runtime.GetRandom` Through The Direct VM Host

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing random syscall sentinel**

Extended `application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs` so it failed until `System.Runtime.GetRandom` was admitted into the direct `neo-vm-rs` syscall host and produced an integer result on the external stack.

- [x] **Step 2: Mirror random-state mutation and dynamic fees**

Added `GetRandom` to the external runtime syscall table. The handler computes the same 128-bit Murmur value as the local runtime path, mutates either `random_counter` or `nonce_bytes` according to `HfAspidochelone`, charges the dynamic runtime fee, and pushes the positive integer directly as a `neo_vm_rs::StackValue`.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs -- --nocapture
cargo test -p neo-core --test application_engine_runtime_tests runtime_get_random_same_block_matches_csharp -- --nocapture
cargo test -p neo-core --test application_engine_runtime_tests runtime_get_random_different_transactions_diverge -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 2.29: Bridge `System.Runtime.CurrentSigners` Through The Direct VM Host

**Files:**
- Modify: `neo-core/src/smart_contract/application_engine/external_vm.rs`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [x] **Step 1: Add a failing signer syscall sentinel**

Extended `application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs` so it failed until `System.Runtime.CurrentSigners` was admitted into the direct `neo-vm-rs` syscall host. The sentinel covers the no-container null result.

- [x] **Step 2: Project transaction signers directly as `StackValue`**

Added `CurrentSigners` to the external runtime syscall table. The handler pushes `StackValue::Null` when no transaction container is available, otherwise it maps `Transaction::signers()` through each signer's existing `to_stack_value` projection and pushes a `StackValue::Array`.

- [x] **Step 3: Verify focused checks**

```powershell
cargo test -p neo-tests --test no_local_neo_vm_dependency application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_current_signers_returns_transaction_signers -- --nocapture
cargo test -p neo-core --test runtime_syscall_tests runtime_current_signers_returns_null_without_container -- --nocapture
```

Result: listed checks pass, with existing unused/dead-code warnings in `neo-core`.

### Task 3: Move Runtime Boundary Out Of `neo_core::neo_vm`

**Files:**
- Create: `neo-core/src/vm_runtime/mod.rs`
- Modify: `neo-core/src/lib.rs`
- Modify: `neo-core/src/smart_contract/application_engine/mod.rs`
- Modify: `neo-core/src/smart_contract/application_engine/interop_host.rs`
- Modify: `neo-rpc/src/server/session.rs`
- Modify: `neo-rpc/src/server/diagnostic.rs`

- [ ] **Step 1: Add a new `vm_runtime` adapter module outside `neo_vm`**

Move or wrap only the Neo N3 host-specific runtime types that `neo-vm-rs` does not expose yet: gas accounting, host syscall dispatch, debug/in-progress state, and diagnostic windows.

- [ ] **Step 2: Make `ApplicationEngine` call `neo_vm_rs::interpret_with_stack_and_syscalls*`**

Implement `SyscallProvider` for the smart-contract host adapter and map `neo_vm_rs::ExecutionResult` into application-engine state.

Current status: a conservative single-context path now calls `neo_vm_rs::interpret_with_stack_and_syscalls_at*` from `ApplicationEngine` for syscall-free scripts and a small set of read-only no-argument runtime syscalls. Full stack-consuming host syscalls, storage/contract/native dispatch, `CALLT`, dynamic-contract-call, and diagnostic execution still require the remaining runtime adapter work.

- [ ] **Step 3: Replace public result/value surfaces**

Expose `neo_vm_rs::{StackValue, VmState, ExecutionResult}` directly at RPC/logging boundaries. Keep local byte helpers only where persisted Neo N3 storage needs explicit compatibility.

### Task 4: Delete The Local `neo_vm` Tree

**Files:**
- Delete: `neo-core/src/neo_vm/**`
- Modify: all imports still using `crate::neo_vm` or `neo_core::neo_vm`
- Test: `tests/tests/no_local_neo_vm_dependency.rs`

- [ ] **Step 1: Add a red sentinel asserting `neo-core/src/neo_vm` no longer exists**

Add a test that fails while the local tree is present.

- [ ] **Step 2: Delete the tree after imports are migrated**

Remove `pub mod neo_vm` from `neo-core/src/lib.rs` and delete the files.

- [ ] **Step 3: Run focused verification**

```powershell
cargo check -p neo-core
cargo test -p neo-tests --test no_local_neo_vm_dependency -- --nocapture
cargo test -p neo-core application_engine -- --nocapture
```

Expected: focused checks pass without local `neo_vm` imports.
