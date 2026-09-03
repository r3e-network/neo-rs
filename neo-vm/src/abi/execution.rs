//! Shared VM execution result types.

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use super::stack_value::StackValue;

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
    #[inline]
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Halt => 1 << 0,
            Self::Fault => 1 << 1,
            Self::Break => 1 << 2,
        }
    }

    /// Decodes a persisted C# Neo.VM state byte.
    #[inline]
    #[must_use]
    pub const fn from_byte(value: u8) -> Self {
        match value {
            value if value == Self::Halt.to_byte() => Self::Halt,
            value if value == Self::Fault.to_byte() => Self::Fault,
            value if value == Self::Break.to_byte() => Self::Break,
            _ => Self::None,
        }
    }

    /// Checks if this state contains the specified flag.
    #[inline]
    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        (self.to_byte() & flag.to_byte()) != 0
    }

    /// Returns true for a final execution state.
    #[inline]
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Halt | Self::Fault)
    }

    /// Returns the RPC/final-result state name for final states.
    #[inline]
    #[must_use]
    pub const fn final_name(self) -> Option<&'static str> {
        match self {
            Self::Halt => Some("HALT"),
            Self::Fault => Some("FAULT"),
            Self::None | Self::Break => None,
        }
    }

    /// Returns true if the state is None.
    #[inline]
    #[must_use]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    /// Returns true if the state contains Halt.
    #[inline]
    #[must_use]
    pub const fn is_halt(self) -> bool {
        self.contains(Self::Halt)
    }

    /// Returns true if the state contains Fault.
    #[inline]
    #[must_use]
    pub const fn is_fault(self) -> bool {
        self.contains(Self::Fault)
    }

    /// Returns true if the state contains Break.
    #[inline]
    #[must_use]
    pub const fn is_break(self) -> bool {
        self.contains(Self::Break)
    }
}

/// Execution backend identifier for result reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendKind {
    /// Direct interpreter execution.
    Interpreter,
}

/// Result of VM script execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Gas consumed in pico units.
    pub fee_consumed_pico: i64,
    /// Final execution state.
    pub state: VmState,
    /// Final evaluation stack contents.
    pub stack: Vec<StackValue>,
    /// Optional user-facing fault message.
    #[serde(default)]
    pub fault_message: Option<String>,
    /// Instruction pointer at the attributed fault location.
    #[serde(default)]
    pub fault_ip: Option<u32>,
    /// Serialized local variables snapshot at the attributed fault location.
    #[serde(default)]
    pub fault_locals: Option<Vec<u8>>,
}
