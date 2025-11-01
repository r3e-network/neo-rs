//! VM state implementation.
//!
//! This module provides the VMState functionality exactly matching C# Neo.VM.VMState.

/// namespace Neo.VM -> public enum VMState : byte

/// Indicates the status of the VM.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMState {
    /// Indicates that the execution is in progress or has not yet begun.
    NONE = 0,

    /// Indicates that the execution has been completed successfully.
    HALT = 1 << 0,

    /// Indicates that the execution has ended, and an exception that cannot be caught is thrown.
    FAULT = 1 << 1,

    /// Indicates that a breakpoint is currently being hit.
    BREAK = 1 << 2,
}

impl VMState {
    #[inline]
    pub fn contains(self, flag: VMState) -> bool {
        (self as u8 & flag as u8) != 0
    }

    #[inline]
    pub fn is_none(self) -> bool {
        self == VMState::NONE
    }

    #[inline]
    pub fn is_halt(self) -> bool {
        self.contains(VMState::HALT)
    }

    #[inline]
    pub fn is_fault(self) -> bool {
        self.contains(VMState::FAULT)
    }

    #[inline]
    pub fn is_break(self) -> bool {
        self.contains(VMState::BREAK)
    }
}
