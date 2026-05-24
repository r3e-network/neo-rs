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
//!
//! ## Example
//!
//! ```rust
//! use neo_core::witness_rule::{WitnessRule, WitnessRuleAction, WitnessCondition};
//! use neo_core::UInt160;
//!
//! // Create a rule that allows if called by entry
//! let rule = WitnessRule::new(
//!     WitnessRuleAction::Allow,
//!     WitnessCondition::CalledByEntry,
//! );
//!
//! // Create a rule that denies if script hash matches
//! let script_hash = UInt160::zero();
//! let deny_rule = WitnessRule::new(
//!     WitnessRuleAction::Deny,
//!     WitnessCondition::ScriptHash { hash: script_hash },
//! );
//! ```

use crate::neo_config::ADDRESS_SIZE;
use crate::neo_io::serializable::helper::get_var_size;
use crate::vm_runtime::StackItem;
use neo_vm_rs::StackValue;
use serde::de::Error as SerdeDeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;

mod display;
mod helpers;
mod json;
mod serialization;

/// The action to be taken if the current context meets with the rule (matches C# WitnessRuleAction exactly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WitnessRuleAction {
    /// Deny the witness if the condition is met.
    Deny = 0,
    /// Allow the witness if the condition is met.
    Allow = 1,
}

impl WitnessRuleAction {
    /// Converts the action to its wire-format byte.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates an action from its wire-format byte.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Deny),
            1 => Some(Self::Allow),
            _ => None,
        }
    }
}

impl Serialize for WitnessRuleAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for WitnessRuleAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        WitnessRuleAction::from_byte(byte)
            .ok_or_else(|| D::Error::custom(format!("Invalid witness rule action byte: {byte}")))
    }
}

impl FromStr for WitnessRuleAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Deny" | "deny" => Ok(Self::Deny),
            "Allow" | "allow" => Ok(Self::Allow),
            other => Err(format!("Invalid witness rule action: {other}")),
        }
    }
}

/// The type of witness condition (matches C# WitnessConditionType exactly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WitnessConditionType {
    /// Boolean condition.
    Boolean = 0x00,
    /// Not condition (logical NOT).
    Not = 0x01,
    /// And condition (logical AND).
    And = 0x02,
    /// Or condition (logical OR).
    Or = 0x03,
    /// Script hash condition.
    ScriptHash = 0x18,
    /// Group condition.
    Group = 0x19,
    /// Called by entry condition.
    CalledByEntry = 0x20,
    /// Called by contract condition.
    CalledByContract = 0x28,
    /// Called by group condition.
    CalledByGroup = 0x29,
}

impl WitnessConditionType {
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Boolean),
            0x01 => Some(Self::Not),
            0x02 => Some(Self::And),
            0x03 => Some(Self::Or),
            0x18 => Some(Self::ScriptHash),
            0x19 => Some(Self::Group),
            0x20 => Some(Self::CalledByEntry),
            0x28 => Some(Self::CalledByContract),
            0x29 => Some(Self::CalledByGroup),
            _ => None,
        }
    }
}

impl Serialize for WitnessConditionType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for WitnessConditionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        WitnessConditionType::from_byte(byte)
            .ok_or_else(|| D::Error::custom(format!("Invalid witness condition type byte: {byte}")))
    }
}

/// Represents a witness condition (matches C# WitnessCondition exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitnessCondition {
    /// Boolean condition with a fixed value.
    Boolean { value: bool },
    /// Not condition that negates another condition.
    Not { condition: Box<WitnessCondition> },
    /// And condition that requires all sub-conditions to be true.
    And { conditions: Vec<WitnessCondition> },
    /// Or condition that requires at least one sub-condition to be true.
    Or { conditions: Vec<WitnessCondition> },
    /// Script hash condition that checks if the current script hash matches.
    ScriptHash { hash: crate::UInt160 },
    /// Group condition that checks if the current group matches.
    Group { group: Vec<u8> }, // ECPoint serialized as bytes (matches C# ECPoint exactly)
    /// Called by entry condition.
    CalledByEntry,
    /// Called by contract condition that checks if called by a specific contract.
    CalledByContract { hash: crate::UInt160 },
    /// Called by group condition that checks if called by a specific group.
    CalledByGroup { group: Vec<u8> }, // ECPoint serialized as bytes (matches C# ECPoint exactly)
}

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
                get_var_size(conditions.len() as u64)
                    + conditions.iter().map(|c| c.size()).sum::<usize>()
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

    /// Converts the witness condition to a neo-vm-rs stack value (matches C# `WitnessCondition.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        let mut items = vec![StackValue::Integer(i64::from(
            self.condition_type().to_byte(),
        ))];

        match self {
            WitnessCondition::Boolean { value } => {
                items.push(StackValue::Boolean(*value));
            }
            WitnessCondition::Not { condition } => {
                items.push(condition.to_stack_value());
            }
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                let expressions = conditions
                    .iter()
                    .map(WitnessCondition::to_stack_value)
                    .collect::<Vec<_>>();
                items.push(StackValue::Array(expressions));
            }
            WitnessCondition::ScriptHash { hash } | WitnessCondition::CalledByContract { hash } => {
                items.push(StackValue::ByteString(hash.to_bytes()));
            }
            WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => {
                items.push(StackValue::ByteString(group.clone()));
            }
            WitnessCondition::CalledByEntry => {}
        }

        StackValue::Array(items)
    }

    /// Converts the witness condition to a VM stack item (matches C# `WitnessCondition.ToStackItem`).
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::try_from(self.to_stack_value()).expect(
            "witness condition StackValue projection uses only VM StackItem-compatible values",
        )
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

    pub fn size(&self) -> usize {
        1 + self.condition.size()
    }

    /// Converts the witness rule to a neo-vm-rs stack value (matches C# `WitnessRule.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::Integer(i64::from(self.action.to_byte())),
            self.condition.to_stack_value(),
        ])
    }

    /// Converts the witness rule to a VM stack item (matches C# `WitnessRule.ToStackItem`).
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::try_from(self.to_stack_value())
            .expect("witness rule StackValue projection uses only VM StackItem-compatible values")
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests;
