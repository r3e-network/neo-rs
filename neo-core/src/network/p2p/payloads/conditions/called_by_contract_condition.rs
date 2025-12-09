//! Called-by-contract witness condition helper (mirrors `CalledByContractCondition.cs`).

use crate::witness_rule::WitnessCondition;
use neo_primitives::UInt160;

/// Creates a called-by-contract witness condition.
pub fn new(hash: UInt160) -> WitnessCondition {
    WitnessCondition::CalledByContract { hash }
}

/// Extracts the contract hash if the condition is `CalledByContract`.
pub fn hash(condition: &WitnessCondition) -> Option<&UInt160> {
    match condition {
        WitnessCondition::CalledByContract { hash } => Some(hash),
        _ => None,
    }
}
