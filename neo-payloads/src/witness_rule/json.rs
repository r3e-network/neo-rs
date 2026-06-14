use super::helpers::{encode_hex, parse_group_bytes};
use super::{WitnessCondition, WitnessRule, WitnessRuleAction};
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use serde::de::Error as SerdeDeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};
use std::str::FromStr;

impl WitnessCondition {
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
                "expressions": conditions.iter().map(|c| c.to_json()).collect::<Vec<_>>(),
            }),
            WitnessCondition::Or { conditions } => json!({
                "type": "Or",
                "expressions": conditions.iter().map(|c| c.to_json()).collect::<Vec<_>>(),
            }),
            WitnessCondition::ScriptHash { hash } => json!({
                "type": "ScriptHash",
                "hash": hash.to_string(),
            }),
            WitnessCondition::Group { group } => json!({
                "type": "Group",
                "group": encode_hex(group),
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
                "group": encode_hex(group),
            }),
        }
    }

    pub fn from_json(json: &Value) -> CoreResult<Self> {
        Self::from_json_with_depth(json, Self::MAX_NESTING_DEPTH)
    }

    pub fn from_json_with_depth(json: &Value, max_depth: usize) -> CoreResult<Self> {
        if max_depth == 0 {
            return Err(CoreError::other("Max nesting depth exceeded"));
        }

        let condition_type = json
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::other("Condition type missing"))?;
        match condition_type {
            "Boolean" => {
                let value = json
                    .get("expression")
                    .and_then(Value::as_bool)
                    .or_else(|| json.get("value").and_then(Value::as_bool))
                    .ok_or_else(|| CoreError::other("Boolean condition missing expression"))?;
                Ok(WitnessCondition::Boolean { value })
            }
            "Not" => {
                let expression = json
                    .get("expression")
                    .ok_or_else(|| CoreError::other("Not condition missing expression"))?;
                let inner = WitnessCondition::from_json_with_depth(expression, max_depth - 1)?;
                Ok(WitnessCondition::Not {
                    condition: Box::new(inner),
                })
            }
            "And" => {
                let expressions = json
                    .get("expressions")
                    .and_then(Value::as_array)
                    .ok_or_else(|| CoreError::other("And condition missing expressions"))?;
                if expressions.is_empty() {
                    return Err(CoreError::other(
                        "Composite witness condition requires at least one expression",
                    ));
                }
                if expressions.len() > Self::MAX_SUBITEMS {
                    return Err(CoreError::other("Composite witness condition exceeds max subitems"));
                }
                let mut conditions = Vec::with_capacity(expressions.len());
                for expr in expressions {
                    conditions.push(WitnessCondition::from_json_with_depth(expr, max_depth - 1)?);
                }
                Ok(WitnessCondition::And { conditions })
            }
            "Or" => {
                let expressions = json
                    .get("expressions")
                    .and_then(Value::as_array)
                    .ok_or_else(|| CoreError::other("Or condition missing expressions"))?;
                if expressions.is_empty() {
                    return Err(CoreError::other(
                        "Composite witness condition requires at least one expression",
                    ));
                }
                if expressions.len() > Self::MAX_SUBITEMS {
                    return Err(CoreError::other("Composite witness condition exceeds max subitems"));
                }
                let mut conditions = Vec::with_capacity(expressions.len());
                for expr in expressions {
                    conditions.push(WitnessCondition::from_json_with_depth(expr, max_depth - 1)?);
                }
                Ok(WitnessCondition::Or { conditions })
            }
            "ScriptHash" => {
                let hash_str = json
                    .get("hash")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CoreError::other("ScriptHash condition missing hash"))?;
                let hash =
                    UInt160::from_str(hash_str).map_err(|e| CoreError::other(format!("Invalid script hash: {e}")))?;
                Ok(WitnessCondition::ScriptHash { hash })
            }
            "Group" => {
                let group_str = json
                    .get("group")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CoreError::other("Group condition missing group"))?;
                let group = parse_group_bytes(group_str)?;
                Ok(WitnessCondition::Group { group })
            }
            "CalledByEntry" => Ok(WitnessCondition::CalledByEntry),
            "CalledByContract" => {
                let hash_str = json
                    .get("hash")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CoreError::other("CalledByContract missing hash"))?;
                let hash =
                    UInt160::from_str(hash_str).map_err(|e| CoreError::other(format!("Invalid script hash: {e}")))?;
                Ok(WitnessCondition::CalledByContract { hash })
            }
            "CalledByGroup" => {
                let group_str = json
                    .get("group")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CoreError::other("CalledByGroup missing group"))?;
                let group = parse_group_bytes(group_str)?;
                Ok(WitnessCondition::CalledByGroup { group })
            }
            other => Err(CoreError::other(format!("Unsupported witness condition type: {other}"))),
        }
    }
}

impl WitnessRule {
    pub fn to_json(&self) -> Value {
        json!({
            "action": self.action.to_string(),
            "condition": self.condition.to_json(),
        })
    }

    pub fn from_json(value: &Value) -> CoreResult<Self> {
        Self::from_json_with_depth(value, WitnessCondition::MAX_NESTING_DEPTH)
    }

    pub fn from_json_with_depth(value: &Value, max_depth: usize) -> CoreResult<Self> {
        let action_str = value
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::other("WitnessRule missing action"))?;
        let action: WitnessRuleAction = action_str.parse().map_err(CoreError::other)?;
        let condition_value = value
            .get("condition")
            .ok_or_else(|| CoreError::other("WitnessRule missing condition"))?;
        let condition = WitnessCondition::from_json_with_depth(condition_value, max_depth)?;
        Ok(Self { action, condition })
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
