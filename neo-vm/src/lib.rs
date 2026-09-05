// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

#![warn(missing_docs)]
//! # Neo Virtual Machine (`NeoVM`)
//!
//! An embedded Neo Virtual Machine runtime for `neo-core`.
//!
//! This crate is fully self-contained. The canonical opcode metadata,
//! instruction parsing, and ABI-level value semantics (the shared VM core)
//! live here as the `vm`, `abi`, `semantics`, `interpreter`, `host`, and
//! `runtime` modules, alongside the stateful execution pieces: execution
//! contexts, reference-counted local stack identity, gas hooks, exception
//! handling, and the smart-contract host boundary.
//!
//! ## Architecture
//!
//! The module follows an adapter-oriented architecture. Canonical opcode
//! metadata, instruction parsing, and ABI-level value semantics live in the
//! vendored VM modules; the crate keeps the stateful host surface needed by
//! neo-rs.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    ExecutionEngine                               │
//! │              (Core VM: stack, contexts, execution loop)          │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐    │
//! │  │ Evaluation  │  │   Context    │  │    Reference         │    │
//! │  │   Stack     │  │   Stack      │  │    Counter           │    │
//! │  │             │  │              │  │   (GC support)       │    │
//! │  └─────────────┘  └──────────────┘  └──────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    JumpTable                                     │
//! │      (Stateful dispatch adapters over vendored VM semantics)       │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Layer Position
//!
//! This crate is part of **Layer 1 (Core)** in the neo-rs architecture:
//!
//! ```text
//! Layer 2 (Service): Application layer
//!            │
//!            ▼
//! Layer 1 (Core):   neo_core::neo_vm embedded runtime
//!            │
//!            ▼
//! Layer 0 (Foundation): neo-primitives, neo-io
//! ```
//!
//! ## Key Components
//!
//! | Component | Purpose | Key Type |
//! |-----------|---------|----------|
//! | [`ExecutionEngine`] | Core VM execution loop | `ExecutionEngine` |
//! | [`EvaluationStack`] | Operand stack | `EvaluationStack` |
//! | [`ExecutionContext`] | Script execution context | `ExecutionContext` |
//! | [`JumpTable`] | Stateful opcode dispatch adapters | `JumpTable` |
//! | [`StackItem`] | VM value types | `StackItem` |
//!
//! ## Features
//!
//! - **Shared NeoVM Semantics**: Opcode metadata and ABI-level behavior come from the vendored VM core
//! - **Stack-Based Execution**: Type-safe evaluation stack with reference counting
//! - **Gas Metering**: Precise execution cost tracking
//! - **Exception Handling**: Comprehensive try-catch-finally support
//! - **Reference Counting**: Efficient memory management without GC pauses
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use neo_core::neo_vm::{ExecutionEngine, Script, VmResult};
//! use neo_vm::VmState as VMState;
//! use neo_vm::OpCode;
//!
//! # fn example() -> VmResult<()> {
//! // Create a script that pushes 1 + 2 and returns
//! let script = Script::new(
//!     vec![
//!         OpCode::PUSH1.byte(),
//!         OpCode::PUSH2.byte(),
//!         OpCode::ADD.byte(),
//!         OpCode::RET.byte(),
//!     ],
//!     false,
//! )?;
//!
//! // Create and run the VM
//! let mut engine = ExecutionEngine::new(None);
//! engine.load_script(script, -1, 0)?;
//!
//! let state = engine.execute();
//! assert_eq!(state, VMState::HALT);
//!
//! // Get the result
//! let result = engine.result_stack().peek(0)?;
//! println!("1 + 2 = {}", result.as_int()?);
//! # Ok(())
//! # }
//! ```
//!
//! ## Gas Model
//!
//! The VM implements precise gas metering:
//!
//! | Operation | Base Cost |
//! |-----------|-----------|
//! | Simple opcode | 1 |
//! | PUSH int | 1 |
//! | PUSH data (per byte) | 1 |
//! | CALL | 1024 |
//! | SYSCALL | 256 |
//! | Storage read | 100 |
//! | Storage write | 1000 |
//!
//! ## Error Handling
//!
//! All fallible operations return [`VmResult`]:
//!
//! ```rust,ignore
//! use neo_core::neo_vm::{VmError, VmResult};
//!
//! fn may_fail() -> VmResult<i64> {
//!     // Returns Err(VmError::StackUnderflow) if stack is empty
//!     engine.pop()?.as_int()
//! }
//! ```

