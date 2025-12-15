//! ContractParameter - matches C# Neo.SmartContract.ContractParameter exactly

use crate::cryptography::ECPoint;
use crate::smart_contract::ContractParameterType;
use crate::{UInt160, UInt256};
use base64::{engine::general_purpose, Engine as _};
use num_bigint::BigInt;
use p256::{
    elliptic_curve::{group::prime::PrimeCurveAffine, sec1::ToEncodedPoint},
    AffinePoint as P256AffinePoint,
};

/// Represents a parameter of a contract method (matches C# ContractParameter)
#[derive(Clone, Debug)]
pub struct ContractParameter {
    /// The type of the parameter
    pub param_type: ContractParameterType,

    /// The value of the parameter
    pub value: ContractParameterValue,
}

/// Possible values for a contract parameter
#[derive(Clone, Debug)]
pub enum ContractParameterValue {
    Any,
    Signature(Vec<u8>),
    Boolean(bool),
    Integer(BigInt),
    Hash160(UInt160),
    Hash256(UInt256),
    ByteArray(Vec<u8>),
    PublicKey(ECPoint),
    String(String),
    Array(Vec<ContractParameter>),
    Map(Vec<(ContractParameter, ContractParameter)>),
    InteropInterface,
    Void,
}

impl ContractParameter {
    /// Initializes a new instance
    pub fn new(param_type: ContractParameterType) -> Self {
        let value = match param_type {
            ContractParameterType::Any => ContractParameterValue::Any,
            ContractParameterType::Signature => ContractParameterValue::Signature(vec![0u8; 64]),
            ContractParameterType::Boolean => ContractParameterValue::Boolean(false),
            ContractParameterType::Integer => ContractParameterValue::Integer(BigInt::from(0)),
            ContractParameterType::Hash160 => ContractParameterValue::Hash160(UInt160::default()),
            ContractParameterType::Hash256 => ContractParameterValue::Hash256(UInt256::default()),
            ContractParameterType::ByteArray => ContractParameterValue::ByteArray(Vec::new()),
            ContractParameterType::PublicKey => {
                ContractParameterValue::PublicKey(Self::default_public_key())
            }
            ContractParameterType::String => ContractParameterValue::String(String::new()),
            ContractParameterType::Array => ContractParameterValue::Array(Vec::new()),
            ContractParameterType::Map => ContractParameterValue::Map(Vec::new()),
            ContractParameterType::InteropInterface => ContractParameterValue::InteropInterface,
            ContractParameterType::Void => ContractParameterValue::Void,
        };

        Self { param_type, value }
    }

    /// Creates with a specific value
    pub fn with_value(param_type: ContractParameterType, value: ContractParameterValue) -> Self {
        Self { param_type, value }
    }

    /// Sets the value from string
    pub fn set_value(&mut self, text: &str) -> Result<(), String> {
        self.value = match self.param_type {
            ContractParameterType::Signature => {
                let bytes = general_purpose::STANDARD
                    .decode(text)
                    .map_err(|e| e.to_string())?;
                if bytes.len() != 64 {
                    return Err("Signature must be 64 bytes".to_string());
                }
                ContractParameterValue::Signature(bytes)
            }
            ContractParameterType::Boolean => {
                let val = text.parse::<bool>().map_err(|e| e.to_string())?;
                ContractParameterValue::Boolean(val)
            }
            ContractParameterType::Integer => {
                let val = text.parse::<BigInt>().map_err(|e| e.to_string())?;
                ContractParameterValue::Integer(val)
            }
            ContractParameterType::Hash160 => {
                let val = text.parse::<UInt160>().map_err(|e| e.to_string())?;
                ContractParameterValue::Hash160(val)
            }
            ContractParameterType::Hash256 => {
                let val = text.parse::<UInt256>().map_err(|e| e.to_string())?;
                ContractParameterValue::Hash256(val)
            }
            ContractParameterType::ByteArray => {
                let bytes = hex::decode(text).map_err(|e| e.to_string())?;
                ContractParameterValue::ByteArray(bytes)
            }
            ContractParameterType::PublicKey => {
                let bytes = hex::decode(text).map_err(|e| e.to_string())?;
                if bytes.len() != 33 && bytes.len() != 65 {
                    return Err("Invalid public key length".to_string());
                }
                let point = ECPoint::from_bytes(&bytes).map_err(|e| e.to_string())?;
                ContractParameterValue::PublicKey(point)
            }
            ContractParameterType::String => ContractParameterValue::String(text.to_string()),
            _ => {
                return Err(format!(
                    "Cannot set value from string for type {:?}",
                    self.param_type
                ))
            }
        };

        Ok(())
    }

