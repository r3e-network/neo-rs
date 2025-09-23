//! VM execution state flags.
//!
//! Mirrors `Neo.VM/VMState.cs` from the C# reference implementation.

use bitflags::bitflags;

bitflags! {
    /// Indicates the status of the virtual machine.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct VMState: u8 {
        /// Execution has not started or is currently running.
        const NONE = 0;
        /// Execution completed successfully.
        const HALT = 1 << 0;
        /// Execution terminated because of an unhandled fault.
        const FAULT = 1 << 1;
        /// Execution is paused at a breakpoint.
        const BREAK = 1 << 2;
    }
}

impl VMState {
    /// Returns `true` when the VM has halted successfully.
    pub fn is_halt(self) -> bool {
        self.contains(VMState::HALT)
    }

    /// Returns `true` when the VM faulted.
    pub fn is_fault(self) -> bool {
        self.contains(VMState::FAULT)
    }

    /// Returns `true` when the VM is currently at a breakpoint.
    pub fn is_break(self) -> bool {
        self.contains(VMState::BREAK)
    }
}
