//! Host VM runtime boundary for the smart-contract layer.
//!
//! The local, stateful NeoVM host — the reference-counted execution engine,
//! execution contexts, the host [`StackItem`], jump table, evaluation stack,
//! interop service, and friends — lives under [`crate::neo_vm`]. Smart-contract
//! host-adapter modules that genuinely need those runtime types import them
//! through this single seam instead of reaching into the VM implementation tree
//! directly. This keeps one well-defined boundary between the smart-contract
//! layer and the local VM host.
//!
//! Pure VM value semantics (the `StackValue`, opcode metadata, validators, and
//! shared limits) are NOT re-exported here; those come straight from the
//! `neo_vm_rs` crate so there is a single source of truth for them.

pub use crate::neo_vm::{
    CompoundParent, EvaluationStack, ExecutionContext, ExecutionEngine, InteropService, JumpTable,
    ReferenceCounter, Script, Slot, StackItem, VmError, VmResult,
};

/// Trait implemented by host objects that can be wrapped in an interop stack item.
pub use crate::neo_vm::stack_item::InteropInterface;

/// Access to the host stack-item submodule (compound item types: arrays, maps, structs).
pub use crate::neo_vm::stack_item;
