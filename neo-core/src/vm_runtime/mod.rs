//! Host-specific VM runtime boundary.
//!
//! This module is the temporary public boundary for the stateful runtime pieces
//! that still live in `neo_core::neo_vm` while shared opcode and value semantics
//! move to `neo-vm-rs`.

pub mod rpc_json;

pub use crate::neo_vm::error::{VmError, VmResult};
pub use crate::neo_vm::evaluation_stack::EvaluationStack;
pub use crate::neo_vm::execution_context::ExecutionContext;
pub use crate::neo_vm::execution_engine::ExecutionEngine;
pub use crate::neo_vm::interop_service::{InteropHost, InteropService};
pub use crate::neo_vm::jump_table::JumpTable;
pub use crate::neo_vm::reference_counter::{CompoundParent, ReferenceCounter};
pub use crate::neo_vm::script::Script;
pub use crate::neo_vm::slot::Slot;
pub use crate::neo_vm::stack_item::{
    Array, InteropInterface, Map, Pointer, StackItem, Struct, VmInteger,
};
