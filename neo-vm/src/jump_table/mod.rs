//! # neo-vm::jump_table
//!
//! Opcode dispatch tables and instruction implementations.
//!
//! ## Boundary
//!
//! This module belongs to `neo-vm`. This VM crate owns deterministic script
//! execution and must not own ledger persistence, network transport, or node
//! composition.
//!
//! ## Contents
//!
//! - `compound`: compound opcode handlers.
//! - `control`: control-flow opcode handlers.
//! - `operations`: bitwise, numeric, splice, and type opcode handlers.
//! - `push`: push opcode handlers.
//! - `shared`: shared handler helpers for C# stack coercion and context guards.
//! - `slot`: VM slot records and helpers.
//! - `stack`: VM stack opcode handlers.
//! - `table`: fixed handler storage and hot opcode dispatch accessors.
//! - `variants`: default and hardfork-specific jump-table construction.
//! - `tests`: Module-local tests and regression coverage.

pub mod compound; // Matches JumpTable.Compound.cs
pub mod control; // Matches JumpTable.Control.cs
pub mod operations;
pub mod push; // Matches JumpTable.Push.cs
mod shared;
pub mod slot; // Matches JumpTable.Slot.cs
pub mod stack; // Matches JumpTable.Stack.cs
mod table;
mod variants;

#[cfg(test)]
use crate::error::VmResult;
#[cfg(test)]
use crate::execution_engine::ExecutionEngine;
#[cfg(test)]
use crate::stack_item::StackItem;
#[cfg(test)]
use neo_vm_rs::{Instruction, OpCode};
#[cfg(test)]
use num_bigint::BigInt;

pub(crate) use shared::{
    get_integer, numeric_operand, push_stack_value, require_context, semantics_error,
};
pub use table::{InstructionHandler, JumpTable};

macro_rules! register_jump_handlers {
    ($jump_table:expr_2021; $($opcode:expr_2021 => $handler:expr_2021),+ $(,)?) => {
        $(
            $jump_table.register($opcode, $handler);
        )+
    };
}

pub(crate) use register_jump_handlers;

#[cfg(test)]
#[path = "../tests/jump_table/mod.rs"]
mod tests;