// Keep public API documentation warnings visible to maintainers.
#![warn(rustdoc::missing_crate_level_docs)]

// Vendored modules use `alloc::` paths; make the crate nameable crate-wide.
extern crate alloc;

// ============================================================================
// Core VM Modules
// ============================================================================

/// Binary serialization for VM stack items.
pub mod binary_serializer;

/// VM error types and result handling.
pub mod error;

/// Type-safe evaluation stack implementation.
///
/// The [`EvaluationStack`] is the primary operand stack for VM operations.
/// It provides type-safe operations and automatic reference counting.
pub mod evaluation_stack;

/// Script execution context with local variables.
///
/// Each [`ExecutionContext`] represents a call frame with:
/// - Instruction pointer
/// - Evaluation stack
/// - Local variables
/// - Static fields
pub mod execution_context;

/// Core VM execution engine.
///
/// The [`ExecutionEngine`] is the main VM that:
/// - Executes scripts
/// - Manages the context stack
/// - Handles the instruction cycle
/// - Tracks gas consumption
pub mod execution_engine;

/// JSON serialization for VM stack items.
pub mod json_serializer;

/// Interoperable trait for smart contract state round-tripping.
pub mod interoperable;

/// NotifyEventArgs for smart contract event notifications.
pub mod notify_event_args;

/// Interop service registry.
///
/// [`InteropService`] manages native contract methods accessible via SYSCALL.
pub mod interop_service;

/// Stateful opcode dispatch adapters.
///
/// The [`JumpTable`] handles neo-rs execution state and delegates shared opcode
/// metadata and ABI-level behavior to the vendored VM core wherever possible.
pub mod jump_table;

/// Reference counting for garbage collection.
pub mod reference_counter;

/// VM script representation and validation.
pub mod script;

/// Script builder for programmatically constructing VM scripts.
pub mod script_builder;

/// JSON-RPC envelope rendering for VM stack items.
pub mod rpc_json;

/// Slot storage for locals, arguments, and static fields.
pub mod slot;

/// StorageContext for smart contract storage operations.
pub mod storage_context;

/// Native VM stack item engine.
///
/// [`StackItem`] is this crate's own enum with inherent constructors and
/// conversion helpers (e.g. `true_value`, `from_i64`, `as_int`).
pub mod stack_item;

// ============================================================================
// Vendored VM Modules (formerly the external the vendored VM core crate)
//
// These modules were vendored into `neo-vm` so the crate is fully
// self-contained. They expose the canonical opcode metadata, ABI-level stack
// value semantics, interpreter, and shared host/runtime helpers. The public
// surface below mirrors the previous the vendored VM core API so downstream code keeps
// working via `neo_vm::*`.
// ============================================================================

mod abi;
mod host;
mod interpreter;
mod runtime;
pub mod semantics;
mod vm;

