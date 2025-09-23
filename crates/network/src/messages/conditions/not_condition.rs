//! Logical negation witness condition helper (mirrors `NotCondition.cs`).

use neo_core::WitnessCondition;

/// Creates a `Not` witness condition.
pub fn new(condition: WitnessCondition) -> WitnessCondition {
    WitnessCondition::Not {
        condition: Box::new(condition),
    }
}

/// Returns the inner expression if the given condition is `Not`.
pub fn expression(condition: &WitnessCondition) -> Option<&WitnessCondition> {
    match condition {
        WitnessCondition::Not { condition } => Some(condition.as_ref()),
        _ => None,
    }
}
