// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # neo-vm
//!
//! NeoVM execution engine, opcode dispatch, stack items, and runtime types.
//!
//! ## Boundary
//!
//! This VM crate owns deterministic script execution and must not own ledger
//! persistence, network transport, or node composition.
//!
//! ## Contents
//!
//! - `types`: Storage-domain types shared by store implementations.
//! - `script_builder`: Helpers for constructing NeoVM scripts
//!   deterministically.
//! - `runtime`: Runtime flags, execution context state, and VM-facing support
//!   types.
//! - `execution_context`: NeoVM execution context frames and instruction-
//!   pointer state.
//! - `execution_engine`: NeoVM execution engine loop and runtime state.
//! - `jump_table`: Opcode dispatch tables and instruction implementations.
//! - `stack_item`: NeoVM stack item representations and conversion helpers.

// ============================================================================
// Core VM Modules
// ============================================================================

/// VM error types and result handling.
mod types;
pub use types::error;

/// Script builder for programmatic VM script construction.
pub mod script_builder;

/// Type-safe evaluation stack implementation.
///
/// The [`EvaluationStack`] is the primary operand stack for VM operations.
/// It provides type-safe operations and automatic reference counting.
mod runtime;
pub use runtime::evaluation_stack;

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

/// Interoperable trait for smart contract state round-tripping.
pub use runtime::interoperable;

/// Interop service registry.
///
/// [`InteropService`] manages native contract methods accessible via SYSCALL.
pub use runtime::interop_service;

/// Stateful opcode dispatch adapters.
///
/// The [`JumpTable`] handles neo-rs execution state and delegates shared opcode
/// metadata and ABI-level behavior to `neo-vm-rs` wherever possible.
pub mod jump_table;

/// Reference counting for garbage collection.
pub use runtime::reference_counter;

/// VM script representation and validation.
pub use types::script;

/// JSON-RPC envelope rendering for VM stack items.
pub use types::rpc_json;

/// Slot storage for locals, arguments, and static fields.
pub use runtime::slot;

/// Stateful, reference-counted host stack item used by the local execution
/// engine. The pure value type lives upstream in the `neo_vm_rs` crate; this
/// host type adds reference counting and interop-interface support.
pub mod stack_item;

// ============================================================================
// Pure VM types come directly from the shared neo-vm-rs crate.
//
// neo_core::neo_vm intentionally does NOT re-export the pure VM symbols defined
// upstream. Callers import those directly from the `neo_vm_rs` crate so there
// is a single, unambiguous source of truth for the shared VM semantics; this
// module only re-exports the local stateful host types below.
// ============================================================================

// ============================================================================
// Public Re-exports from the local VM host (stateful types)
// ============================================================================

pub use error::{VmError, VmResult};
pub use execution_context::ExecutionContext;
pub use execution_engine::ExecutionEngine;
pub use jump_table::JumpTable;
pub use runtime::{
    CompoundId, EvaluationStack, InteropService, Interoperable, InteroperableError,
    ReferenceCounter, Slot,
};
pub use stack_item::{InteropInterface, StackItem};
pub use types::rpc_json::StackItemRpcJson;
pub use types::script::Script;

/// Verification contract (script + parameter list + cached hash).
///
/// Moved here from `neo-execution` so that `neo-wallets` can use `Contract`
/// without depending on `neo-execution`.
pub mod contract;
pub use contract::Contract;

/// Decode a VM stack value as a NeoVM integer.
///
/// This preserves the compatibility surface older workspace crates used from
/// `neo-vm-rs` while keeping the 32-byte integer bound enforced by the local
/// stateful `StackItem` conversion rules.
pub fn stack_value_as_bigint(value: &neo_vm_rs::StackValue) -> Result<num_bigint::BigInt, VmError> {
    match value {
        neo_vm_rs::StackValue::Integer(value) => Ok(num_bigint::BigInt::from(*value)),
        neo_vm_rs::StackValue::BigInteger(bytes) | neo_vm_rs::StackValue::ByteString(bytes) => {
            stack_item::stack_item::decode_integer_bytes(bytes)
        }
        neo_vm_rs::StackValue::Boolean(value) => Ok(num_bigint::BigInt::from(u8::from(*value))),
        _ => Err(VmError::invalid_type_simple(
            "Stack value is not integer-compatible",
        )),
    }
}

// ============================================================================
// I/O Abstraction
// ============================================================================

/// Production I/O implementation.
pub use neo_io as io;