// Full public re-export surface (mirrors the vendored crate's `lib.rs`).
pub use abi::{
    BackendKind, COMPACT_TAG_ARRAY, COMPACT_TAG_BIG_INTEGER, COMPACT_TAG_BOOLEAN,
    COMPACT_TAG_BUFFER, COMPACT_TAG_BYTESTRING, COMPACT_TAG_INTEGER, COMPACT_TAG_INTEROP,
    COMPACT_TAG_ITERATOR, COMPACT_TAG_MAP, COMPACT_TAG_NULL, COMPACT_TAG_POINTER,
    COMPACT_TAG_STRUCT, ExecutionResult, NEOVM_STACK_ITEM_TYPE_ANY, NEOVM_STACK_ITEM_TYPE_ARRAY,
    NEOVM_STACK_ITEM_TYPE_BOOLEAN, NEOVM_STACK_ITEM_TYPE_BUFFER, NEOVM_STACK_ITEM_TYPE_BYTESTRING,
    NEOVM_STACK_ITEM_TYPE_INTEGER, NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE,
    NEOVM_STACK_ITEM_TYPE_MAP, NEOVM_STACK_ITEM_TYPE_POINTER, NEOVM_STACK_ITEM_TYPE_STRUCT,
    STACK_VALUE_CODEC_TAG_ARRAY, STACK_VALUE_CODEC_TAG_BIG_INTEGER, STACK_VALUE_CODEC_TAG_BOOLEAN,
    STACK_VALUE_CODEC_TAG_BUFFER, STACK_VALUE_CODEC_TAG_BYTESTRING, STACK_VALUE_CODEC_TAG_INTEGER,
    STACK_VALUE_CODEC_TAG_INTEROP, STACK_VALUE_CODEC_TAG_ITERATOR, STACK_VALUE_CODEC_TAG_MAP,
    STACK_VALUE_CODEC_TAG_NULL, STACK_VALUE_CODEC_TAG_POINTER, STACK_VALUE_CODEC_TAG_STRUCT,
    StackItemType, StackValue, VmState, byte_sequence_bytes, byte_sequence_len,
    concat_splice_values, default_value_for_type_tag, encode_integer,
    new_array_default_value_for_neovm_type_tag, new_array_default_value_for_type_tag,
    normalize_stack_item_type_tag, pop_byte_arg, slice_splice_value, stack_value_as_bool,
    stack_value_as_bytes, stack_value_as_fixed_bytes, stack_value_as_i64, stack_value_as_string,
    stack_value_as_u8, stack_value_as_u32, stack_value_into_items, stack_value_span_bytes,
};
pub use abi::{callback_codec, fast_codec, result_codec};
pub use host::{interop_hash, syscall_arg_count};
pub use interpreter::{
    CALLT_MARKER, CALLT_MARKER_HI, INITIALIZER_COMPLETE_MARKER, SyscallProvider, interpret,
    interpret_with_stack_and_syscalls, interpret_with_stack_and_syscalls_at,
    interpret_with_stack_and_syscalls_at_with_initializer,
    interpret_with_stack_and_syscalls_at_with_initializer_and_result_limit,
    interpret_with_stack_and_syscalls_at_with_result_limit, interpret_with_syscalls,
    last_interpreter_ip, last_result_limit, last_result_stack_len, last_result_stage,
};
pub use runtime::{RuntimeStack, VmContext};
pub use vm::{
    DEFAULT_MAX_INVOCATION_DEPTH, DEFAULT_MAX_STACK_DEPTH, ExceptionHandlingContext,
    ExceptionHandlingState, ExecutionEngineLimits, FromOperand, Instruction, InstructionError,
    InstructionErrorKind, InstructionResult, MAX_ITEM_SIZE, MAX_SCRIPT_SIZE, OpCode,
    ScriptInstruction, Tarjan, ValidatedScript, ValidationResult, VmOrderedDictionary,
};
pub use vm::{
    instruction_jump_target, instruction_try_targets, next_stack_item_id,
    parse_script_instructions, validate_script, validate_strict_script,
};

// ============================================================================
// Public Re-exports from neo-vm (stateful host types)
// ============================================================================

pub use binary_serializer::BinarySerializer;
pub use error::{VmError, VmResult};
pub use evaluation_stack::EvaluationStack;
pub use execution_context::ExecutionContext;
pub use execution_engine::ExecutionEngine;
pub use interop_service::InteropService;
pub use interoperable::Interoperable;
pub use json_serializer::JsonSerializer;
pub use jump_table::JumpTable;
pub use notify_event_args::NotifyEventArgs;
pub use reference_counter::{CompoundParent, ReferenceCounter};
pub use rpc_json::{
    stack_item_rpc_json, stack_item_rpc_json_deferred_size_check, stack_items_rpc_json_per_item,
};
pub use script::Script;
pub use script_builder::ScriptBuilder;
pub use slot::Slot;
pub use stack_item::{StackItem, StackItemExt};
pub use storage_context::StorageContext;

// ============================================================================
// I/O Abstraction
// ============================================================================

/// Production I/O implementation.
pub use neo_io_crate as io;
