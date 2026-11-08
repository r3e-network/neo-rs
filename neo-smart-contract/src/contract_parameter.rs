//! Contract parameter types for RPC serialization.
//!
//! Implements the full ContractParameter type from C# Neo for contract invocation via RPC.

use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Represents a contract parameter value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ContractParameterValue {
    /// Boolean value
    Boolean(bool),
    /// Integer value (uses string representation in JSON for large numbers)
    Integer(i64),
    /// Byte array value (hex string in JSON)
    ByteArray(Vec<u8>),
    /// String value
    String(String),
    /// Hash160 value (20 bytes, hex string with 0x prefix in JSON)
    Hash160(UInt160),
    /// Hash256 value (32 bytes, hex string with 0x prefix in JSON)
    Hash256(UInt256),
    /// Public key value (33 bytes compressed, hex string in JSON)
    PublicKey(Vec<u8>),
    /// Signature value (64 bytes, hex string in JSON)
    Signature(Vec<u8>),
    /// Array of parameters
    Array(Vec<ContractParameter>),
    /// Map of key-value parameter pairs
    Map(Vec<(ContractParameter, ContractParameter)>),
    /// Void/null type
    Void,
    /// Interop interface (cannot be serialized, only used internally)
    InteropInterface,
    /// Any type (dynamic type)
    Any,
}

/// Represents a contract parameter with a type and value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractParameter {
    /// The parameter value
    #[serde(flatten)]
    pub value: ContractParameterValue,
}

impl ContractParameter {
    /// Creates a new contract parameter.
    pub fn new(value: ContractParameterValue) -> Self {
        Self { value }
    }

    /// Creates a contract parameter from JSON.
    ///
    /// Expected JSON format:
    /// ```json
    /// {
    ///   "type": "Boolean",
    ///   "value": true
    /// }
    /// ```
    ///
    /// Supported types: Boolean, Integer, ByteArray, String, Hash160, Hash256,
    /// PublicKey, Signature, Array, Map, Void, Any
    pub fn from_json(json: &JsonValue) -> Result<Self, String> {
        let type_str = json["type"]
            .as_str()
            .ok_or_else(|| "Missing 'type' field".to_string())?;

        let value_json = &json["value"];

        let value = match type_str {
            "Boolean" => {
                let v = value_json
                    .as_bool()
                    .ok_or_else(|| "Invalid boolean value".to_string())?;
                ContractParameterValue::Boolean(v)
            }
            "Integer" => {
                // Accept both number and string representations
                let v = if let Some(n) = value_json.as_i64() {
                    n
                } else if let Some(s) = value_json.as_str() {
                    s.parse::<i64>()
                        .map_err(|_| "Invalid integer string".to_string())?
                } else {
                    return Err("Invalid integer value".to_string());
                };
                ContractParameterValue::Integer(v)
            }
            "ByteArray" => {
                let hex_str = value_json
                    .as_str()
                    .ok_or_else(|| "Invalid byte array, expected hex string".to_string())?;
                let bytes = hex::decode(hex_str.trim_start_matches("0x"))
                    .map_err(|_| "Invalid hex string".to_string())?;
                ContractParameterValue::ByteArray(bytes)
            }
            "String" => {
                let v = value_json
                    .as_str()
                    .ok_or_else(|| "Invalid string value".to_string())?
                    .to_string();
                ContractParameterValue::String(v)
            }
            "Hash160" => {
                let hex_str = value_json
                    .as_str()
                    .ok_or_else(|| "Invalid Hash160, expected hex string".to_string())?;
                let hash = UInt160::from_hex(hex_str.trim_start_matches("0x"))
                    .map_err(|e| format!("Invalid Hash160: {}", e))?;
                ContractParameterValue::Hash160(hash)
            }
            "Hash256" => {
                let hex_str = value_json
                    .as_str()
                    .ok_or_else(|| "Invalid Hash256, expected hex string".to_string())?;
                let hash = UInt256::from_hex(hex_str.trim_start_matches("0x"))
                    .map_err(|e| format!("Invalid Hash256: {}", e))?;
                ContractParameterValue::Hash256(hash)
            }
            "PublicKey" => {
                let hex_str = value_json
                    .as_str()
                    .ok_or_else(|| "Invalid PublicKey, expected hex string".to_string())?;
                let bytes = hex::decode(hex_str.trim_start_matches("0x"))
                    .map_err(|_| "Invalid hex string for PublicKey".to_string())?;
                if bytes.len() != 33 {
                    return Err(format!("PublicKey must be 33 bytes, got {}", bytes.len()));
                }
                ContractParameterValue::PublicKey(bytes)
            }
            "Signature" => {
                let hex_str = value_json
                    .as_str()
                    .ok_or_else(|| "Invalid Signature, expected hex string".to_string())?;
                let bytes = hex::decode(hex_str.trim_start_matches("0x"))
                    .map_err(|_| "Invalid hex string for Signature".to_string())?;
                if bytes.len() != 64 {
                    return Err(format!("Signature must be 64 bytes, got {}", bytes.len()));
                }
                ContractParameterValue::Signature(bytes)
            }
            "Array" => {
                let arr = value_json
                    .as_array()
                    .ok_or_else(|| "Invalid Array, expected JSON array".to_string())?;
                let params: Result<Vec<_>, _> = arr
                    .iter()
                    .map(Self::from_json)
                    .collect();
                ContractParameterValue::Array(params?)
            }
            "Map" => {
                let arr = value_json
                    .as_array()
                    .ok_or_else(|| "Invalid Map, expected JSON array of [key, value] pairs".to_string())?;
                let pairs: Result<Vec<_>, _> = arr
                    .iter()
                    .map(|pair| {
                        let pair_arr = pair
                            .as_array()
                            .ok_or_else(|| "Map entry must be [key, value] array".to_string())?;
                        if pair_arr.len() != 2 {
                            return Err("Map entry must have exactly 2 elements".to_string());
                        }
                        let key = Self::from_json(&pair_arr[0])?;
                        let value = Self::from_json(&pair_arr[1])?;
                        Ok((key, value))
                    })
                    .collect();
                ContractParameterValue::Map(pairs?)
            }
            "Void" => ContractParameterValue::Void,
            "Any" => ContractParameterValue::Any,
            "InteropInterface" => ContractParameterValue::InteropInterface,
            _ => return Err(format!("Unknown contract parameter type: {}", type_str)),
        };

        Ok(Self::new(value))
    }

