//! `WitnessCondition` / `WitnessRule` — protocol-level witness-rule models.
//!
//! These types describe *when* a witness is accepted (`Allow`/`Deny` plus a
//! condition tree over script hashes, groups, caller relationships and
//! boolean combinators). They are protocol concepts, not I/O concepts, and
//! are consumed by several layers at once:
//!
//! * `neo-io` implements the Neo binary codec for them (local `Serializable`
//!   trait), because that crate owns the generic serialization machinery.
//! * `neo-vm` implements the stack projection (`ToStackItem`) because it owns
//!   `StackValue`/`StackItem`.
//! * `neo-protocol` re-exports them as payload types.
//!
//! Because two of those layers are siblings, the model itself has to live in
//! the shared foundation crate. `neo-primitives` is a Layer 0 crate that both
//! `neo-vm` and `neo-protocol` depend on, so it can own the types without
//! creating a dependency edge (or a trait-impl orphan problem) between them.

use crate::{UInt160, WitnessConditionType, WitnessRuleAction};
use serde::de::Error as SerdeDeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};
use std::fmt;
use std::str::FromStr;

/// Size of a `UInt160` in bytes (matches C# `UInt160.Length`).
const ADDRESS_SIZE: usize = 20;

/// Size of a compressed ECPoint in bytes.
pub const ECPOINT_COMPRESSED_SIZE: usize = 33;
/// Size of an uncompressed ECPoint in bytes.
pub const ECPOINT_UNCOMPRESSED_SIZE: usize = 65;

/// Represents a witness condition (matches C# `WitnessCondition` exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitnessCondition {
    /// Boolean condition with a fixed value.
    Boolean {
        /// The fixed boolean value.
        value: bool,
    },
    /// Not condition that negates another condition.
    Not {
        /// The condition being negated.
        condition: Box<WitnessCondition>,
    },
    /// And condition that requires all sub-conditions to be true.
    And {
        /// All sub-conditions that must hold.
        conditions: Vec<WitnessCondition>,
    },
    /// Or condition that requires at least one sub-condition to be true.
    Or {
        /// Sub-conditions, at least one of which must hold.
        conditions: Vec<WitnessCondition>,
    },
    /// Script hash condition that checks if the current script hash matches.
    ScriptHash {
        /// The contract hash to match.
        hash: UInt160,
    },
    /// Group condition that checks if the current group matches.
    Group {
        /// The compressed group public key (ECPoint bytes).
        group: Vec<u8>,
    },
    /// Called by entry condition.
    CalledByEntry,
    /// Called by contract condition that checks if called by a specific contract.
    CalledByContract {
        /// The contract hash that must have called.
        hash: UInt160,
    },
    /// Called by group condition that checks if called by a specific group.
    CalledByGroup {
        /// The compressed group public key (ECPoint bytes).
        group: Vec<u8>,
    },
}

/// The rule used to describe the scope of the witness (matches C#
/// `WitnessRule` exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessRule {
    /// Indicates the action to be taken if the current context meets with the rule.
    pub action: WitnessRuleAction,
    /// The condition of the rule.
    pub condition: WitnessCondition,
}

impl WitnessCondition {
    /// Maximum number of sub-items allowed (matches C# `MaxSubitems` exactly).
    pub const MAX_SUBITEMS: usize = 16;
    /// Maximum nesting depth allowed (matches C# `MaxNestingDepth` exactly).
    pub const MAX_NESTING_DEPTH: usize = 3;

