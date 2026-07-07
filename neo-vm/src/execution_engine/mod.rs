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
//! - `context`: Runtime context records carried through the local workflow.
//! - `control_flow`: VM control-flow opcode handlers.
//! - `core`: Core reader, writer, var-int, and macro helpers for binary IO.
//! - `drop`: VM drop-stack opcode handlers.
//! - `exception`: VM exception opcode handlers.
//! - `execution`: Execution payload records and VM-result domain types.
//! - `interop`: Interop host glue between NeoVM execution and native/runtime
//!   services.
//! - `stack`: VM stack opcode handlers.
//! - `stubs`: placeholder opcode handlers guarded by tests.
//! - `tests`: Module-local tests and regression coverage.

use crate::error::VmError;
use crate::error::VmResult;
use crate::evaluation_stack::EvaluationStack;
use crate::execution_context::ExecutionContext;
use crate::interop_service::{InteropHost, InteropService};
use crate::jump_table::JumpTable;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::stack_item::StackItem;
use neo_primitives::CallFlags;
use neo_vm_rs::Instruction;

use std::convert::TryFrom;

const HASH_SIZE: usize = 32;

/// A wrapper around a raw host pointer that centralizes all unsafe access.
///
/// # Safety
///
/// This type exists to encapsulate the raw `*mut dyn InteropHost` pointer that the
/// execution engine uses to call back into the host environment (e.g. `ApplicationEngine`).
///
/// The following invariants **must** be upheld by the caller who creates a `HostPtr`:
///
/// 1. **Lifetime**: The pointed-to `InteropHost` must outlive the `HostPtr` (and therefore
///    the `ExecutionEngine` that holds it).
/// 2. **Exclusive access**: While the `ExecutionEngine` holds this pointer, no other code
///    should hold a mutable reference to the same `InteropHost`.
/// 3. **Single-thread access**: The `ExecutionEngine` is not shared across threads
///    concurrently. `HostPtr` implements `Send` so the engine can be *moved* between
///    threads, but it is deliberately `!Sync` because the raw pointer must not be
///    dereferenced from multiple threads simultaneously.
/// 4. **Validity**: The pointer must not be dangling. The `Option` wrapper on the engine
///    field handles the null case.
///
/// `HostPtr` is `Copy` because it wraps a raw pointer -- this is required so that it can
/// be extracted from `&self` before passing `&mut self` to the host callback methods
/// (mirroring the original `Option<*mut dyn InteropHost>` which was also `Copy`).
#[derive(Clone, Copy)]
pub(crate) struct HostPtr(
    *mut dyn InteropHost,
    /// Marker to make `HostPtr` `!Send` and `!Sync` by default so that the
    /// manual `Send` impl below is the only path to thread-safety.
    std::marker::PhantomData<*const ()>,
);

// SAFETY: `ExecutionEngine` (the sole owner of `HostPtr`) is never shared
// across threads (`!Sync`). Sending the engine to another thread is safe
// because the pointed-to host moves with it (the host is the parent struct
// that owns the engine). All mutable access is serialized through
// `&mut ExecutionEngine`.
// Rationale: raw host pointers are confined to this VM interop bridge so the
// execution engine can avoid per-callback dynamic ownership wrappers.
#[allow(unsafe_code)]
unsafe impl Send for HostPtr {}

impl HostPtr {
    /// Creates a new `HostPtr` from a raw pointer.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `ptr` is valid for the lifetime of this `HostPtr`
    /// and that no aliasing `&mut` references exist during method calls.
    // Rationale: creating the raw host wrapper is the single unsafe entry point
    // for the VM interop callback fast path.
    #[allow(unsafe_code)]
    pub(crate) unsafe fn new(ptr: *mut dyn InteropHost) -> Self {
        Self(ptr, std::marker::PhantomData)
    }

    /// Returns the underlying raw pointer (for API compatibility with callers that
    /// need to pass it onward).
    #[inline]
    pub(crate) fn as_raw(&self) -> *mut dyn InteropHost {
        self.0
    }

