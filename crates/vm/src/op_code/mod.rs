//! OpCode module for the Neo Virtual Machine.
//!
//! This module defines all the instructions supported by the Neo Virtual Machine.
//! The OpCodes are organized into logical categories for better maintainability.

#[allow(clippy::module_inception)]
mod op_code;
mod operand_size;

pub use op_code::OpCode;
pub use operand_size::OperandSize;