    /// Gets the type of the condition (matches C# `Type` property exactly).
    #[must_use]
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
    #[must_use]
    pub fn is_valid(&self, max_depth: usize) -> bool {
        if max_depth == 0 {
            return false;
        }

        match self {
            WitnessCondition::Boolean { .. }
            | WitnessCondition::CalledByEntry
            | WitnessCondition::ScriptHash { .. }
            | WitnessCondition::Group { .. }
            | WitnessCondition::CalledByContract { .. }
            | WitnessCondition::CalledByGroup { .. } => true,
            WitnessCondition::Not { condition } => condition.is_valid(max_depth - 1),
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                if conditions.is_empty() || conditions.len() > Self::MAX_SUBITEMS {
                    return false;
                }
                conditions.iter().all(|c| c.is_valid(max_depth - 1))
            }
        }
    }

    /// Calculates the size of the condition when serialized (matches C#
    /// `Size` property exactly).
    #[must_use]
    pub fn size(&self) -> usize {
        let payload = match self {
            WitnessCondition::Boolean { .. } => 1, // bool
            WitnessCondition::Not { condition } => condition.size(),
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                var_size_usize(conditions.len())
                    + conditions.iter().map(WitnessCondition::size).sum::<usize>()
            }
            WitnessCondition::ScriptHash { .. } => ADDRESS_SIZE,
            WitnessCondition::Group { .. } | WitnessCondition::CalledByGroup { .. } => {
                ECPOINT_COMPRESSED_SIZE // constant, not group.len()
            }
            WitnessCondition::CalledByEntry => 0,
            WitnessCondition::CalledByContract { .. } => ADDRESS_SIZE,
        };
        1 + payload
    }

    /// Calculates the size of the condition when serialized.
    #[must_use]
    pub fn len(&self) -> usize {
        self.size()
    }

    /// Returns true if the condition has zero size when serialized.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Serializes this condition to a JSON value.
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            WitnessCondition::Boolean { value } => json!({
                "type": "Boolean",
                "expression": value,
            }),
            WitnessCondition::Not { condition } => json!({
                "type": "Not",
                "expression": condition.to_json(),
            }),
            WitnessCondition::And { conditions } => json!({
                "type": "And",
                "expressions": conditions.iter().map(WitnessCondition::to_json).collect::<Vec<_>>(),
            }),
            WitnessCondition::Or { conditions } => json!({
                "type": "Or",
                "expressions": conditions.iter().map(WitnessCondition::to_json).collect::<Vec<_>>(),
            }),
            WitnessCondition::ScriptHash { hash } => json!({
                "type": "ScriptHash",
                "hash": hash.to_string(),
            }),
            WitnessCondition::Group { group } => json!({
                "type": "Group",
                "group": hex::encode(group),
            }),
            WitnessCondition::CalledByEntry => json!({
                "type": "CalledByEntry",
            }),
            WitnessCondition::CalledByContract { hash } => json!({
                "type": "CalledByContract",
                "hash": hash.to_string(),
            }),
            WitnessCondition::CalledByGroup { group } => json!({
                "type": "CalledByGroup",
                "group": hex::encode(group),
            }),
        }
    }

    /// Parses a condition from a JSON value at the default maximum nesting depth.
    ///
    /// # Errors
    ///
    /// Returns a message when the value is not a recognised witness condition.
    pub fn from_json(json: &Value) -> Result<Self, String> {
        Self::from_json_with_depth(json, Self::MAX_NESTING_DEPTH)
    }

    /// Parses a condition from a JSON value, enforcing a maximum nesting depth.
    ///
    /// # Errors
    ///
    /// Returns a message when the value is malformed or exceeds `max_depth`.
    pub fn from_json_with_depth(json: &Value, max_depth: usize) -> Result<Self, String> {
        if max_depth == 0 {
            return Err("Max nesting depth exceeded".to_string());
        }

        let condition_type = json
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "Condition type missing".to_string())?;
        match condition_type {
            "Boolean" => {
                let value = json
                    .get("expression")
                    .and_then(Value::as_bool)
                    .or_else(|| json.get("value").and_then(Value::as_bool))
                    .ok_or_else(|| "Boolean condition missing expression".to_string())?;
                Ok(WitnessCondition::Boolean { value })
            }
            "Not" => {
                let expression = json
                    .get("expression")
                    .ok_or_else(|| "Not condition missing expression".to_string())?;
                let inner = WitnessCondition::from_json_with_depth(expression, max_depth - 1)?;
                Ok(WitnessCondition::Not {
                    condition: Box::new(inner),
                })
            }
            "And" | "Or" => {
                let expressions = json
                    .get("expressions")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("{condition_type} condition missing expressions"))?;
                if expressions.is_empty() {
                    return Err(format!(
                        "{condition_type} condition requires at least one expression"
                    ));
                }
                if expressions.len() > Self::MAX_SUBITEMS {
                    return Err(format!(
                        "{condition_type} condition exceeds {} subitems",
                        Self::MAX_SUBITEMS
                    ));
                }
                let mut conditions = Vec::with_capacity(expressions.len());
                for expression in expressions {
                    conditions.push(WitnessCondition::from_json_with_depth(
                        expression,
                        max_depth - 1,
                    )?);
                }
                if condition_type == "And" {
                    Ok(WitnessCondition::And { conditions })
                } else {
                    Ok(WitnessCondition::Or { conditions })
                }
            }
            "ScriptHash" => {
                let hash_str = json
                    .get("hash")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "ScriptHash condition missing hash".to_string())?;
                let hash =
                    UInt160::from_str(hash_str).map_err(|e| format!("Invalid script hash: {e}"))?;
                Ok(WitnessCondition::ScriptHash { hash })
            }
            "Group" => {
                let group_str = json
                    .get("group")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Group condition missing group".to_string())?;
                let group = parse_group_bytes(group_str)?;
                Ok(WitnessCondition::Group { group })
            }
            "CalledByEntry" => Ok(WitnessCondition::CalledByEntry),
            "CalledByContract" => {
                let hash_str = json
                    .get("hash")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "CalledByContract missing hash".to_string())?;
                let hash =
                    UInt160::from_str(hash_str).map_err(|e| format!("Invalid script hash: {e}"))?;
                Ok(WitnessCondition::CalledByContract { hash })
            }
            "CalledByGroup" => {
                let group_str = json
                    .get("group")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "CalledByGroup missing group".to_string())?;
                let group = parse_group_bytes(group_str)?;
                Ok(WitnessCondition::CalledByGroup { group })
            }
            other => Err(format!("Unsupported witness condition type: {other}")),
        }
    }
}