    fn default_public_key() -> ECPoint {
        let generator = P256AffinePoint::generator().to_encoded_point(true);
        ECPoint::from_bytes(generator.as_bytes())
            .expect("secp256r1 generator encoding must be valid")
    }

    /// Converts to JSON representation
    pub fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "type".to_string(),
            serde_json::Value::String(format!("{:?}", self.param_type)),
        );

        let value = match &self.value {
            ContractParameterValue::Any => serde_json::Value::Null,
            ContractParameterValue::Signature(bytes) => {
                serde_json::Value::String(general_purpose::STANDARD.encode(bytes))
            }
            ContractParameterValue::Boolean(b) => serde_json::Value::Bool(*b),
            ContractParameterValue::Integer(i) => serde_json::Value::String(i.to_string()),
            ContractParameterValue::Hash160(h) => serde_json::Value::String(h.to_string()),
            ContractParameterValue::Hash256(h) => serde_json::Value::String(h.to_string()),
            ContractParameterValue::ByteArray(bytes) => {
                serde_json::Value::String(general_purpose::STANDARD.encode(bytes))
            }
            ContractParameterValue::PublicKey(key) => {
                serde_json::Value::String(hex::encode(key.encoded()))
            }
            ContractParameterValue::String(s) => serde_json::Value::String(s.clone()),
            ContractParameterValue::Array(arr) => {
                let items: Vec<serde_json::Value> = arr.iter().map(|p| p.to_json()).collect();
                serde_json::Value::Array(items)
            }
            ContractParameterValue::Map(map) => {
                let items: Vec<serde_json::Value> = map
                    .iter()
                    .map(|(k, v)| {
                        let mut pair = serde_json::Map::new();
                        pair.insert("key".to_string(), k.to_json());
                        pair.insert("value".to_string(), v.to_json());
                        serde_json::Value::Object(pair)
                    })
                    .collect();
                serde_json::Value::Array(items)
            }
            _ => serde_json::Value::Null,
        };

        if !matches!(value, serde_json::Value::Null) {
            obj.insert("value".to_string(), value);
        }

        serde_json::Value::Object(obj)
    }

    /// Creates from JSON representation
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let obj = json.as_object().ok_or("Expected JSON object")?;

        let type_str = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Missing type field")?;

        let param_type = match type_str {
            "Any" => ContractParameterType::Any,
            "Signature" => ContractParameterType::Signature,
            "Boolean" => ContractParameterType::Boolean,
            "Integer" => ContractParameterType::Integer,
            "Hash160" => ContractParameterType::Hash160,
            "Hash256" => ContractParameterType::Hash256,
            "ByteArray" => ContractParameterType::ByteArray,
            "PublicKey" => ContractParameterType::PublicKey,
            "String" => ContractParameterType::String,
            "Array" => ContractParameterType::Array,
            "Map" => ContractParameterType::Map,
            "InteropInterface" => ContractParameterType::InteropInterface,
            "Void" => ContractParameterType::Void,
            _ => return Err(format!("Unknown parameter type: {}", type_str)),
        };

        let mut param = Self::new(param_type);

        if let Some(value_json) = obj.get("value") {
            if !value_json.is_null() {
                match param_type {
                    ContractParameterType::String => {
                        if let Some(s) = value_json.as_str() {
                            param.set_value(s)?;
                        }
                    }
                    ContractParameterType::Boolean => {
                        if let Some(b) = value_json.as_bool() {
                            param.value = ContractParameterValue::Boolean(b);
                        }
                    }
                    ContractParameterType::Integer => {
                        if let Some(s) = value_json.as_str() {
                            param.set_value(s)?;
                        }
                    }
                    ContractParameterType::Array => {
                        if let Some(arr) = value_json.as_array() {
                            let items: Result<Vec<_>, _> =
                                arr.iter().map(Self::from_json).collect();
                            param.value = ContractParameterValue::Array(items?);
                        }
                    }
                    _ => {
                        if let Some(s) = value_json.as_str() {
                            param.set_value(s)?;
                        }
                    }
                }
            }
        }

        Ok(param)
    }
}
