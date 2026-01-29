// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # Neo Virtual Machine (`NeoVM`)
//!
//! A complete, high-performance implementation of the Neo Virtual Machine.
//!
//! The Neo Virtual Machine (`NeoVM`) is a lightweight, stack-based virtual machine
//! designed for executing smart contracts on the Neo blockchain. This implementation
//! provides full compatibility with the Neo N3 VM specification while offering
//! advanced features for debugging, gas metering, and cross-platform deployment.
//!
//! ## Architecture
//!
//! The VM follows a layered architecture:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    ApplicationEngine                             │
//! │         (High-level interface with blockchain integration)       │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
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
//! │            (Opcode implementations and dispatch)                 │
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
//! Layer 1 (Core):   neo-vm ◄── YOU ARE HERE
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
//! | [`ApplicationEngine`] | Blockchain-aware VM | `ApplicationEngine` |
//! | [`EvaluationStack`] | Operand stack | `EvaluationStack` |
//! | [`ExecutionContext`] | Script execution context | `ExecutionContext` |
//! | [`JumpTable`] | Opcode dispatch | `JumpTable` |
//! | [`StackItem`] | VM value types | `StackItem` |
//! | [`ScriptBuilder`] | Bytecode construction | `ScriptBuilder` |
//!
//! ## Features
//!
//! - **Complete Opcode Support**: All Neo VM opcodes with precise semantics matching C# implementation
//! - **Stack-Based Execution**: Type-safe evaluation stack with reference counting
//! - **Gas Metering**: Precise execution cost tracking
//! - **Exception Handling**: Comprehensive try-catch-finally support
//! - **Debugging Support**: Breakpoints, step execution, and state inspection
//! - **Reference Counting**: Efficient memory management without GC pauses
//! - **Script Building**: Programmatic smart contract bytecode generation
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use neo_vm::{op_code::OpCode, ExecutionEngine, Script, VMState};
//!
//! # fn example() -> neo_vm::VmResult<()> {
//! // Create a script that pushes 1 + 2 and returns
//! let script = Script::new(
//!     vec![
//!         OpCode::PUSH1 as u8,
//!         OpCode::PUSH2 as u8,
//!         OpCode::ADD as u8,
//!         OpCode::RET as u8,
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
//! ## Using the `ApplicationEngine`
//!
//! For blockchain-aware contract execution:
//!
//! ```rust,ignore
//! use neo_vm::{ApplicationEngine, TriggerType};
//!
//! # fn example() -> neo_vm::VmResult<()> {
//! // Create application engine with blockchain context
//! let mut engine = ApplicationEngine::new(
//!     TriggerType::Application,
//!     snapshot,
//!     transaction,
//!     settings,
//!     gas,
//! )?;
//!
//! // Execute contract
//! engine.load_script(contract_script, -1, 0)?;
//! let state = engine.execute();
//!
//! // Get notifications
//! for notification in engine.notifications() {
//!     println!("Event: {} - {:?}", notification.event_name, notification.state);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Building Scripts
//!
//! ```rust,ignore
//! use neo_vm::{op_code::OpCode, ScriptBuilder};
//!
//! # fn example() -> neo_vm::VmResult<()> {
//! let mut builder = ScriptBuilder::new();
//!
//! // Build a script programmatically
//! builder.emit_push(42i32)?;
//! builder.emit(OpCode::DUP)?;
//! builder.emit(OpCode::ADD)?;
//! builder.emit(OpCode::RET)?;
//!
//! let script = builder.to_array()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Debugging
//!
//! ```rust,ignore
//! use neo_vm::{Debugger, ExecutionEngine, Script};
//!
//! # fn example() -> neo_vm::VmResult<()> {
//! let mut engine = ExecutionEngine::new(None);
//! engine.load_script(script, -1, 0)?;
//!
//! let mut debugger = Debugger::new(engine);
//!
//! // Set a breakpoint
//! debugger.add_breakpoint(10);
//!
//! // Step execution
//! let state = debugger.step_into();
//!
//! // Inspect state
//! println!("Instruction pointer: {}", debugger.instruction_pointer());
//! println!("Stack depth: {}", debugger.stack_count());
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
//! use neo_vm::{VmError, VmResult};
//!
//! fn may_fail() -> VmResult<i64> {
//!     // Returns Err(VmError::StackUnderflow) if stack is empty
//!     engine.pop()?.as_int()
//! }
//! ```

// Warn on missing documentation
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

extern crate std;

// ============================================================================
// Core VM Modules
// ============================================================================