impl WitnessRule {
    /// Creates a new `WitnessRule` (matches C# constructor exactly).
    #[must_use]
    pub fn new(action: WitnessRuleAction, condition: WitnessCondition) -> Self {
        Self { action, condition }
    }

    /// Validates the rule (matches C# validation exactly).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.condition.is_valid(WitnessCondition::MAX_NESTING_DEPTH)
    }

    /// Serialized size of the rule (action byte + condition).
    #[must_use]
    pub fn size(&self) -> usize {
        1 + self.condition.size()
    }

    /// Serializes this rule to a JSON value.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "action": self.action.to_string(),
            "condition": self.condition.to_json(),
        })
    }

    /// Parses a rule from a JSON value at the default maximum nesting depth.
    ///
    /// # Errors
    ///
    /// Returns a message when the value is not a recognised witness rule.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        Self::from_json_with_depth(value, WitnessCondition::MAX_NESTING_DEPTH)
    }

    /// Parses a rule from a JSON value, enforcing a maximum nesting depth.
    ///
    /// # Errors
    ///
    /// Returns a message when the value is malformed or exceeds `max_depth`.
    pub fn from_json_with_depth(value: &Value, max_depth: usize) -> Result<Self, String> {
        let action_str = value
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| "WitnessRule missing action".to_string())?;
        let action: WitnessRuleAction = action_str.parse()?;
        let condition_value = value
            .get("condition")
            .ok_or_else(|| "WitnessRule missing condition".to_string())?;
        let condition = WitnessCondition::from_json_with_depth(condition_value, max_depth)?;
        Ok(Self { action, condition })
    }
}

