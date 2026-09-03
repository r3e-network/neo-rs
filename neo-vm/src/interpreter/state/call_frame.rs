use super::StackValue;
use alloc::vec::Vec;

pub(in crate::interpreter) struct CallFrame {
    pub(super) return_ip: usize,
    pub(super) initialized: bool,
    #[cfg(target_arch = "riscv32")]
    pub(super) retained_offset: usize,
    #[cfg(target_arch = "riscv32")]
    pub(super) args_len: usize,
    #[cfg(target_arch = "riscv32")]
    pub(super) locals_len: usize,
    #[cfg(not(target_arch = "riscv32"))]
    pub(super) locals: Vec<StackValue>,
    #[cfg(not(target_arch = "riscv32"))]
    pub(super) args: Vec<StackValue>,
}
