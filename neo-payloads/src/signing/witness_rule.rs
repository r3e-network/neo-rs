// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! WitnessRule - Conditional witness validation for Neo N3.
//!
//! This module provides witness rule evaluation for conditional transaction
//! verification, matching the C# Neo implementation exactly.
//!
//! ## Overview
//!
//! Witness rules allow fine-grained control over when witnesses are accepted:
//! - **Action**: `Allow` or `Deny` the witness
//! - **Condition**: Logical conditions evaluated against the execution context
//!
//! ## Condition Types
//!
//! | Condition | Description |
//! |-----------|-------------|
//! | `Boolean` | Fixed true/false value |
//! | `Not` | Logical negation |
//! | `And`/`Or` | Logical combination |
//! | `ScriptHash` | Match specific contract hash |
//! | `Group` | Match validator group |
//! | `CalledByEntry` | Called by entry script |
//! | `CalledByContract` | Called by specific contract |
//! | `CalledByGroup` | Called by specific group |

use neo_io::serializable::helper::SerializeHelper;

#[path = "../witness_rule/display.rs"]
mod display;
/// Constructors and parsing helpers for witness conditions and rules.
#[path = "../witness_rule/helpers.rs"]
pub mod helpers;
#[path = "../witness_rule/json.rs"]
mod json;
#[path = "../witness_rule/serialization.rs"]
mod serialization;

// Stack projection lives in neo-core (depends on the VM crate) so neo-io stays
// free of any VM dependency.
#[path = "../witness_rule/stack_projection.rs"]
mod stack_projection;

pub use neo_primitives::WitnessConditionType;
pub use neo_primitives::WitnessRuleAction;

#[cfg(test)]
#[path = "../tests/signing/witness_rule.rs"]
mod tests;

/// Represents a witness condition (matches C# WitnessCondition exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitnessCondition {
    /// Boolean condition with a fixed value.
    Boolean {
        /// Fixed boolean result for this condition.
        value: bool,
    },
    /// Not condition that negates another condition.
    Not {
        /// Condition whose result is negated.
        condition: Box<WitnessCondition>,
    },
    /// And condition that requires all sub-conditions to be true.
    And {
        /// Conditions that must all evaluate to true.
        conditions: Vec<WitnessCondition>,
    },
    /// Or condition that requires at least one sub-condition to be true.
    Or {
        /// Conditions where at least one must evaluate to true.
        conditions: Vec<WitnessCondition>,
    },
    /// Script hash condition that checks if the current script hash matches.
    ScriptHash {
        /// Script hash to compare with the current script.
        hash: neo_primitives::UInt160,
    },
    /// Group condition that checks if the current group matches.
    Group {
        /// Compressed secp256r1 ECPoint bytes identifying the group.
        group: Vec<u8>,
    },
    /// Called by entry condition.
    CalledByEntry,
    /// Called by contract condition that checks if called by a specific contract.
    CalledByContract {
        /// Calling contract script hash.
        hash: neo_primitives::UInt160,
    },
    /// Called by group condition that checks if called by a specific group.
    CalledByGroup {
        /// Compressed secp256r1 ECPoint bytes identifying the calling group.
        group: Vec<u8>,
    },
}

/// Size of UInt160 in bytes (matches C# UInt160.Length).
const ADDRESS_SIZE: usize = 20;

/// The rule used to describe the scope of the witness (matches C# WitnessRule exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessRule {
    /// Indicates the action to be taken if the current context meets with the rule.
    pub action: WitnessRuleAction,
    /// The condition of the rule.
    pub condition: WitnessCondition,
}

impl WitnessCondition {
    /// Maximum number of sub-items allowed (matches C# MaxSubitems exactly).
    pub const MAX_SUBITEMS: usize = 16;
    /// Maximum nesting depth allowed (matches C# MaxNestingDepth exactly).
    pub const MAX_NESTING_DEPTH: usize = 3;

    /// Gets the type of the condition (matches C# Type property exactly).
    pub fn condition_type(&self) -> WitnessConditionType {
        match self {
            WitnessCondition::Boolean { .. } => WitnessConditionType::Boolean,
            WitnessCondition::Not { .. } => WitnessConditionType::Not,
            WitnessCondition::And { .. } => WitnessConditionType::And,
            WitnessCondition::Or { .. } => WitnessConditionType::Or,
            WitnessCondition::ScriptHash { .. } => WitnessConditionType::ScriptHash,
            WitnessCondition::Group { .. } => WitnessConditionType::Group,
            WitnessCondition::CalledByEntry => WitnessConditionType::CalledByEntry,
            WitnessCondition::CalledByContract { .. } => WitnessConditionType::CalledByContract,
            WitnessCondition::CalledByGroup { .. } => WitnessConditionType::CalledByGroup,
        }
    }

    /// Validates the condition structure (matches C# validation exactly).
    pub fn is_valid(&self, max_depth: usize) -> bool {
        if max_depth == 0 {
            return false;
        }

        match self {
            WitnessCondition::Boolean { .. } => true,
            WitnessCondition::CalledByEntry => true,
            WitnessCondition::ScriptHash { .. } => true,
            WitnessCondition::Group { .. } => true,
            WitnessCondition::CalledByContract { .. } => true,
            WitnessCondition::CalledByGroup { .. } => true,
            WitnessCondition::Not { condition } => condition.is_valid(max_depth - 1),
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                if conditions.is_empty() || conditions.len() > Self::MAX_SUBITEMS {
                    return false;
                }
                conditions.iter().all(|c| c.is_valid(max_depth - 1))
            }
        }
    }

    /// Calculates the size of the condition when serialized (matches C# Size property exactly).
    pub fn size(&self) -> usize {
        let payload = match self {
            WitnessCondition::Boolean { .. } => 1, // bool
            WitnessCondition::Not { condition } => condition.size(),
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                SerializeHelper::get_var_size_serializable_slice(conditions)
            }
            WitnessCondition::ScriptHash { .. } => ADDRESS_SIZE,
            WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => {
                group.len()
            }
            WitnessCondition::CalledByEntry => 0,
            WitnessCondition::CalledByContract { .. } => ADDRESS_SIZE,
        };
        1 + payload
    }

    /// Calculates the size of the condition when serialized (matches earlier `len` helper`).
    pub fn len(&self) -> usize {
        self.size()
    }

    /// Returns true if the condition has zero size when serialized
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }
}

impl WitnessRule {
    /// Creates a new WitnessRule (matches C# constructor exactly).
    pub fn new(action: WitnessRuleAction, condition: WitnessCondition) -> Self {
        Self { action, condition }
    }

    /// Validates the rule (matches C# validation exactly).
    pub fn is_valid(&self) -> bool {
        self.condition.is_valid(WitnessCondition::MAX_NESTING_DEPTH)
    }

    /// Calculates the serialized size of the rule.
    pub fn size(&self) -> usize {
        1 + self.condition.size()
    }
}
