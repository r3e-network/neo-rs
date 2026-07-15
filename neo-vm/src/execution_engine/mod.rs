//! # neo-vm::execution_engine
//!
//! NeoVM execution engine loop and runtime state.
//!
//! ## Boundary
//!
//! This module belongs to `neo-vm`. This VM crate owns deterministic script
//! execution and must not own ledger persistence, network transport, or node
//! composition.
//!
//! ## Contents
//!
//! - `control_flow`: VM control-flow opcode handlers.
//! - `core`: Core reader, writer, var-int, and macro helpers for binary IO.
//! - `drop`: VM drop-stack opcode handlers.
//! - `exception`: VM exception opcode handlers.
//! - `host`: Unsafe host-pointer bridge for allocation-free interop callbacks.
//! - `interop`: Interop host glue between NeoVM execution and native/runtime
//!   services.
//! - `runtime`: execution loop, invocation contexts, stack access, and gas.
//! - `stubs`: placeholder opcode handlers guarded by tests.
//! - `tests`: Module-local tests and regression coverage.

use crate::error::VmError;
use crate::error::VmResult;
use crate::evaluation_stack::EvaluationStack;
use crate::execution_context::ExecutionContext;
use crate::execution_profile::ExecutionProfileCollector;
use crate::interop_service::{InteropHost, InteropService};
use crate::jump_table::JumpTable;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::stack_item::StackItem;
use neo_primitives::CallFlags;

use std::convert::TryFrom;

const HASH_SIZE: usize = 32;

/// Default gas limit for execution (20 GAS)
/// This is a reasonable default to prevent infinite loops and resource exhaustion
/// Value is in fractional GAS units where 1 GAS = 100_000_000 (10^8)
pub const DEFAULT_GAS_LIMIT: u64 = 20_0000_0000; // 20 GAS

use crate::{ExecutionEngineLimits, VmState as VMState};

/// The execution engine for the Neo VM.
pub struct ExecutionEngine<S = ()> {
    /// The current state of the VM
    pub(crate) state: VMState,

    /// Flag indicating if the engine is in the middle of a jump
    pub is_jumping: bool,

    /// The jump table used to execute instructions
    pub(crate) jump_table: JumpTable<S>,

    /// Restrictions on the VM
    pub(crate) limits: ExecutionEngineLimits,

    /// Used for reference counting of objects in the VM
    pub(crate) reference_counter: ReferenceCounter,

    /// Optional interop service used for handling syscalls
    pub(crate) interop_service: Option<InteropService<S>>,

    /// Host responsible for advanced syscall execution (`ApplicationEngine`).
    ///
    /// All unsafe pointer access is encapsulated inside [`HostPtr`], which provides
    /// safe method wrappers for every `InteropHost` callback. See the `HostPtr` type
    /// documentation for the safety invariants that callers must uphold.
    pub(crate) interop_host: Option<HostPtr<S>>,

    /// Effective call flags for the current execution context
    pub(crate) call_flags: CallFlags,

    /// The invocation stack of the VM
    pub(crate) invocation_stack: Vec<ExecutionContext<S>>,

    /// The stack to store the return values
    pub(crate) result_stack: EvaluationStack,

    /// The VM object representing the uncaught exception
    pub(crate) uncaught_exception: Option<StackItem>,

    /// Number of instructions executed during this execution session.
    pub instructions_executed: u64,

    /// Opt-in diagnostic counters; absent on the normal consensus hot path.
    pub(crate) execution_profile: Option<Box<ExecutionProfileCollector>>,

    /// Total gas consumed during execution (in fractional GAS units, 1 GAS = 10^8)
    pub(crate) gas_consumed: u64,

    /// Maximum gas allowed for this execution (in fractional GAS units)
    pub(crate) gas_limit: u64,
}

mod control_flow;
mod core;
mod drop;
mod exception;
mod host;
mod interop;
mod runtime;
mod stubs;

pub(crate) use host::HostPtr;

#[cfg(test)]
#[path = "../tests/execution_engine/core.rs"]
mod tests;
