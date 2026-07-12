//! # neo-vm::execution_engine::runtime
//!
//! Execution-loop, context-stack, and evaluation-stack mechanics.
//!
//! ## Boundary
//!
//! This module owns mutable engine progression. Opcode semantics, host interop,
//! and exception policy remain in sibling execution-engine modules.
//!
//! ## Contents
//!
//! - `context`: invocation-context load, unload, and removal.
//! - `execution`: instruction dispatch and the main VM loop.
//! - `stack`: evaluation-stack access and gas accounting.

use super::{
    DEFAULT_GAS_LIMIT, ExecutionContext, ExecutionEngine, Script, StackItem, VMState, VmError,
    VmResult,
};

mod context;
mod execution;
mod stack;
