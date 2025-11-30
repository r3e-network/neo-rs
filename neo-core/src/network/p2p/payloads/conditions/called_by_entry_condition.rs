//! Called-by-entry witness condition helper (mirrors `CalledByEntryCondition.cs`).

use crate::witness_rule::WitnessCondition;

/// Returns the singleton called-by-entry condition.
pub fn new() -> WitnessCondition {
    WitnessCondition::CalledByEntry
}

/// Returns `true` if the supplied condition is `CalledByEntry`.
pub fn is_called_by_entry(condition: &WitnessCondition) -> bool {
    matches!(condition, WitnessCondition::CalledByEntry)
}