/// High-level application engine with blockchain integration.
///
/// The [`ApplicationEngine`] wraps [`ExecutionEngine`] and adds:
/// - Interop service registration
/// - Gas limit enforcement
/// - Blockchain state access
/// - Notification events
pub mod application_engine;

/// Exception for invalid scripts during loading.
pub mod bad_script_exception;

/// Call flags for interop service methods.
pub mod call_flags;

/// Base exception type for catchable VM exceptions.
pub mod catchable_exception;

/// Collection types for VM stack items.
pub mod collections;

/// Debugging support with breakpoints and step execution.
///
/// The [`Debugger`] provides:
/// - Breakpoint management
/// - Step into/over/out
/// - Stack inspection
/// - Variable watching
pub mod debugger;

/// VM error types and result handling.
pub mod error;

/// Type-safe evaluation stack implementation.
///
/// The [`EvaluationStack`] is the primary operand stack for VM operations.
/// It provides type-safe operations and automatic reference counting.
pub mod evaluation_stack;

/// Exception handling context for try-catch-finally.
pub mod exception_handling_context;

/// Exception handling state tracking.
pub mod exception_handling_state;

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

/// Configurable execution limits.
///
/// [`ExecutionEngineLimits`] controls:
/// - Max stack size
/// - Max item size
/// - Max invocation stack size
pub mod execution_engine_limits;

/// Reference counter interface.
pub mod i_reference_counter;

/// VM instruction representation.
pub mod instruction;

/// Interop service registry.
///
/// [`InteropService`] manages native contract methods accessible via SYSCALL.
pub mod interop_service;

/// Opcode implementations and instruction dispatch.
///
/// The [`JumpTable`] contains implementations for all VM opcodes.
pub mod jump_table;

/// VM opcode definitions.
pub mod op_code;

/// Reference counting for garbage collection.
pub mod reference_counter;

/// VM script representation and validation.
pub mod script;

/// Utility for constructing VM bytecode.
///
/// [`ScriptBuilder`] provides a fluent API for building scripts.
pub mod script_builder;

/// Slot storage for locals, arguments, and static fields.
pub mod slot;

/// Polymorphic VM value types.
///
/// [`StackItem`] represents all values that can exist on the VM stack:
/// - Primitive types (Integer, Boolean, ByteString)
/// - Complex types (Array, Map, Struct)
/// - Special types (Pointer, InteropInterface)
pub mod stack_item;

/// Tarjan's algorithm for garbage collection.
pub mod strongly_connected_components;

/// VM execution states.
///
/// - `HALT`: Execution completed successfully
/// - `FAULT`: Execution failed
/// - `BREAK`: Hit a breakpoint
/// - `NONE`: Not started
pub mod vm_state;

/// Exception for unhandled VM exceptions.
pub mod vm_unhandled_exception;

// ============================================================================
// Test Utilities
// ============================================================================

#[cfg(test)]
#[allow(dead_code)]
pub mod tests;

// ============================================================================
// Public Re-exports
// ============================================================================

pub use application_engine::{ApplicationEngine, NotificationEvent, TriggerType};
pub use bad_script_exception::BadScriptException;
pub use call_flags::CallFlags;
pub use catchable_exception::CatchableException;
pub use collections::VmOrderedDictionary as OrderedDictionary;
pub use debugger::Debugger;
pub use error::{VmError, VmResult};
pub use evaluation_stack::EvaluationStack;
pub use exception_handling_context::ExceptionHandlingContext;
pub use exception_handling_state::ExceptionHandlingState;
pub use execution_context::ExecutionContext;
pub use execution_engine::ExecutionEngine;
pub use execution_engine_limits::ExecutionEngineLimits;
pub use i_reference_counter::IReferenceCounter;
pub use instruction::Instruction;
pub use interop_service::{InteropService, VmInteropDescriptor as InteropDescriptor};
pub use jump_table::{InstructionHandler, JumpTable};
pub use op_code::OpCode;
pub use reference_counter::{CompoundParent, ReferenceCounter};
pub use script::Script;
pub use script_builder::ScriptBuilder;
pub use slot::Slot;
pub use stack_item::{StackItem, StackItemType};
pub use strongly_connected_components::Tarjan;
pub use vm_state::VMState;
pub use vm_unhandled_exception::VMUnhandledException;

// ============================================================================
// I/O Abstraction
// ============================================================================

/// Test I/O implementation for unit tests.
#[cfg(test)]
#[allow(dead_code)]
pub use crate::tests::real_io as io;

/// Production I/O implementation.
#[cfg(not(test))]
pub extern crate neo_io;

/// Re-export of `neo_io` for production use.
#[cfg(not(test))]
pub use neo_io as io;
