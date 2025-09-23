//! Control operations module for the Neo Virtual Machine.
//!
//! This module contains all control flow operations, syscalls, and interop services
//! organized into logical submodules for better maintainability.

pub mod control_ops;
pub mod exception_handling;
pub mod interop_services;
pub mod oracle;
pub mod storage;
pub mod syscall;
pub mod types;
pub mod witness;

use crate::{jump_table::JumpTable, op_code::OpCode};

/// Registers all control operation handlers with the jump table.
pub fn register_handlers(jump_table: &mut JumpTable) {
    // Register the baseline control flow handlers provided by `control_ops`.
    super::control_ops::register_handlers(jump_table);

    // Override the exception-handling opcodes with the parity-checked implementations.
    jump_table.register(OpCode::TRY, exception_handling::try_op);
    jump_table.register(OpCode::TryL, exception_handling::try_l);
    jump_table.register(OpCode::THROW, exception_handling::throw);
    jump_table.register(OpCode::ABORT, exception_handling::abort);
    jump_table.register(OpCode::ASSERT, exception_handling::assert);
    jump_table.register(OpCode::ENDTRY, exception_handling::endtry);
    jump_table.register(OpCode::EndtryL, exception_handling::endtry_l);
    jump_table.register(OpCode::ENDFINALLY, exception_handling::endfinally);

    // Syscalls stay delegated to the dedicated module.
    jump_table.register(OpCode::SYSCALL, syscall::syscall);
}

pub use oracle::{get_oracle_request_signers, get_oracle_response_attribute};
pub use storage::{calculate_storage_fee, construct_storage_key};
pub use syscall::syscall;
pub use types::{Block, ExceptionHandler, Transaction};
pub use witness::check_witness_internal;
