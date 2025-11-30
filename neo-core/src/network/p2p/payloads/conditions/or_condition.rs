//! Logical disjunction witness condition helper (mirrors `OrCondition.cs`).

use crate::witness_rule::WitnessCondition;

/// Creates an `Or` witness condition from the supplied sub-conditions.
pub fn new(conditions: Vec<WitnessCondition>) -> WitnessCondition {
    WitnessCondition::Or { conditions }
}

/// Returns the expressions if the given condition is `Or`.
pub fn expressions(condition: &WitnessCondition) -> Option<&[WitnessCondition]> {
    match condition {
        WitnessCondition::Or { conditions } => Some(conditions.as_slice()),
        _ => None,
    }
}
