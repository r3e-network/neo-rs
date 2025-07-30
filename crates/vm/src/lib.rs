//! Neo VM implementation in Rust.
//!
//! This crate provides an implementation of the Neo Virtual Machine (NeoVM)
//! used by the Neo blockchain for executing smart contracts.

// Always import standard library types
extern crate std;

use thiserror::Error;

pub mod application_engine;
pub mod call_flags;
pub mod debugger;
pub mod error;
pub mod evaluation_stack;
pub mod exception_handling;
pub mod execution_context;
pub mod execution_engine;
pub mod instruction;
pub mod interop_service;
pub mod jump_table;
pub mod op_code;
pub mod reference_counter;
pub mod script;
pub mod script_builder;
pub mod stack_item;
pub mod strongly_connected_components;

#[cfg(test)]
pub mod tests;

pub use application_engine::{ApplicationEngine, NotificationEvent, TriggerType};
pub use call_flags::CallFlags;
pub use debugger::{Breakpoint, Debugger};
pub use error::{VmError, VmResult};
pub use evaluation_stack::EvaluationStack;
pub use exception_handling::{ExceptionHandlingContext, ExceptionHandlingState};
pub use execution_context::{ExecutionContext, Slot};
pub use execution_engine::{ExecutionEngine, ExecutionEngineLimits, VMState};
pub use instruction::Instruction;
pub use interop_service::{InteropDescriptor, InteropMethod, InteropService};
pub use jump_table::{InstructionHandler, JumpTable};
pub use op_code::OpCode;
pub use reference_counter::ReferenceCounter;
pub use script::Script;
pub use script_builder::ScriptBuilder;
pub use stack_item::{StackItem, StackItemType};
pub use strongly_connected_components::Tarjan;

#[cfg(test)]
pub use crate::tests::real_io as io;

#[cfg(not(test))]
pub extern crate neo_io;
#[cfg(not(test))]
pub use neo_io as io;