impl fmt::Display for WitnessCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WitnessCondition::Boolean { value } => write!(f, "Boolean({value})"),
            WitnessCondition::Not { condition } => write!(f, "Not({condition})"),
            WitnessCondition::And { conditions } => write!(
                f,
                "And([{}])",
                conditions
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            WitnessCondition::Or { conditions } => write!(
                f,
                "Or([{}])",
                conditions
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            WitnessCondition::ScriptHash { hash } => write!(f, "ScriptHash({hash})"),
            WitnessCondition::Group { group } => write!(f, "Group({group:?})"),
            WitnessCondition::CalledByEntry => write!(f, "CalledByEntry"),
            WitnessCondition::CalledByContract { hash } => write!(f, "CalledByContract({hash})"),
            WitnessCondition::CalledByGroup { group } => write!(f, "CalledByGroup({group:?})"),
        }
    }
}

impl fmt::Display for WitnessRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WitnessRule {{ action: {}, condition: {} }}",
            self.action, self.condition
        )
    }
}

impl Serialize for WitnessCondition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WitnessCondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        WitnessCondition::from_json(&value).map_err(SerdeDeError::custom)
    }
}

impl Serialize for WitnessRule {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WitnessRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        WitnessRule::from_json(&value).map_err(SerdeDeError::custom)
    }
}

/// Encoded size of `value` as a Neo variable-length integer.
///
/// Mirrors `neo_io::serializable::helper::get_var_size_usize`, which cannot be
/// used here: `neo-io` builds on this crate, not the other way around.
const fn var_size_usize(value: usize) -> usize {
    if value < 0xFD {
        1
    } else if value <= 0xFFFF {
        3
    } else if value <= 0xFFFF_FFFF {
        5
    } else {
        9
    }
}

/// Parses a hex-encoded ECPoint group, validating the byte length.
fn parse_group_bytes(value: &str) -> Result<Vec<u8>, String> {
    let stripped = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(stripped).map_err(|e| format!("Invalid hex string: {e}"))?;
    match bytes.len() {
        ECPOINT_COMPRESSED_SIZE | ECPOINT_UNCOMPRESSED_SIZE => Ok(bytes),
        _ => Err(format!(
            "Invalid ECPoint length: expected {} or {} bytes, got {}",
            ECPOINT_COMPRESSED_SIZE,
            ECPOINT_UNCOMPRESSED_SIZE,
            bytes.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> Vec<u8> {
        let mut bytes = vec![0x02];
        bytes.extend_from_slice(&[0x11; ECPOINT_COMPRESSED_SIZE - 1]);
        bytes
    }

    #[test]
    fn and_rule_size_matches_binary_layout() {
        let rule = WitnessRule::new(
            WitnessRuleAction::Allow,
            WitnessCondition::And {
                conditions: vec![
                    WitnessCondition::CalledByEntry,
                    WitnessCondition::ScriptHash {
                        hash: UInt160::zero(),
                    },
                ],
            },
        );

        // action(1) + type(1) + array var-int(1) + entry(1) + scripthash(1 + 20)
        assert_eq!(rule.size(), 1 + 1 + 1 + 1 + 21);
    }

    #[test]
    fn json_round_trips_nested_conditions() {
        let rule = WitnessRule::new(
            WitnessRuleAction::Allow,
            WitnessCondition::Or {
                conditions: vec![
                    WitnessCondition::CalledByGroup { group: group() },
                    WitnessCondition::Not {
                        condition: Box::new(WitnessCondition::CalledByEntry),
                    },
                ],
            },
        );

        let json = rule.to_json();
        assert_eq!(WitnessRule::from_json(&json).unwrap(), rule);
    }

    #[test]
    fn validation_rejects_empty_composite_and_deep_nesting() {
        assert!(
            !WitnessRule::new(
                WitnessRuleAction::Allow,
                WitnessCondition::And {
                    conditions: Vec::new(),
                }
            )
            .is_valid()
        );

        let deep = WitnessCondition::Not {
            condition: Box::new(WitnessCondition::Not {
                condition: Box::new(WitnessCondition::Not {
                    condition: Box::new(WitnessCondition::CalledByEntry),
                }),
            }),
        };
        // Each `Not` consumes one level of the depth budget, so three nested `Not`s
        // need a budget of four and exceed the protocol default of three.
        assert!(!deep.is_valid(3));
        assert!(deep.is_valid(4));
    }
}
