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
/// Matches C# `ExecutionEngineLimits.MaxItemSize` (`ushort.MaxValue * 2`).
/// NB: this was `1024 * 1024` up to neo-vm v3.6.x and changed to `ushort.MaxValue * 2`
/// afterwards; Neo N3 v3.9.x requires the smaller value.
pub const MAX_ITEM_SIZE: usize = u16::MAX as usize * 2;

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
    /// Whether a zero-bit SHL/SHR converts the operand to `Integer` before
    /// pushing it (Gorgon hardfork semantics). When `false`, a zero shift
    /// preserves the operand's original stack item type (pre-Gorgon).
    pub zero_shift_converts_to_integer: bool,
    /// Maximum number of instructions that can be executed.
    ///
    /// The C# reference `ExecutionEngineLimits` has no instruction-count limit:
    /// on-chain execution is bounded by protocol gas, not by a local budget.
    /// `DEFAULT` therefore disables the limit (`u64::MAX`); callers that need a
    /// service-level budget (RPC admission, probes) may set a lower value.
    pub max_instructions: u64,
}

impl ExecutionEngineLimits {
    /// Default execution limits matching the Neo C# reference node.
    pub const DEFAULT: Self = Self {
        max_shift: 256,
        max_stack_size: DEFAULT_MAX_STACK_DEPTH as u32,
        // C#: `MaxItemSize = ushort.MaxValue * 2` -> 131_070
        max_item_size: u16::MAX as u32 * 2,
        // C#: `MaxComparableSize = 65536` -> note this is `u16::MAX + 1`, not `u16::MAX`
        max_comparable_size: u16::MAX as u32 + 1,
        max_invocation_stack_size: DEFAULT_MAX_INVOCATION_DEPTH as u32,
        max_try_nesting_depth: 16,
        catch_engine_exceptions: true,
        // C# ExecutionEngineLimits has no instruction-count cap; gas is the
        // protocol resource bound. A local 1,000,000 cap here turned a
        // service-level budget into a consensus validity rule (FAULT) that the
        // reference implementation does not have.
        max_instructions: u64::MAX,
        // Neo N3 v3.10.1 targets activate the Gorgon hardfork; pre-Gorgon
        // replay must clear this flag (see ApplicationEngine construction).
        zero_shift_converts_to_integer: true,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_do_not_cap_instructions() {
        // R05: the C# reference ExecutionEngineLimits has no instruction-count
        // cap — gas is the protocol resource bound. A local 1,000,000 cap
        // turned a service-level budget into a consensus validity rule.
        assert_eq!(ExecutionEngineLimits::DEFAULT.max_instructions, u64::MAX);
    }

    #[test]
    fn default_limits_use_gorgon_shift_semantics() {
        // R06: the shipped default targets Neo N3 v3.10.1 where Gorgon is
        // active; pre-Gorgon replay opts out via ApplicationEngine.
        assert!(ExecutionEngineLimits::DEFAULT.zero_shift_converts_to_integer);
    }
}
