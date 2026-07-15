//! Shared execution limits used by NeoVM consumers.

use alloc::{format, string::String};

/// Maximum NeoVM script size accepted by local execution and proof input code.
pub const MAX_SCRIPT_SIZE: usize = 1024 * 1024;

/// Default maximum evaluation stack depth.
pub const DEFAULT_MAX_STACK_DEPTH: usize = 2048;

/// Default maximum invocation depth.
pub const DEFAULT_MAX_INVOCATION_DEPTH: usize = 1024;

/// Maximum size for buffers and compound values used by bounded execution.
///
/// Matches C# `ExecutionEngineLimits.MaxItemSize = ushort.MaxValue * 2 = 131070`
/// (Neo.VM/ExecutionEngineLimits.cs). Distinct from [`MAX_SCRIPT_SIZE`] (1 MiB):
/// NEWBUFFER/CAT results are capped at the item size, not the script size.
pub const MAX_ITEM_SIZE: usize = (u16::MAX as usize) * 2;

/// Restrictions applied by the NeoVM execution engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionEngineLimits {
    /// Maximum amount the shift opcodes can move bits.
    pub max_shift: i32,
    /// Maximum number of items allowed on the evaluation stack or in slots.
    pub max_stack_size: u32,
    /// Maximum size in bytes of any single stack item.
    pub max_item_size: u32,
    /// Maximum size for items that participate in comparisons.
    pub max_comparable_size: u32,
    /// Maximum depth of the invocation stack.
    pub max_invocation_stack_size: u32,
    /// Maximum nesting depth for try/catch/finally blocks.
    pub max_try_nesting_depth: u32,
    /// Whether engine-generated exceptions can be caught by smart contracts.
    pub catch_engine_exceptions: bool,
    /// Maximum number of instructions that can be executed.
    pub max_instructions: u64,
}

impl ExecutionEngineLimits {
    /// Default execution limits matching the Neo C# reference node.
    pub const DEFAULT: Self = Self {
        max_shift: 256,
        max_stack_size: DEFAULT_MAX_STACK_DEPTH as u32,
        // C# ExecutionEngineLimits.MaxItemSize = ushort.MaxValue * 2 = 131070.
        max_item_size: (u16::MAX as u32) * 2,
        // C# ExecutionEngineLimits.MaxComparableSize = 65536 (not ushort.MaxValue).
        max_comparable_size: 65536,
        max_invocation_stack_size: DEFAULT_MAX_INVOCATION_DEPTH as u32,
        max_try_nesting_depth: 16,
        catch_engine_exceptions: true,
        max_instructions: 1_000_000,
    };

    /// Ensures the provided item size does not exceed the configured limit.
    pub fn assert_max_item_size(&self, size: usize) -> Result<(), String> {
        if size > self.max_item_size as usize {
            return Err(format!(
                "MaxItemSize exceed: {}/{}",
                size, self.max_item_size
            ));
        }
        Ok(())
    }

    /// Ensures the supplied shift value is within bounds.
    pub fn assert_shift(&self, shift: i32) -> Result<(), String> {
        if shift < 0 || shift > self.max_shift {
            return Err(format!("Invalid shift value: {}/{}", shift, self.max_shift));
        }
        Ok(())
    }
}

impl Default for ExecutionEngineLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}
