//! Called-by-group witness condition helper (mirrors `CalledByGroupCondition.cs`).

use neo_core::WitnessCondition;

/// Creates a called-by-group witness condition using a serialized EC point.
pub fn new(group: Vec<u8>) -> WitnessCondition {
    WitnessCondition::CalledByGroup { group }
}

/// Extracts the group bytes if the condition is `CalledByGroup`.
pub fn group_bytes(condition: &WitnessCondition) -> Option<&[u8]> {
    match condition {
        WitnessCondition::CalledByGroup { group } => Some(group.as_slice()),
        _ => None,
    }
}
