use super::{AbiStackValue, SyscallProvider};
use alloc::{format, string::String, vec::Vec};

pub(super) struct NoSyscalls;

impl SyscallProvider for NoSyscalls {
    fn syscall(
        &mut self,
        api: u32,
        _ip: usize,
        _stack: &mut Vec<AbiStackValue>,
    ) -> Result<(), String> {
        Err(format!("unsupported syscall 0x{api:08x}"))
    }
}
