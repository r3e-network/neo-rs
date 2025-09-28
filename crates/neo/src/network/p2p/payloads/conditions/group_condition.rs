//! Group witness condition helper (mirrors `GroupCondition.cs`).

use crate::witness_rule::WitnessCondition;

/// Creates a group witness condition using a serialized EC point (33-byte compressed form).
pub fn new(group: Vec<u8>) -> WitnessCondition {
    WitnessCondition::Group { group }
}

/// Extracts the group bytes if the condition is of type `Group`.
pub fn group_bytes(condition: &WitnessCondition) -> Option<&[u8]> {
    match condition {
        WitnessCondition::Group { group } => Some(group.as_slice()),
        _ => None,
    }
}
