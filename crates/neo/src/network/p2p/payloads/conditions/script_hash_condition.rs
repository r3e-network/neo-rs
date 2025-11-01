//! Script hash witness condition helper (mirrors `ScriptHashCondition.cs`).

use crate::{uint160::UInt160, witness_rule::WitnessCondition};

/// Creates a script-hash witness condition.
pub fn new(hash: UInt160) -> WitnessCondition {
    WitnessCondition::ScriptHash { hash }
}

/// Extracts the script hash if the condition is of type `ScriptHash`.
pub fn hash(condition: &WitnessCondition) -> Option<&UInt160> {
    match condition {
        WitnessCondition::ScriptHash { hash } => Some(hash),
        _ => None,
    }
}
