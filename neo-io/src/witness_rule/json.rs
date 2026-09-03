use super::helpers::{encode_hex, parse_group_bytes};
use super::{WitnessCondition, WitnessRule, WitnessRuleAction};
use neo_primitives::UInt160;
use serde::de::Error as SerdeDeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};
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

    pub fn from_json(json: &Value) -> Result<Self, String> {
        Self::from_json_with_depth(json, Self::MAX_NESTING_DEPTH)
    }

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
            "And" => {
                let expressions = json
                    .get("expressions")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "And condition missing expressions".to_string())?;
                if expressions.is_empty() {
                    return Err(
                        "Composite witness condition requires at least one expression".to_string(),
                    );
                }
                if expressions.len() > Self::MAX_SUBITEMS {
                    return Err("Composite witness condition exceeds max subitems".to_string());
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
                    .ok_or_else(|| "Or condition missing expressions".to_string())?;
                if expressions.is_empty() {
                    return Err(
                        "Composite witness condition requires at least one expression".to_string(),
                    );
                }
                if expressions.len() > Self::MAX_SUBITEMS {
                    return Err("Composite witness condition exceeds max subitems".to_string());
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
    pub fn to_json(&self) -> Value {
        json!({
            "action": self.action.to_string(),
            "condition": self.condition.to_json(),
        })
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        Self::from_json_with_depth(value, WitnessCondition::MAX_NESTING_DEPTH)
    }

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
