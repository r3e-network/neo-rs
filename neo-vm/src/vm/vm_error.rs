use std::fmt::Error;
use crate::vm_state::VMState;
use thiserror::Error;
use crate::op_code::OpCode;

/// Represents errors during VM execution.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum VMError {
    #[error("Invocation stack size limit exceeded: {0}")]
    InvocationStackOverflow(String),

    #[error("Try nesting depth limit exceeded: {0}")]
    TryNestingOverflow(String),

    #[error("Stack size limit exceeded: {0}")]
    StackOverflow(String),

    #[error("Item size exceeds limit: {0}")]
    ItemTooLarge(String),

    #[error("Encountered invalid opcode: {0}")]
    InvalidOpcode(String),

    #[error("Tried to divide by zero: {0}")]
    DivisionByZero(String),

    #[error("Invalid jump offset or pointer: {0}")]
    InvalidJump(String),

    #[error("Invalid token encountered: {0}")]
    InvalidToken(String),

    #[error("Invalid parameter for operation: {0}")]
    InvalidParameter(String),

    #[error("Item not found in collection: {0}")]
    ItemNotFound(String),

    #[error("Type mismatch for operation: {0}")]
    InvalidType(String),

    #[error("Custom VM error: {0}")]
    Custom(String),
}

impl VMError {
    pub(crate) fn InvalidOpCode(p0: OpCode) -> Error {
        todo!()
    }
}
