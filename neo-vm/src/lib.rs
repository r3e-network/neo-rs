// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{rc::Rc, vec::Vec};

use neo_core::types::VmState;
use neo_base::errors;

use tables::*;
pub use {builder::*, context::*, decode::*, execution::*, interop::*};
pub use {opcode::*, operand::*, program::*, reference::*, stack::*, types::*};


pub mod builder;
pub mod context;
pub mod decode;
pub mod execution;
pub mod program;
pub mod interop;
pub mod reference;
pub mod opcode;
pub mod operand;
pub mod stack;
pub mod tables;
pub mod types;


pub(crate) const MAX_STACK_ITEM_SIZE: usize = 65535 * 2;


pub trait RunPrice {
    fn price(&self) -> u64;
}

#[derive(Debug)]
pub struct VmLimits {
    /// The maximum number of bits that `OpCode::Shl` and `OpCode::Shr` can shift.
    pub max_shift: usize,

    /// The maximum number of items that can be contained in the vm's evaluation stacks and slots.
    pub max_stack_size: usize,

    /// The maximum size of an item in the vm.
    pub max_item_size: usize,

    /// The largest comparable size. If a `ByteString` or `Struct` exceeds this size,
    /// comparison operations on it cannot be performed in the vm.
    pub max_comparable_size: usize,

    /// The maximum number of frames in the invocation stack of the vm.
    pub max_invocation_stack_size: usize,

    /// The maximum nesting depth of `try` blocks.
    pub max_try_nesting_depth: usize,

    /// Allow catching the vm exceptions
    pub catch_exceptions: bool,
}


impl Default for VmLimits {
    fn default() -> Self {
        Self {
            max_shift: 256,
            max_stack_size: 2048,
            max_item_size: MAX_STACK_ITEM_SIZE,
            max_comparable_size: 65536,
            max_invocation_stack_size: 1024,
            max_try_nesting_depth: 16,
            catch_exceptions: true,
        }
    }
}

#[derive(Debug, errors::Error)]
pub enum SyscallError {
    #[error("syscall: no such syscall(0x{0:x})")]
    NoSuchSyscall(u32),
}


pub trait VmEnv {
    fn price_of(&self, opcode: OpCode, operand_len: usize) -> u64;

    // i.e. on interop call
    fn on_syscall(&self, syscall: u32, params: &[StackItem]) -> Result<Vec<StackItem>, SyscallError>;

    // i.e. on CALL_T, call token
    fn on_token_call(&self, token: u32);
}


#[derive(Debug, errors::Error)]
pub enum ExecError {
    #[error("exec: invalid opcode {1:x} at {0:x}")]
    InvalidOpCode(u32, u8),

    #[error("exec: invalid operand {2:x} of {1:?} at {0:x}")]
    InvalidOperand(u32, OpCode, i64),

    #[error("exec: Invalid execution of {1:?} at {0:x} because '{2}'")]
    InvalidExecution(u32, OpCode, &'static str),

    #[error("exec: invalid cast to type {1:?} on {1:?} at {0:x}")]
    InvalidCast(u32, OpCode, StackItemType),

    #[error("exec: invalid jump target if {1:?} at {0} to {2}")]
    InvalidJumpTarget(u32, OpCode, u32),

    #[error("exec: index {2} not in boundary on {1:?} with {3} at {0:x}")]
    IndexOutOfBound(u32, OpCode, usize, usize),

    #[error("exec: stack {2} not in boundary on {1:?} at {0:x}")]
    StackOutOfBound(u32, OpCode, usize),
}

impl ExecError {
    pub fn as_vm_state(&self) -> VmState {
        VmState::Halt // Halt on all ExecError now
        //  match self { _ => VmState::Halt }
    }
}


// struct Invocation {
//     program: Program,
//     context: ExecContext,
// }

pub struct NeoVm<Env: VmEnv> {
    state: VmState,
    limits: VmLimits,
    gas_limit: u64,
    gas_consumed: u64,
    invocations: Vec<ExecContext>,
    env: Env,
}

impl<Env: VmEnv> NeoVm<Env> {
    pub fn new(gas_limit: u64, env: Env) -> Self {
        let limits = VmLimits::default();
        NeoVm {
            state: VmState::Break,
            limits,
            invocations: Vec::new(),
            env,
            gas_limit,
            gas_consumed: 0,
        }
    }

    pub fn vm_state(&self) -> VmState { self.state }

    pub fn execute(&mut self) -> Result<(), ExecError> {
        if self.state == VmState::Break {
            self.state = VmState::None;
        }

        while self.state != VmState::Halt && self.state != VmState::Fault {
            let Some(cx) = self.current_cx() else {
                self.state = VmState::Halt;
                continue; // TODO: return Err
            };

            let _ = cx.execute()
                .inspect_err(|err| self.state = err.as_vm_state())?;
        }

        Ok(())
    }

    #[inline]
    fn current_cx(&mut self) -> Option<&mut ExecContext> {
        self.invocations.last_mut()
    }
}
