//! Boolean witness condition helper (mirrors `BooleanCondition.cs`).

use neo_core::WitnessCondition;

/// Creates a boolean witness condition.
pub fn new(value: bool) -> WitnessCondition {
    WitnessCondition::Boolean { value }
}

/// Extracts the boolean value if the condition is of type `Boolean`.
pub fn value(condition: &WitnessCondition) -> Option<bool> {
    match condition {
        WitnessCondition::Boolean { value } => Some(*value),
        _ => None,
    }
}
