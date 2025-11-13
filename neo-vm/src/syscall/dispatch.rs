use alloc::vec::Vec;

use crate::{error::VmError, runtime::RuntimeHost, value::VmValue};

use super::{contract, runtime, storage};

pub struct SyscallDispatcher<'a> {
    host: &'a mut dyn RuntimeHost,
    handlers: Vec<(&'static str, SyscallHandler)>,
}

type SyscallHandler = fn(&mut dyn RuntimeHost, &[VmValue]) -> Result<VmValue, VmError>;

#[derive(Clone, Copy, Debug)]
pub enum Syscall {
    RuntimeLog,
    RuntimeNotify,
    RuntimePlatform,
    RuntimeTrigger,
    RuntimeInvocationCounter,
    StorageGet,
    StoragePut,
    StorageDelete,
    StorageGetContext,
    RuntimeCheckWitness,
    RuntimeTime,
    RuntimeScriptHash,
    RuntimeScript,
}

impl<'a> SyscallDispatcher<'a> {
    pub fn new(host: &'a mut dyn RuntimeHost) -> Self {
        let mut dispatcher = Self {
            host,
            handlers: Vec::new(),
        };
        runtime::register_runtime(&mut dispatcher);
        storage::register_storage(&mut dispatcher);
        contract::register_contract(&mut dispatcher);
        dispatcher
    }

    pub fn register(&mut self, name: &'static str, handler: SyscallHandler) {
        self.handlers.push((name, handler));
    }

    pub fn invoke(&mut self, name: &str, args: &[VmValue]) -> Result<VmValue, VmError> {
        let handler = self
            .handlers
            .iter()
            .find(|(sys_name, _)| *sys_name == name)
            .map(|(_, handler)| *handler)
            .ok_or(VmError::UnsupportedSyscall)?;
        handler(self.host, args)
    }
}
