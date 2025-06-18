//! OpCode module for the Neo Virtual Machine.
//!
//! This module defines all the instructions supported by the Neo Virtual Machine.
//! The OpCodes are organized into logical categories for better maintainability.

pub mod categories;
mod op_code;
mod operand_size;

// Use the corrected OpCode implementation
pub use op_code::OpCode;
pub use operand_size::OperandSize;

// Re-export category types for convenience
pub use categories::{
    ConstantOpCode,
    FlowControlOpCode,
    StackOpCode,
    ArithmeticOpCode,
    OpCodeCategory,
};