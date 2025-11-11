use alloc::vec::Vec;

use crate::{
    error::VmError, instruction::Instruction, runtime::RuntimeHost, syscall::SyscallDispatcher,
    value::VmValue,
};

use super::native::NativeInvoker;

mod arithmetic;
mod calls;
mod control;
mod conversion;
mod data;
mod execute;
mod logic;
mod stack_ops;

pub struct VirtualMachine<'a> {
    pub(super) instructions: &'a [Instruction],
    pub(super) ip: usize,
    pub(super) stack: Vec<VmValue>,
    pub(super) locals: Vec<VmValue>,
    pub(super) invoker: &'a mut dyn NativeInvoker,
    pub(super) syscalls: Option<SyscallDispatcher<'a>>,
}

impl<'a> VirtualMachine<'a> {
    pub fn new(instructions: &'a [Instruction], invoker: &'a mut dyn NativeInvoker) -> Self {
        Self {
            instructions,
            ip: 0,
            stack: Vec::new(),
            locals: Vec::new(),
            invoker,
            syscalls: None,
        }
    }

    pub fn with_context(
        instructions: &'a [Instruction],
        invoker: &'a mut dyn NativeInvoker,
        context: &'a mut dyn RuntimeHost,
    ) -> Self {
        Self {
            instructions,
            ip: 0,
            stack: Vec::new(),
            locals: Vec::new(),
            invoker,
            syscalls: Some(SyscallDispatcher::new(context)),
        }
    }

    pub fn execute(self) -> Result<VmValue, VmError> {
        self.run()
    }
}