    /// Calls [`InteropHost::on_context_loaded`] on the wrapped host.
    ///
    /// # Safety (internal)
    ///
    /// Safe to call as long as the `HostPtr` invariants documented on the type are upheld.
    // Rationale: callbacks stay allocation-free by using the proven host
    // pointer invariant instead of boxing every VM host transition.
    #[allow(unsafe_code)]
    pub(crate) fn on_context_loaded(
        &self,
        engine: &mut ExecutionEngine,
        context: &ExecutionContext,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract — the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (*self.0).on_context_loaded(engine, context) }
    }

    /// Calls [`InteropHost::on_context_unloaded`] on the wrapped host.
    // Rationale: callbacks stay allocation-free by using the proven host
    // pointer invariant instead of boxing every VM host transition.
    #[allow(unsafe_code)]
    pub(crate) fn on_context_unloaded(
        &self,
        engine: &mut ExecutionEngine,
        context: &ExecutionContext,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract — the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (*self.0).on_context_unloaded(engine, context) }
    }

    /// Calls [`InteropHost::pre_execute_instruction`] on the wrapped host.
    // Rationale: instruction hooks are on the VM hot path and use the confined
    // host pointer invariant to avoid dispatch wrapper allocation.
    #[allow(unsafe_code)]
    pub(crate) fn pre_execute_instruction(
        &self,
        engine: &mut ExecutionEngine,
        instruction: &Instruction,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract — the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (*self.0).pre_execute_instruction(engine, instruction) }
    }

    /// Calls [`InteropHost::post_execute_instruction`] on the wrapped host.
    // Rationale: instruction hooks are on the VM hot path and use the confined
    // host pointer invariant to avoid dispatch wrapper allocation.
    #[allow(unsafe_code)]
    pub(crate) fn post_execute_instruction(
        &self,
        engine: &mut ExecutionEngine,
        instruction: &Instruction,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract — the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (*self.0).post_execute_instruction(engine, instruction) }
    }

    /// Calls [`InteropHost::invoke_syscall`] on the wrapped host.
    // Rationale: syscall dispatch is a VM hot path and uses the confined host
    // pointer invariant to avoid an additional ownership layer.
    #[allow(unsafe_code)]
    pub(crate) fn invoke_syscall(&self, engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract — the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (*self.0).invoke_syscall(engine, hash) }
    }

    /// Calls [`InteropHost::on_callt`] on the wrapped host.
    // Rationale: CALLT dispatch is a VM hot path and uses the confined host
    // pointer invariant to avoid an additional ownership layer.
    #[allow(unsafe_code)]
    pub(crate) fn on_callt(&self, engine: &mut ExecutionEngine, token_id: u16) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract — the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (*self.0).on_callt(engine, token_id) }
    }
}

/// Default gas limit for execution (20 GAS)
/// This is a reasonable default to prevent infinite loops and resource exhaustion
/// Value is in fractional GAS units where 1 GAS = 100_000_000 (10^8)
pub const DEFAULT_GAS_LIMIT: u64 = 20_0000_0000; // 20 GAS

use neo_vm_rs::{ExecutionEngineLimits, VmState as VMState};

/// The execution engine for the Neo VM.
pub struct ExecutionEngine {
    /// The current state of the VM
    pub(crate) state: VMState,

    /// Flag indicating if the engine is in the middle of a jump
    pub is_jumping: bool,

    /// The jump table used to execute instructions
    pub(crate) jump_table: JumpTable,

    /// Restrictions on the VM
    pub(crate) limits: ExecutionEngineLimits,

    /// Used for reference counting of objects in the VM
    pub(crate) reference_counter: ReferenceCounter,

    /// Optional interop service used for handling syscalls
    pub(crate) interop_service: Option<InteropService>,

    /// Host responsible for advanced syscall execution (`ApplicationEngine`).
    ///
    /// All unsafe pointer access is encapsulated inside [`HostPtr`], which provides
    /// safe method wrappers for every `InteropHost` callback. See the `HostPtr` type
    /// documentation for the safety invariants that callers must uphold.
    pub(crate) interop_host: Option<HostPtr>,

    /// Effective call flags for the current execution context
    pub(crate) call_flags: CallFlags,

    /// The invocation stack of the VM
    pub(crate) invocation_stack: Vec<ExecutionContext>,

    /// The stack to store the return values
    pub(crate) result_stack: EvaluationStack,

    /// The VM object representing the uncaught exception
    pub(crate) uncaught_exception: Option<StackItem>,

    /// Number of instructions executed during this execution session.
    pub instructions_executed: u64,

    /// Total gas consumed during execution (in fractional GAS units, 1 GAS = 10^8)
    pub(crate) gas_consumed: u64,

    /// Maximum gas allowed for this execution (in fractional GAS units)
    pub(crate) gas_limit: u64,
}

mod context;
mod control_flow;
mod core;
mod drop;
mod exception;
mod execution;
mod interop;
mod stack;
mod stubs;

#[cfg(test)]
#[path = "../tests/execution_engine/core.rs"]
mod tests;
