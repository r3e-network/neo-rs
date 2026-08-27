use super::executor::interpret_with_stack_and_syscalls_at_internal;
use crate::{ExecutionResult, StackValue as AbiStackValue};
use alloc::{string::String, vec::Vec};

mod no_syscalls;

use no_syscalls::NoSyscalls;

pub fn interpret(script: &[u8]) -> Result<ExecutionResult, String> {
    let mut host = NoSyscalls;
    interpret_with_stack_and_syscalls_at(script, Vec::new(), 0, &mut host)
}

pub trait SyscallProvider {
    fn on_instruction(&mut self, _opcode: u8) -> Result<(), String> {
        Ok(())
    }

    fn syscall(
        &mut self,
        api: u32,
        ip: usize,
        stack: &mut Vec<AbiStackValue>,
    ) -> Result<(), String>;

    fn initializer_complete(&mut self, _ip: usize) -> Result<(), String> {
        Ok(())
    }

    /// Handle CALLT opcode. The default encodes the token with a distinctive
    /// marker so the host can distinguish CALLT from regular SYSCALL.
    fn callt(
        &mut self,
        token: u16,
        ip: usize,
        stack: &mut Vec<AbiStackValue>,
    ) -> Result<(), String> {
        // Encode: CALLT_MARKER_HI in upper 16 bits + token in low 16 bits.
        // Host checks (api >> 16) == CALLT_MARKER_HI to detect CALLT calls.
        self.syscall(CALLT_MARKER | token as u32, ip, stack)
    }
}

/// Marker for CALLT tokens sent through the syscall channel.
/// Upper 16 bits = 0x4354 ("CT"), lower 16 bits = token_id.
/// This pattern is checked via (api >> 16) == 0x4354.
pub const CALLT_MARKER: u32 = 0x4354_0000;
pub const CALLT_MARKER_HI: u16 = 0x4354;
pub const INITIALIZER_COMPLETE_MARKER: u32 = 0x494e_4954;

pub fn interpret_with_syscalls<H: SyscallProvider>(
    script: &[u8],
    host: &mut H,
) -> Result<ExecutionResult, String> {
    interpret_with_stack_and_syscalls_at(script, Vec::new(), 0, host)
}

pub fn interpret_with_stack_and_syscalls<H: SyscallProvider>(
    script: &[u8],
    initial_stack: Vec<AbiStackValue>,
    host: &mut H,
) -> Result<ExecutionResult, String> {
    interpret_with_stack_and_syscalls_at(script, initial_stack, 0, host)
}

pub fn interpret_with_stack_and_syscalls_at<H: SyscallProvider>(
    script: &[u8],
    initial_stack: Vec<AbiStackValue>,
    initial_ip: usize,
    host: &mut H,
) -> Result<ExecutionResult, String> {
    interpret_with_stack_and_syscalls_at_internal(
        script,
        initial_stack,
        initial_ip,
        None,
        None,
        host,
    )
}

pub fn interpret_with_stack_and_syscalls_at_with_result_limit<H: SyscallProvider>(
    script: &[u8],
    initial_stack: Vec<AbiStackValue>,
    initial_ip: usize,
    result_stack_limit: usize,
    host: &mut H,
) -> Result<ExecutionResult, String> {
    interpret_with_stack_and_syscalls_at_internal(
        script,
        initial_stack,
        initial_ip,
        None,
        Some(result_stack_limit),
        host,
    )
}

pub fn interpret_with_stack_and_syscalls_at_with_initializer<H: SyscallProvider>(
    script: &[u8],
    initial_stack: Vec<AbiStackValue>,
    initial_ip: usize,
    initializer_ip: usize,
    host: &mut H,
) -> Result<ExecutionResult, String> {
    interpret_with_stack_and_syscalls_at_internal(
        script,
        initial_stack,
        initial_ip,
        Some(initializer_ip),
        None,
        host,
    )
}

pub fn interpret_with_stack_and_syscalls_at_with_initializer_and_result_limit<
    H: SyscallProvider,
>(
    script: &[u8],
    initial_stack: Vec<AbiStackValue>,
    initial_ip: usize,
    initializer_ip: usize,
    result_stack_limit: usize,
    host: &mut H,
) -> Result<ExecutionResult, String> {
    interpret_with_stack_and_syscalls_at_internal(
        script,
        initial_stack,
        initial_ip,
        Some(initializer_ip),
        Some(result_stack_limit),
        host,
    )
}

#[cfg(test)]
mod try_catch_tests {
    use super::*;
    use crate::{StackValue, VmState};
    use alloc::vec;

    mod error_syscall;
    use error_syscall::ErrorSyscall;

    #[test]
    fn test_try_catch_syscall_exception() {
        let script: Vec<u8> = vec![
            0x3b, 0x0a, 0x00, // TRY catch_offset=10, finally_offset=0
            0x41, 0xde, 0xad, 0xde, 0xad, // SYSCALL
            0x3d, 0x05, // ENDTRY offset=5
            0x11, // PUSH1
            0x3d, 0x02, // ENDTRY offset=2
            0x12, // PUSH2
        ];
        let mut provider = ErrorSyscall;
        let result =
            interpret_with_stack_and_syscalls_at(&script, Vec::new(), 0, &mut provider).unwrap();
        assert_eq!(result.state, VmState::Halt);
        assert_eq!(result.stack.len(), 3);
    }
}
