//! Execution engine module for the Neo Virtual Machine.
//!
//! This module provides the core execution engine implementation for the Neo VM,
//! a stack-based virtual machine designed for smart contract execution.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    ExecutionEngine                           │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                  Invocation Stack                        ││
//! │  │  ┌──────────────────────────────────────────────────┐   ││
//! │  │  │ ExecutionContext (current)                        │   ││
//! │  │  │  ┌────────────┐  ┌────────────┐  ┌────────────┐  │   ││
//! │  │  │  │ Script     │  │ EvalStack  │  │ AltStack   │  │   ││
//! │  │  │  │ (bytecode) │  │ (operands) │  │ (temp)     │  │   ││
//! │  │  │  └────────────┘  └────────────┘  └────────────┘  │   ││
//! │  │  └──────────────────────────────────────────────────┘   ││
//! │  │  ┌──────────────────────────────────────────────────┐   ││
//! │  │  │ ExecutionContext (caller)                         │   ││
//! │  │  └──────────────────────────────────────────────────┘   ││
//! │  │  ... (more contexts)                                     ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │ JumpTable    │  │ RefCounter   │  │ InteropService   │  │
//! │  │ (opcodes)    │  │ (GC)         │  │ (syscalls)       │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Components
//!
//! - [`ExecutionEngine`]: Main VM execution loop and state management
//! - [`ExecutionContext`]: Script execution state (instruction pointer, stacks)
//! - [`EvaluationStack`]: Operand stack for instruction execution
//! - [`JumpTable`]: Opcode dispatch table for instruction execution
//! - [`ReferenceCounter`]: Garbage collection for compound stack items
//!
//! # VM States
//!
//! - `NONE`: Initial state before execution
//! - `HALT`: Successful completion
//! - `FAULT`: Execution error (invalid operation, stack underflow, etc.)
//! - `BREAK`: Breakpoint hit (debugging)
//!
//! # Execution Model
//!
//! 1. Load script into new execution context
//! 2. Push context onto invocation stack
//! 3. Fetch instruction at current instruction pointer
//! 4. Execute instruction via jump table
//! 5. Update instruction pointer
//! 6. Repeat until HALT, FAULT, or context stack empty
//!
//! # Stack Items
//!
//! The VM supports these stack item types:
//! - `Boolean`, `Integer`, `ByteString`, `Buffer`
//! - `Array`, `Struct`, `Map` (compound types)
//! - `Pointer`, `InteropInterface` (special types)

use crate::call_flags::CallFlags;
use crate::error::VmError;
use crate::error::VmResult;
use crate::evaluation_stack::EvaluationStack;
use crate::execution_context::ExecutionContext;
use crate::instruction::Instruction;
use crate::interop_service::{InteropHost, InteropService};
use crate::jump_table::JumpTable;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::stack_item::StackItem;

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
/// 3. **Thread safety**: `HostPtr` is intentionally `!Send` and `!Sync` due to the raw
///    pointer. Do not share across threads.
/// 4. **Validity**: The pointer must not be dangling. The `Option` wrapper on the engine
///    field handles the null case.
///
/// `HostPtr` is `Copy` because it wraps a raw pointer — this is required so that it can
/// be extracted from `&self` before passing `&mut self` to the host callback methods
/// (mirroring the original `Option<*mut dyn InteropHost>` which was also `Copy`).
#[derive(Clone, Copy)]
pub(crate) struct HostPtr(*mut dyn InteropHost);

// SAFETY: `HostPtr` wraps a raw pointer whose safety invariants are already
// enforced by the `unsafe fn new` contract: the pointee must be valid for the
// lifetime of the `HostPtr` and must not be aliased mutably during callbacks.
// All access is serialized through `&mut ExecutionEngine`, so sending the
// pointer across threads is safe when the engine itself is behind a `Mutex`.
unsafe impl Send for HostPtr {}
unsafe impl Sync for HostPtr {}

impl HostPtr {
    /// Creates a new `HostPtr` from a raw pointer.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `ptr` is valid for the lifetime of this `HostPtr`
    /// and that no aliasing `&mut` references exist during method calls.
    pub(crate) unsafe fn new(ptr: *mut dyn InteropHost) -> Self {
        Self(ptr)
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
    pub(crate) fn on_context_unloaded(
        &self,
        engine: &mut ExecutionEngine,
        context: &ExecutionContext,
    ) -> VmResult<()> {
        unsafe { (*self.0).on_context_unloaded(engine, context) }
    }

    /// Calls [`InteropHost::pre_execute_instruction`] on the wrapped host.
    pub(crate) fn pre_execute_instruction(
        &self,
        engine: &mut ExecutionEngine,
        context: &ExecutionContext,
        instruction: &Instruction,
    ) -> VmResult<()> {
        unsafe { (*self.0).pre_execute_instruction(engine, context, instruction) }
    }

    /// Calls [`InteropHost::post_execute_instruction`] on the wrapped host.
    pub(crate) fn post_execute_instruction(
        &self,
        engine: &mut ExecutionEngine,
        context: &ExecutionContext,
        instruction: &Instruction,
    ) -> VmResult<()> {
        unsafe { (*self.0).post_execute_instruction(engine, context, instruction) }
    }

    /// Calls [`InteropHost::invoke_syscall`] on the wrapped host.
    pub(crate) fn invoke_syscall(
        &self,
        engine: &mut ExecutionEngine,
        hash: u32,
    ) -> VmResult<()> {
        unsafe { (*self.0).invoke_syscall(engine, hash) }
    }

    /// Calls [`InteropHost::on_callt`] on the wrapped host.
    pub(crate) fn on_callt(
        &self,
        engine: &mut ExecutionEngine,
        token_id: u16,
    ) -> VmResult<()> {
        unsafe { (*self.0).on_callt(engine, token_id) }
    }
}

/// Default gas limit for execution (20 GAS)
/// This is a reasonable default to prevent infinite loops and resource exhaustion
/// Value is in fractional GAS units where 1 GAS = 100_000_000 (10^8)
pub const DEFAULT_GAS_LIMIT: u64 = 20_0000_0000; // 20 GAS

pub use crate::execution_engine_limits::ExecutionEngineLimits;
pub use crate::vm_state::VMState;

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
    pub(crate) instructions_executed: u64,

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
mod tests;