    /// Converts the contract parameter to JSON.
    pub fn to_json(&self) -> JsonValue {
        match &self.value {
            ContractParameterValue::Boolean(v) => serde_json::json!({
                "type": "Boolean",
                "value": v
            }),
            ContractParameterValue::Integer(v) => serde_json::json!({
                "type": "Integer",
                "value": v.to_string()
            }),
            ContractParameterValue::ByteArray(v) => serde_json::json!({
                "type": "ByteArray",
                "value": format!("0x{}", hex::encode(v))
            }),
            ContractParameterValue::String(v) => serde_json::json!({
                "type": "String",
                "value": v
            }),
            ContractParameterValue::Hash160(v) => serde_json::json!({
                "type": "Hash160",
                "value": format!("0x{}", v)
            }),
            ContractParameterValue::Hash256(v) => serde_json::json!({
                "type": "Hash256",
                "value": format!("0x{}", v)
            }),
            ContractParameterValue::PublicKey(v) => serde_json::json!({
                "type": "PublicKey",
                "value": format!("0x{}", hex::encode(v))
            }),
            ContractParameterValue::Signature(v) => serde_json::json!({
                "type": "Signature",
                "value": format!("0x{}", hex::encode(v))
            }),
            ContractParameterValue::Array(v) => serde_json::json!({
                "type": "Array",
                "value": v.iter().map(|p| p.to_json()).collect::<Vec<_>>()
            }),
            ContractParameterValue::Map(v) => serde_json::json!({
                "type": "Map",
                "value": v.iter().map(|(k, val)| vec![k.to_json(), val.to_json()]).collect::<Vec<_>>()
            }),
            ContractParameterValue::Void => serde_json::json!({
                "type": "Void"
            }),
            ContractParameterValue::InteropInterface => serde_json::json!({
                "type": "InteropInterface"
            }),
            ContractParameterValue::Any => serde_json::json!({
                "type": "Any"
            }),
        }
    }

    /// Creates a boolean parameter.
    pub fn boolean(value: bool) -> Self {
        Self::new(ContractParameterValue::Boolean(value))
    }

    /// Creates an integer parameter.
    pub fn integer(value: i64) -> Self {
        Self::new(ContractParameterValue::Integer(value))
    }

    /// Creates a byte array parameter.
    pub fn byte_array(value: Vec<u8>) -> Self {
        Self::new(ContractParameterValue::ByteArray(value))
    }

    /// Creates a string parameter.
    pub fn string(value: String) -> Self {
        Self::new(ContractParameterValue::String(value))
    }

    /// Creates a hash160 parameter.
    pub fn hash160(value: UInt160) -> Self {
        Self::new(ContractParameterValue::Hash160(value))
    }

    /// Creates a hash256 parameter.
    pub fn hash256(value: UInt256) -> Self {
        Self::new(ContractParameterValue::Hash256(value))
    }

    /// Creates an array parameter.
    pub fn array(value: Vec<ContractParameter>) -> Self {
        Self::new(ContractParameterValue::Array(value))
    }

    /// Creates a map parameter.
    pub fn map(value: Vec<(ContractParameter, ContractParameter)>) -> Self {
        Self::new(ContractParameterValue::Map(value))
    }

    /// Creates a void parameter.
    pub fn void() -> Self {
        Self::new(ContractParameterValue::Void)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boolean_from_json() {
        let json = serde_json::json!({"type": "Boolean", "value": true});
        let param = ContractParameter::from_json(&json).unwrap();
        matches!(param.value, ContractParameterValue::Boolean(true));
    }

    #[test]
    fn test_integer_from_json() {
        let json = serde_json::json!({"type": "Integer", "value": "12345"});
        let param = ContractParameter::from_json(&json).unwrap();
        matches!(param.value, ContractParameterValue::Integer(12345));
    }

    #[test]
    fn test_byte_array_from_json() {
        let json = serde_json::json!({"type": "ByteArray", "value": "0x48656c6c6f"});
        let param = ContractParameter::from_json(&json).unwrap();
        if let ContractParameterValue::ByteArray(bytes) = param.value {
            assert_eq!(bytes, b"Hello");
        } else {
            panic!("Expected ByteArray");
        }
    }

    #[test]
    fn test_array_from_json() {
        let json = serde_json::json!({
            "type": "Array",
            "value": [
                {"type": "Integer", "value": "1"},
                {"type": "String", "value": "test"}
            ]
        });
        let param = ContractParameter::from_json(&json).unwrap();
        if let ContractParameterValue::Array(arr) = param.value {
            assert_eq!(arr.len(), 2);
        } else {
            panic!("Expected Array");
        }
    }
}

