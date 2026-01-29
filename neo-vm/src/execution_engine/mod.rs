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
    /// # Safety Warning (H-3)
    ///
    /// This field uses a raw pointer (`*mut dyn InteropHost`) instead of a safe reference
    /// or smart pointer. This design choice was made to avoid complex lifetime annotations
    /// that would propagate throughout the codebase.
    ///
    /// ## Invariants that MUST be maintained:
    ///
    /// 1. **Lifetime**: The pointed-to `InteropHost` MUST outlive the `ExecutionEngine`.
    ///    The caller (typically `ApplicationEngine`) is responsible for ensuring this.
    ///
    /// 2. **Exclusive Access**: While the `ExecutionEngine` holds this pointer, no other
    ///    code should hold a mutable reference to the same `InteropHost`.
    ///
    /// 3. **Thread Safety**: The `ExecutionEngine` is not `Send` or `Sync` due to this
    ///    raw pointer. Do not share across threads.
    ///
    /// 4. **Null Safety**: The pointer is wrapped in `Option`, so null checks are handled.
    ///    However, a dangling pointer (pointing to freed memory) would cause UB.
    ///
    /// ## Why not use safer alternatives?
    ///
    /// - `&'a mut dyn InteropHost`: Would require lifetime parameter on `ExecutionEngine`,
    ///   propagating to all users and making the API significantly more complex.
    /// - `Arc<Mutex<dyn InteropHost>>`: Would add runtime overhead and potential deadlocks.
    /// - `Box<dyn InteropHost>`: Would transfer ownership, but the host needs to outlive
    ///   multiple engine invocations.
    ///
    /// ## Mitigation
    ///
    /// All unsafe dereferences are localized to a few methods with SAFETY comments.
    /// The `ApplicationEngine` in neo-contract manages the lifetime correctly.
    pub(crate) interop_host: Option<*mut dyn InteropHost>,

    /// Effective call flags for the current execution context
    pub(crate) call_flags: CallFlags,

    /// The invocation stack of the VM
    pub(crate) invocation_stack: Vec<ExecutionContext>,

    /// The stack to store the return values
    pub(crate) result_stack: EvaluationStack,

    /// The VM object representing the uncaught exception
    pub(crate) uncaught_exception: Option<StackItem>,
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
