use crate::error::VmError;

use super::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn exec_syscall(&mut self, name: &'static str) -> Result<(), VmError> {
        let args = self.collect_syscall_args()?;
        let dispatcher = self.syscalls.as_mut().ok_or(VmError::UnsupportedSyscall)?;
        let result = dispatcher.invoke(name, &args)?;
        self.stack.push(result);
        Ok(())
    }

    pub(super) fn exec_call_native(
        &mut self,
        contract: &'static str,
        method: &'static str,
        arg_count: usize,
    ) -> Result<(), VmError> {
        if self.stack.len() < arg_count {
            return Err(VmError::StackUnderflow);
        }
        let start = self.stack.len() - arg_count;
        let args = self.stack.split_off(start);
        let result = self
            .invoker
            .invoke(contract, method, &args)
            .map_err(|_| VmError::NativeFailure("invoke failed"))?;
        self.stack.push(result);
        Ok(())
    }
}
