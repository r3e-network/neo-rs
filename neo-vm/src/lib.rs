// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{rc::Rc, vec::Vec};

use neo_base::errors;

pub use {decode::*, execution::*, execution_context::*, interop::*, script_builder::*};
pub use {evaluation_stack::*, operand::*, program::*, reference::*, stackitem_type::*};
use {slots::*, tables::*};
use crate::vm::{OpCode, VMState};

pub mod script_builder;
pub mod execution_context;
pub mod decode;
pub mod execution;
pub mod interop;
pub mod operand;
pub mod program;
pub mod reference;
pub mod slots;
pub mod evaluation_stack;
pub mod tables;
pub mod stackitem_type;
pub mod vm;
pub mod script;
pub mod exception;
mod call_flags;
pub mod vm_error;

pub const MAX_STACK_SIZE: usize = 2048;
pub const MAX_STACK_ITEM_SIZE: usize = 65535 * 2;
pub const MAX_COMPARABLE_SIZE: usize = 65536;

pub trait RunPrice {
    fn price(&self) -> u64;
}

impl RunPrice for OpCode {
    #[inline]
    fn price(&self) -> u64 { CODE_ATTRS[self.as_u8() as usize].price }
}

#[derive(Debug, errors::Error)]
pub enum SyscallError {
    #[error("syscall: no such syscall(0x{0:x})")]
    NoSuchSyscall(u32),
}

pub trait VmEnv {
    fn price_of(&self, opcode: OpCode, operand_len: usize) -> u64;

    // i.e. on interop call
    fn on_syscall(
        &self,
        syscall: u32,
        params: &[StackItem],
    ) -> Result<Vec<StackItem>, SyscallError>;

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

    #[error("exec: exceed execution limits: {0}")]
    ExceedExecutionLimits(&'static str),
}

impl ExecError {
    pub fn as_vm_state(&self) -> VMState {
        VMState::Halt // Halt on all ExecError now
        //  match self { _ => VMState::Halt }
    }
}

// struct Invocation {
//     program: Program,
//     context: ExecContext,
// }

pub struct NeoVm<Env: VmEnv> {
    state: VMState,
    gas_limit: u64,
    gas_consumed: u64,
    invocations: Vec<ExecutionContext>,
    env: Env,
}

impl<Env: VmEnv> NeoVm<Env> {
    pub fn new(gas_limit: u64, env: Env) -> Self {
        NeoVm {
            state: VMState::Break,
            gas_limit,
            gas_consumed: 0,
            invocations: Vec::new(),
            env,
        }
    }

    pub fn execute(&mut self) -> Result<(), ExecError> {
        if self.state == VMState::Break {
            self.state = VMState::None;
        }

        while self.state != VMState::Halt && self.state != VMState::Fault {
            let Some(cx) = self.current_cx() else {
                self.state = VMState::Halt;
                continue; // TODO: return Err
            };

            let _ = cx.execute().inspect_err(|err| self.state = err.as_vm_state())?;
        }

        Ok(())
    }

    #[inline]
    pub fn vm_state(&self) -> VMState { self.state }

    #[inline]
    fn current_cx(&mut self) -> Option<&mut ExecutionContext> { self.invocations.last_mut() }
}
