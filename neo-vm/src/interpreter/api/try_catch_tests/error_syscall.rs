use super::{StackValue, SyscallProvider};
use alloc::{string::String, string::ToString, vec::Vec};

pub(super) struct ErrorSyscall;

impl SyscallProvider for ErrorSyscall {
    fn on_instruction(&mut self, _opcode: u8) -> Result<(), String> {
        Ok(())
    }

    fn syscall(
        &mut self,
        _api: u32,
        _ip: usize,
        _stack: &mut Vec<StackValue>,
    ) -> Result<(), String> {
        Err("error".to_string())
    }
}
