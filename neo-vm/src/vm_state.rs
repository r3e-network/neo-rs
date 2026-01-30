//! VM state - Execution status of the Neo Virtual Machine.
//!
//! This module provides the `VMState` enum which tracks the execution status
//! of the Neo VM, matching the C# Neo.VM.VMState implementation.
//!
//! ## States
//!
//! | State | Description |
//! |-------|-------------|
//! | `NONE` | Execution has not started or is in progress |
//! | `HALT` | Execution completed successfully |
//! | `FAULT` | Execution failed with an uncaught exception |
//! | `BREAK` | Execution paused at a breakpoint |
//!
//! ## State Combinations
//!
//! States can be combined using bitwise operations:
//! - `HALT | FAULT` - Both halt and fault conditions
//! - `HALT | BREAK` - Halted at a breakpoint
//!
//! ## Example
//!
//! ```rust
//! use neo_vm::VMState;
//!
//! // Check execution result
//! let state = VMState::HALT;
//! assert!(state.is_halt());
//! assert!(!state.is_fault());
//!
//! // Check for any completion state
//! let completed = state.contains(VMState::HALT);
//! ```

/// namespace Neo.VM -> public enum `VMState` : byte
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
    /// Checks if this state contains the specified flag.
    #[inline]
    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        (self as u8 & flag as u8) != 0
    }

    /// Returns true if the state is NONE (execution in progress or not started).
    #[inline]
    #[must_use]
    pub fn is_none(self) -> bool {
        self == Self::NONE
    }

    /// Returns true if the state contains HALT (execution completed successfully).
    #[inline]
    #[must_use]
    pub fn is_halt(self) -> bool {
        self.contains(Self::HALT)
    }

    /// Returns true if the state contains FAULT (execution ended with exception).
    #[inline]
    #[must_use]
    pub fn is_fault(self) -> bool {
        self.contains(Self::FAULT)
    }

    /// Returns true if the state contains BREAK (breakpoint hit).
    #[inline]
    #[must_use]
    pub fn is_break(self) -> bool {
        self.contains(Self::BREAK)
    }
}
