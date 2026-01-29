//! Restrictions applied to the Neo VM execution engine.
//!
//! Port of `Neo.VM/ExecutionEngineLimits.cs` from the C# reference node.

use crate::{VmError, VmResult};

/// Describes the operational limits enforced by the execution engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionEngineLimits {
    /// Maximum amount the shift opcodes can move bits.
    pub max_shift: i32,
    /// Maximum number of items allowed on the evaluation stack or in slots.
    pub max_stack_size: u32,
    /// Maximum size (in bytes) of any single stack item.
    pub max_item_size: u32,
    /// Maximum size for items that participate in comparisons.
    pub max_comparable_size: u32,
    /// Maximum depth of the invocation stack.
    pub max_invocation_stack_size: u32,
    /// Maximum nesting depth for try/catch/finally blocks.
    pub max_try_nesting_depth: u32,
    /// Whether engine-generated exceptions can be caught by smart contracts.
    pub catch_engine_exceptions: bool,
}

impl ExecutionEngineLimits {
    /// Default execution limits matching the C# implementation exactly.
    /// In C#: `MaxItemSize` = ushort.MaxValue = 65535
    pub const DEFAULT: Self = Self {
        max_shift: 256,
        max_stack_size: 2 * 1024,             // C#: 2 * 1024
        max_item_size: u16::MAX as u32,       // C#: ushort.MaxValue = 65535
        max_comparable_size: u16::MAX as u32, // C#: ushort.MaxValue = 65535
        max_invocation_stack_size: 1_024,     // C#: 1024
        max_try_nesting_depth: 16,            // C#: 16
        catch_engine_exceptions: true,
    };

    /// Ensures the provided item size does not exceed the configured limit.
    pub fn assert_max_item_size(&self, size: usize) -> VmResult<()> {
        if size > self.max_item_size as usize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxItemSize exceed: {}/{}",
                size, self.max_item_size
            )));
        }
        Ok(())
    }

    /// Ensures the supplied shift value is within bounds.
    pub fn assert_shift(&self, shift: i32) -> VmResult<()> {
        if shift < 0 || shift > self.max_shift {
            return Err(VmError::invalid_operation_msg(format!(
                "Invalid shift value: {}/{}",
                shift, self.max_shift
            )));
        }
        Ok(())
    }
}

impl Default for ExecutionEngineLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}
