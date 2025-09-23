//! Witness condition JSON converter matching the C# REST server implementation.

use std::{str::FromStr, sync::Arc};

use hex::decode;
use neo_core::{UInt160, WitnessCondition, WitnessConditionType};
// Removed neo_cryptography dependency - using external crypto crates directly
use serde_json::{Map, Value};

use crate::rest_server::rest_server_utility_j_tokens::witness_condition_to_json;

/// Converts between `WitnessCondition` structures and the REST server JSON representation.
#[derive(Debug, Default, Clone)]
pub struct WitnessConditionJsonConverter;

impl WitnessConditionJsonConverter {
    /// Returns a reference-counted converter instance.
    pub fn new_arc() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Serialises a witness condition to JSON using the same projection as the C# node.
    pub fn to_json(&self, condition: &WitnessCondition) -> Value {
        witness_condition_to_json(condition)
    }

    /// Deserialises a witness condition from JSON. Returns `None` if the payload is invalid.
    pub fn from_json(&self, value: &Value) -> Option<WitnessCondition> {
        from_json_internal(value, WitnessCondition::MAX_NESTING_DEPTH)
    }
}

fn from_json_internal(value: &Value, remaining_depth: usize) -> Option<WitnessCondition> {
    if remaining_depth == 0 {
        return None;
    }

    let obj = value.as_object()?;
    let type_value = get_property_case_insensitive(obj, "type")?;
    let type_str = type_value.as_str()?;
    let condition_type = parse_condition_type(type_str)?;

    let condition = match condition_type {
        WitnessConditionType::Boolean => {
            let expression = get_property_case_insensitive(obj, "expression")?.as_bool()?;
            WitnessCondition::Boolean { value: expression }
        }
        WitnessConditionType::Not => {
            let expr_value = get_property_case_insensitive(obj, "expression")?;
            let inner = from_json_internal(expr_value, remaining_depth - 1)?;
            WitnessCondition::Not {
                condition: Box::new(inner),
            }
        }
        WitnessConditionType::And | WitnessConditionType::Or => {
            let expressions = get_property_case_insensitive(obj, "expressions")?.as_array()?;
            if expressions.is_empty()
                || expressions.len() > WitnessCondition::MAX_SUBITEMS
            {
                return None;
            }

            let mut parsed = Vec::with_capacity(expressions.len());
            for expr in expressions {
                parsed.push(from_json_internal(expr, remaining_depth - 1)?);
            }

            if condition_type == WitnessConditionType::And {
                WitnessCondition::And { conditions: parsed }
            } else {
                WitnessCondition::Or { conditions: parsed }
            }
        }
        WitnessConditionType::ScriptHash => {
            let hash_value = get_property_case_insensitive(obj, "hash")?.as_str()?;
            let hash = UInt160::from_str(hash_value).ok()?;
            WitnessCondition::ScriptHash { hash }
        }
        WitnessConditionType::Group => {
            let group_value = get_property_case_insensitive(obj, "group")?.as_str()?;
            let group = parse_group_bytes(group_value)?;
            WitnessCondition::Group { group }
        }
        WitnessConditionType::CalledByEntry => WitnessCondition::CalledByEntry,
        WitnessConditionType::CalledByContract => {
            let hash_value = get_property_case_insensitive(obj, "hash")?.as_str()?;
            let hash = UInt160::from_str(hash_value).ok()?;
            WitnessCondition::CalledByContract { hash }
        }
        WitnessConditionType::CalledByGroup => {
            let group_value = get_property_case_insensitive(obj, "group")?.as_str()?;
            let group = parse_group_bytes(group_value)?;
            WitnessCondition::CalledByGroup { group }
        }
    };

    if condition.is_valid(remaining_depth) {
        Some(condition)
    } else {
        None
    }
}

fn get_property_case_insensitive<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.iter().find_map(|(key, value)| {
        if key.eq_ignore_ascii_case(name) {
            Some(value)
        } else {
            None
        }
    })
}

fn parse_condition_type(name: &str) -> Option<WitnessConditionType> {
    match name.trim() {
        value if value.eq_ignore_ascii_case("Boolean") => Some(WitnessConditionType::Boolean),
        value if value.eq_ignore_ascii_case("Not") => Some(WitnessConditionType::Not),
        value if value.eq_ignore_ascii_case("And") => Some(WitnessConditionType::And),
        value if value.eq_ignore_ascii_case("Or") => Some(WitnessConditionType::Or),
        value if value.eq_ignore_ascii_case("ScriptHash") => Some(WitnessConditionType::ScriptHash),
        value if value.eq_ignore_ascii_case("Group") => Some(WitnessConditionType::Group),
        value if value.eq_ignore_ascii_case("CalledByEntry") => Some(WitnessConditionType::CalledByEntry),
        value if value.eq_ignore_ascii_case("CalledByContract") => {
            Some(WitnessConditionType::CalledByContract)
        }
        value if value.eq_ignore_ascii_case("CalledByGroup") => {
            Some(WitnessConditionType::CalledByGroup)
        }
        _ => None,
    }
}

fn parse_group_bytes(encoded: &str) -> Option<Vec<u8>> {
    let trimmed = encoded.trim().trim_start_matches("0x");
    let raw = decode(trimmed).ok()?;
    let point = ECPoint::decode(&raw, ECCurve::secp256r1()).ok()?;
    point.encode_point(true).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_boolean_condition() {
        let converter = WitnessConditionJsonConverter::default();
        let condition = WitnessCondition::Boolean { value: true };
        let json = converter.to_json(&condition);
        let parsed = converter.from_json(&json).expect("boolean condition should parse");
        assert_eq!(condition, parsed);
    }
}
