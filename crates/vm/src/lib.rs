//! Neo VM implementation in Rust.
//!
//! This crate provides an implementation of the Neo Virtual Machine (NeoVM)
//! used by the Neo blockchain for executing smart contracts.

// Note: We always use std for now as the VM requires std library features
// #![cfg_attr(not(feature = "std"), no_std)]

// Always import standard library types
extern crate std;
use std::prelude::v1::*;

use thiserror::Error;

pub mod instruction;
pub mod op_code;
pub mod script;
pub mod evaluation_stack;
pub mod execution_context;
pub mod execution_engine;
pub mod jump_table;
pub mod reference_counter;
pub mod stack_item;
pub mod interop_service;
pub mod script_builder;
pub mod application_engine;
pub mod exception_handling;
pub mod strongly_connected_components;
pub mod debugger;
pub mod call_flags;

// For internal testing, we use mock modules
#[cfg(test)]
pub mod tests;

pub use instruction::Instruction;
pub use op_code::OpCode;
pub use script::Script;
pub use evaluation_stack::EvaluationStack;
pub use execution_context::{ExecutionContext, Slot};
pub use execution_engine::{ExecutionEngine, ExecutionEngineLimits, VMState};
pub use jump_table::{JumpTable, InstructionHandler};
pub use reference_counter::ReferenceCounter;
pub use stack_item::StackItem;
pub use interop_service::{InteropService, InteropMethod, InteropDescriptor};
pub use script_builder::ScriptBuilder;
pub use application_engine::{ApplicationEngine, TriggerType, NotificationEvent};
pub use exception_handling::{ExceptionHandlingContext, ExceptionHandlingState};
pub use strongly_connected_components::Tarjan;
pub use debugger::{Debugger, Breakpoint};
pub use call_flags::CallFlags;

// We re-export the io module for internal testing
#[cfg(test)]
pub use crate::tests::mock_io as io;

// We use the actual neo-io crate in non-test builds
#[cfg(not(test))]
pub extern crate neo_io;
#[cfg(not(test))]
pub use neo_io as io;

/// The result type for the VM.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during VM execution.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum Error {
    /// An error occurred while parsing a script or instruction.
    #[error("Parse error: {0}")]
    ParseError(String),

    /// The instruction is invalid.
    #[error("Invalid instruction: {0}")]
    InvalidInstruction(String),

    /// The opcode is invalid.
    #[error("Invalid opcode: {0}")]
    InvalidOpCode(u8),

    /// The operation is not supported.
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    /// The operation is invalid.
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// An invalid operand was provided.
    #[error("Invalid operand: {0}")]
    InvalidOperand(String),

    /// An invalid script was provided.
    #[error("Invalid script: {0}")]
    InvalidScript(String),

    /// The stack underflow.
    #[error("Stack underflow")]
    StackUnderflow,

    /// The operation would overflow.
    #[error("Overflow")]
    Overflow,

    /// The operation would underflow.
    #[error("Underflow")]
    Underflow,

    /// Division by zero.
    #[error("Division by zero")]
    DivisionByZero,

    /// Insufficient stack items for operation (should result in BREAK, not FAULT).
    #[error("Insufficient stack items")]
    InsufficientStackItems,

    /// The type is invalid.
    #[error("Invalid type: {0}")]
    InvalidType(String),

    /// The execution has been halted.
    #[error("Execution halted: {0}")]
    ExecutionHalted(String),

    /// The VM is in a fault state.
    #[error("VM fault: {0}")]
    VMFault(String),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    IOError(String),

    /// A neo-io error occurred.
    #[error("Neo-IO error: {0}")]
    NeoIOError(String),

    /// A mock io error occurred (for testing).
    #[cfg(test)]
    #[error("Mock IO error: {0}")]
    MockIOError(String),

    /// An unknown error occurred.
    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IOError(err.to_string())
    }
}

impl From<neo_io::Error> for Error {
    fn from(err: neo_io::Error) -> Self {
        Error::NeoIOError(err.to_string())
    }
}

#[cfg(test)]
impl From<crate::tests::mock_io::Error> for Error {
    fn from(err: crate::tests::mock_io::Error) -> Self {
        Error::MockIOError(err.to_string())
    }
}
