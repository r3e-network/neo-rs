// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

#![warn(missing_docs)]
//! # Neo Virtual Machine (`NeoVM`)
//!
//! An embedded Neo Virtual Machine runtime for `neo-core`.
//!
//! This module contains the remaining stateful execution pieces that are not yet
//! provided by `neo-vm-rs`: execution contexts, reference-counted local stack
//! identity, gas hooks, exception handling, and the smart-contract host boundary.
//! Opcode metadata and ABI-level semantics are imported directly from `neo-vm-rs`
//! wherever the behavior matches.
//!
//! ## Architecture
//!
//! The module follows an adapter-oriented architecture. Canonical opcode
//! metadata, instruction parsing, and ABI-level value semantics live in
//! `neo-vm-rs`; `neo_core::neo_vm` keeps the stateful host surface needed by
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
//! │      (Stateful dispatch adapters over neo-vm-rs semantics)       │
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
//! - **Shared NeoVM Semantics**: Opcode metadata and ABI-level behavior come from `neo-vm-rs`
//! - **Stack-Based Execution**: Type-safe evaluation stack with reference counting
//! - **Gas Metering**: Precise execution cost tracking
//! - **Exception Handling**: Comprehensive try-catch-finally support
//! - **Reference Counting**: Efficient memory management without GC pauses
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use neo_core::neo_vm::{ExecutionEngine, Script, VmResult};
//! use neo_vm_rs::VmState as VMState;
//! use neo_vm_rs::OpCode;
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

// Documentation warnings deferred — tracked for incremental doc coverage
#![allow(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

// ============================================================================
// Core VM Modules
// ============================================================================

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

/// Interop service registry.
///
/// [`InteropService`] manages native contract methods accessible via SYSCALL.
pub mod interop_service;

/// Stateful opcode dispatch adapters.
///
/// The [`JumpTable`] handles neo-rs execution state and delegates shared opcode
/// metadata and ABI-level behavior to `neo-vm-rs` wherever possible.
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

/// Stack item type alias and extension methods.
///
/// `StackItem` is now a type alias for [`neo_vm_rs::StackValue`].
pub mod stack_item;

// ============================================================================
// Public Re-exports from neo-vm-rs
//
// These types are the canonical definitions from the shared neo-vm-rs crate.
// Re-exporting them allows downstream code to access them via `neo_vm::*`
// without depending on neo-vm-rs directly.
// ============================================================================

/// Opcode enum — canonical NeoVM opcodes.
pub use neo_vm_rs::OpCode;
/// Parsed bytecode instruction.
pub use neo_vm_rs::Instruction;
/// Instruction parsing errors.
pub use neo_vm_rs::{InstructionError, InstructionErrorKind, InstructionResult};
/// Execution engine configuration limits.
pub use neo_vm_rs::ExecutionEngineLimits;
/// VM execution state (None/Halt/Fault/Break).
pub use neo_vm_rs::VmState;
/// Exception handling context for try/catch/finally.
pub use neo_vm_rs::{ExceptionHandlingContext, ExceptionHandlingState};
/// Stack item type discriminant.
pub use neo_vm_rs::StackItemType;
/// ABI-level stack value (lightweight, no reference counting).
pub use neo_vm_rs::StackValue;
/// Ordered dictionary for Map stack items.
pub use neo_vm_rs::VmOrderedDictionary;
/// Tarjan's algorithm for cycle detection (GC).
pub use neo_vm_rs::Tarjan;
/// Atomic counter for compound stack item identity.
pub use neo_vm_rs::next_stack_item_id;
/// Syscall hash computation.
pub use neo_vm_rs::interop_hash;
/// Script validation functions.
pub use neo_vm_rs::{validate_script, validate_strict_script};
/// Instruction parsing utilities.
pub use neo_vm_rs::{parse_script_instructions, instruction_jump_target, instruction_try_targets};
/// VM constants.
pub use neo_vm_rs::{DEFAULT_MAX_INVOCATION_DEPTH, DEFAULT_MAX_STACK_DEPTH, MAX_ITEM_SIZE, MAX_SCRIPT_SIZE};

// Re-export semantics modules (pure opcode logic from neo-vm-rs).
pub use neo_vm_rs::semantics;

// ============================================================================
// Public Re-exports from neo-vm (stateful host types)
// ============================================================================

pub use error::{VmError, VmResult};
pub use evaluation_stack::EvaluationStack;
pub use execution_context::ExecutionContext;
pub use execution_engine::ExecutionEngine;
pub use interop_service::InteropService;
pub use jump_table::JumpTable;
pub use reference_counter::{CompoundParent, ReferenceCounter};
pub use rpc_json::{stack_item_rpc_json, stack_item_rpc_json_deferred_size_check, stack_items_rpc_json_per_item};
pub use script::Script;
pub use script_builder::ScriptBuilder;
pub use slot::Slot;
pub use stack_item::{StackItem, StackItemExt};

// ============================================================================
// I/O Abstraction
// ============================================================================

/// Production I/O implementation.
pub use neo_io_crate as io;
