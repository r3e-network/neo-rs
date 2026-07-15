//! # NeoVM Runtime Types
//!
//! Canonical Neo N3 bytecode, execution-state, and VM metadata types.
//!
//! ## Boundary
//!
//! This module owns VM-level primitives shared by script decoding and
//! execution. Mutable runtime values remain owned by `crate::stack_item`.
//!
//! ## Contents
//!
//! Opcode and instruction metadata, execution limits and state, strict script
//! validation, exception contexts, syscall metadata, and small VM collections.

mod collections;
mod exception_handling;
mod identity;
mod instruction;
mod limits;
mod opcode;
mod script_validation;
mod stack_item_type;
mod state;
mod syscall;

pub use collections::{VmOrderedDictionary, encode_integer};
pub use exception_handling::{ExceptionHandlingContext, ExceptionHandlingState};
pub use identity::next_stack_item_id;
pub use instruction::{
    FromOperand, Instruction, InstructionError, InstructionErrorKind, InstructionResult,
};
pub use limits::{
    DEFAULT_MAX_INVOCATION_DEPTH, DEFAULT_MAX_STACK_DEPTH, ExecutionEngineLimits, MAX_ITEM_SIZE,
    MAX_SCRIPT_SIZE,
};
pub use opcode::OpCode;
pub use script_validation::{
    ScriptInstruction, ValidatedScript, ValidationResult, instruction_jump_target,
    instruction_try_targets, parse_script_instructions, validate_script, validate_strict_script,
};
pub use stack_item_type::{
    NEOVM_STACK_ITEM_TYPE_ANY, NEOVM_STACK_ITEM_TYPE_ARRAY, NEOVM_STACK_ITEM_TYPE_BOOLEAN,
    NEOVM_STACK_ITEM_TYPE_BUFFER, NEOVM_STACK_ITEM_TYPE_BYTESTRING, NEOVM_STACK_ITEM_TYPE_INTEGER,
    NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE, NEOVM_STACK_ITEM_TYPE_MAP,
    NEOVM_STACK_ITEM_TYPE_POINTER, NEOVM_STACK_ITEM_TYPE_STRUCT, StackItemType,
};
pub use state::VmState;
pub use syscall::{interop_hash, syscall_arg_count};
