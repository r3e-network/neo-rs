//! Logical conjunction witness condition helper (mirrors `AndCondition.cs`).

use neo_core::WitnessCondition;

/// Creates an `And` witness condition from the supplied sub-conditions.
pub fn new(conditions: Vec<WitnessCondition>) -> WitnessCondition {
    WitnessCondition::And { conditions }
}

/// Returns the expressions if the given condition is `And`.
pub fn expressions(condition: &WitnessCondition) -> Option<&[WitnessCondition]> {
    match condition {
        WitnessCondition::And { conditions } => Some(conditions.as_slice()),
        _ => None,
    }
}
