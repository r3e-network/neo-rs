//! Witness condition helper compatibility surface for P2P consumers.
//!
//! The canonical witness-rule data types live in `neo-ledger-types`; this
//! module keeps the historical P2P helper paths without pulling VM or runtime
//! dependencies into the network crate.

macro_rules! composite_condition_module {
    ($module:ident, $variant:ident) => {
        pub mod $module {
            use crate::witness_rule::WitnessCondition;

            /// Creates a composite witness condition from the supplied sub-conditions.
            pub fn new(conditions: Vec<WitnessCondition>) -> WitnessCondition {
                WitnessCondition::$variant { conditions }
            }

            /// Returns the expressions if the condition has this composite type.
            pub fn expressions(condition: &WitnessCondition) -> Option<&[WitnessCondition]> {
                match condition {
                    WitnessCondition::$variant { conditions } => Some(conditions.as_slice()),
                    _ => None,
                }
            }
        }
    };
}

macro_rules! group_condition_module {
    ($module:ident, $variant:ident) => {
        pub mod $module {
            use crate::witness_rule::WitnessCondition;

            /// Creates a witness condition using a serialized EC point.
            pub fn new(group: Vec<u8>) -> WitnessCondition {
                WitnessCondition::$variant { group }
            }

            /// Extracts the group bytes if the condition has this group type.
            pub fn group_bytes(condition: &WitnessCondition) -> Option<&[u8]> {
                match condition {
                    WitnessCondition::$variant { group } => Some(group.as_slice()),
                    _ => None,
                }
            }
        }
    };
}

macro_rules! hash_condition_module {
    ($module:ident, $variant:ident) => {
        pub mod $module {
            use crate::witness_rule::WitnessCondition;
            use neo_primitives::UInt160;

            /// Creates a witness condition with a UInt160 script hash.
            pub fn new(hash: UInt160) -> WitnessCondition {
                WitnessCondition::$variant { hash }
            }

            /// Extracts the hash if the condition has this hash type.
            pub fn hash(condition: &WitnessCondition) -> Option<&UInt160> {
                match condition {
                    WitnessCondition::$variant { hash } => Some(hash),
                    _ => None,
                }
            }
        }
    };
}

composite_condition_module!(and_condition, And);
composite_condition_module!(or_condition, Or);
group_condition_module!(group_condition, Group);
group_condition_module!(called_by_group_condition, CalledByGroup);
hash_condition_module!(called_by_contract_condition, CalledByContract);
hash_condition_module!(script_hash_condition, ScriptHash);

pub mod boolean_condition {
    use crate::witness_rule::WitnessCondition;

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
}

pub mod called_by_entry_condition {
    use crate::witness_rule::WitnessCondition;

    /// Returns the singleton called-by-entry condition.
    pub fn new() -> WitnessCondition {
        WitnessCondition::CalledByEntry
    }

    /// Returns `true` if the supplied condition is `CalledByEntry`.
    pub fn is_called_by_entry(condition: &WitnessCondition) -> bool {
        matches!(condition, WitnessCondition::CalledByEntry)
    }
}

pub mod not_condition {
    use crate::witness_rule::WitnessCondition;

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
}

pub mod witness_condition {
    pub use crate::witness_rule::{WitnessCondition, WitnessConditionType};

    /// Maximum number of sub-items allowed inside composite conditions.
    pub const MAX_SUBITEMS: usize = crate::witness_rule::WitnessCondition::MAX_SUBITEMS;
    /// Maximum nesting depth for composite conditions.
    pub const MAX_NESTING_DEPTH: usize = crate::witness_rule::WitnessCondition::MAX_NESTING_DEPTH;

    /// Returns `true` when the condition can be nested respecting Neo's depth limits.
    pub fn is_valid(condition: &WitnessCondition, max_depth: usize) -> bool {
        condition.is_valid(max_depth)
    }
}

pub use and_condition::new as new_and;
pub use boolean_condition::{new as new_boolean, value as boolean_value};
pub use called_by_contract_condition::{
    hash as called_by_contract_hash, new as new_called_by_contract,
};
pub use called_by_entry_condition::{is_called_by_entry, new as new_called_by_entry};
pub use called_by_group_condition::{
    group_bytes as called_by_group_bytes, new as new_called_by_group,
};
pub use group_condition::{group_bytes, new as new_group};
pub use not_condition::{expression as not_expression, new as new_not};
pub use or_condition::{expressions as or_expressions, new as new_or};
pub use script_hash_condition::{hash as script_hash_value, new as new_script_hash};
pub use witness_condition::{
    MAX_NESTING_DEPTH, MAX_SUBITEMS, WitnessCondition, WitnessConditionType, is_valid,
};
