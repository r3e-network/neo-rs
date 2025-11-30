//! Witness condition facade for network payloads.
//!
//! The canonical implementation lives in `crate::witness_rule::WitnessCondition`; this module re-exports the
//! type and exposes the same constants as the C# hierarchy so the network crate can stay aligned
//! with the original layout.

pub use crate::witness_rule::WitnessCondition;
pub use crate::witness_rule::WitnessConditionType;

/// Maximum number of sub-items allowed inside composite conditions (matches C# `MaxSubitems`).
pub const MAX_SUBITEMS: usize = crate::witness_rule::WitnessCondition::MAX_SUBITEMS;
/// Maximum nesting depth for composite conditions (matches C# `MaxNestingDepth`).
pub const MAX_NESTING_DEPTH: usize = crate::witness_rule::WitnessCondition::MAX_NESTING_DEPTH;

/// Returns `true` when the condition can be nested respecting Neo's depth limits.
pub fn is_valid(condition: &WitnessCondition, max_depth: usize) -> bool {
    condition.is_valid(max_depth)
}
