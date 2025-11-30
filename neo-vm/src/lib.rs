//! # Neo Virtual Machine (NeoVM)
//!
//! A complete implementation of the Neo Virtual Machine in Rust.
//!
//! The Neo Virtual Machine (NeoVM) is a stack-based virtual machine that executes
//! smart contracts on the Neo blockchain. This crate provides a fully compatible
//! implementation of the NeoVM specification with advanced features for debugging,
//! interoperability, and performance monitoring.
//!
//! ## Features
//!
//! - **Complete OpCode Support**: All Neo VM opcodes with precise semantics
//! - **Stack-Based Execution**: Evaluation stack with type-safe operations
//! - **Interop Services**: Native contract integration and system calls
//! - **Exception Handling**: Comprehensive error handling and recovery
//! - **Debugging Support**: Breakpoints, step execution, and state inspection
//! - **Script Building**: Programmatic smart contract bytecode generation
//! - **Reference Counting**: Memory management for complex data structures
//!
//! ## Architecture
//!
//! The VM is organized into several core components:
//!
//! - **ExecutionEngine**: Main VM execution loop and state management
//! - **ApplicationEngine**: High-level engine with interop service integration
//! - **EvaluationStack**: Type-safe stack for VM operations
//! - **ExecutionContext**: Script execution context and local variables
//! - **JumpTable**: OpCode implementation and instruction dispatch
//! - **StackItem**: Polymorphic data types for VM values
//! - **ScriptBuilder**: Utility for constructing VM scripts
//!
//! ## Example
//!
//! ```rust,no_run
//! use neo_vm::{op_code::OpCode, ExecutionEngine, Script, VMState};
//!
//! # fn example() -> neo_vm::VmResult<()> {
//! // Create a simple script that pushes numbers and adds them
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
//! // Create and configure the VM engine
//! let mut engine = ExecutionEngine::new(None);
//! engine.load_script(script, -1, 0)?;
//!
//! // Execute the script
//! let state = engine.execute();
//! assert_eq!(state, VMState::HALT);
//!
//! // Get the result from the stack
//! if let Ok(result_item) = engine.result_stack().peek(0) {
//!     println!("Result: {}", result_item.as_int().unwrap());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Interop Services
//!
//! The VM supports interop services for accessing blockchain state:
//!
//! ```rust,no_run
//! use neo_vm::{
//!     call_flags::CallFlags,
//!     interop_service::VmInteropDescriptor,
//!     ExecutionEngine,
//! };
//!
//! # fn example() -> neo_vm::VmResult<()> {
//! let mut engine = ExecutionEngine::new(None);
//!
//! // Register custom interop service
//! if let Some(service) = engine.interop_service_mut() {
//!     service.register(VmInteropDescriptor {
//!         name: "MyService.Method".to_string(),
//!         handler: None,
//!         price: 0,
//!         required_call_flags: CallFlags::NONE,
//!     })?;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Debugging
//!
//! The VM includes comprehensive debugging features:
//!
//! ```rust,no_run
//! use neo_vm::{op_code::OpCode, Debugger, ExecutionEngine, Script};
//!
//! # fn example() -> neo_vm::VmResult<()> {
//! let mut engine = ExecutionEngine::new(None);
//! let script = Script::new(vec![OpCode::RET as u8], false)?;
//! engine.load_script(script, -1, 0)?;
//!
//! let mut debugger = Debugger::new(engine);
//!
//! // Execute with debugging
//! let _state = debugger.execute();
//! # Ok(())
//! # }
//! ```

//#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

// Always import standard library types
extern crate std;

// Core VM modules
/// High level application engine with syscall integration
pub mod application_engine;
/// Exception emitted when a script is invalid during loading
pub mod bad_script_exception;
/// Interop service registry and native calls
pub mod call_flags;
/// Base class for VM exceptions that can be caught by smart contracts
pub mod catchable_exception;
/// Collection helpers used by VM stack types
pub mod collections;
/// Debugging support with breakpoints and step execution
pub mod debugger;
/// VM error types and result handling
pub mod error;
/// Type-safe evaluation stack implementation
pub mod evaluation_stack;
/// Shims for the C# source layout exposing the exception handling context types
pub mod exception_handling_context;
/// Shims for the C# exception handling state types
pub mod exception_handling_state;
/// Script execution context and local variables
pub mod execution_context;
/// Low-level VM execution engine
pub mod execution_engine;
/// Configurable limits governing VM execution
pub mod execution_engine_limits;
/// Reference counter interface shared across VM components
pub mod i_reference_counter;
/// VM instruction representation
pub mod instruction;
pub mod interop_service;

/// OpCode implementation and instruction dispatch
pub mod jump_table;
/// VM opcode definitions and utilities
pub mod op_code;
/// Memory management for complex data structures
pub mod reference_counter;
/// VM script representation and validation
pub mod script;
/// Utility for constructing VM bytecode
pub mod script_builder;
/// Slot storage for locals/arguments/static fields
pub mod slot;
/// Polymorphic data types for VM values
pub mod stack_item;
/// Graph algorithms for garbage collection
pub mod strongly_connected_components;
/// Virtual machine lifecycle states (HALT/FAULT/BREAK)
pub mod vm_state;
/// Exception raised when execution terminates without being caught
pub mod vm_unhandled_exception;

/// Test utilities and compatibility tests
#[cfg(test)]
#[allow(dead_code)]
pub mod tests;

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

// I/O abstraction layer
/// Test I/O implementation for unit tests
#[cfg(test)]
#[allow(dead_code)]
pub use crate::tests::real_io as io;

/// Production I/O implementation
#[cfg(not(test))]
pub extern crate neo_io;
/// Re-export of neo_io for production use
#[cfg(not(test))]
pub use neo_io as io;
