//! NeoVM execution state.

use serde::{Deserialize, Serialize};

/// NeoVM execution state.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VmState {
    /// Execution has not started or is in progress.
    None = 0,
    /// Execution completed successfully.
    Halt = 1 << 0,
    /// Execution failed with an error.
    Fault = 1 << 1,
    /// Execution paused at a breakpoint.
    Break = 1 << 2,
}

impl VmState {
    /// C# Neo.VM-compatible state constant.
    pub const NONE: Self = Self::None;
    /// C# Neo.VM-compatible state constant.
    pub const HALT: Self = Self::Halt;
    /// C# Neo.VM-compatible state constant.
    pub const FAULT: Self = Self::Fault;
    /// C# Neo.VM-compatible state constant.
    pub const BREAK: Self = Self::Break;

    /// Returns the C# Neo.VM byte tag for this state.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Decodes a persisted C# Neo.VM state byte.
    #[must_use]
    pub const fn from_byte(value: u8) -> Self {
        match value {
            1 => Self::Halt,
            2 => Self::Fault,
            4 => Self::Break,
            _ => Self::None,
        }
    }

    /// Checks whether this state contains the specified flag.
    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        (self.to_byte() & flag.to_byte()) != 0
    }

    /// Returns true for a final execution state.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Halt | Self::Fault)
    }

    /// Returns the RPC name for a final state.
    #[must_use]
    pub const fn final_name(self) -> Option<&'static str> {
        match self {
            Self::Halt => Some("HALT"),
            Self::Fault => Some("FAULT"),
            Self::None | Self::Break => None,
        }
    }

    /// Returns true if execution has not started.
    #[must_use]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    /// Returns true if execution halted successfully.
    #[must_use]
    pub const fn is_halt(self) -> bool {
        self.contains(Self::Halt)
    }

    /// Returns true if execution faulted.
    #[must_use]
    pub const fn is_fault(self) -> bool {
        self.contains(Self::Fault)
    }

    /// Returns true if execution is paused.
    #[must_use]
    pub const fn is_break(self) -> bool {
        self.contains(Self::Break)
    }
}
