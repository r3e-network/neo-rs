use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::fmt;
use NeoRust::crypto::ECPoint;
use num_bigint::BigInt;
use neo_json::jtoken::JToken;
use crate::cryptography::ECCurve;
use crate::neo_contract::contract_parameter_type::ContractParameterType;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

/// Represents a parameter of a smart contract method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractParameter {
    /// The type of the parameter.
    pub param_type: ContractParameterType,

    /// The value of the parameter.
    pub value: Option<ContractParameterValue>,
}

/// Represents the possible values of smart contract parameters.
#[derive(Debug, Clone,Eq, PartialEq)]
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
    Map(HashMap<ContractParameter, ContractParameter>),
}

impl ContractParameter {
    /// Creates a new ContractParameter with the specified type.
    pub fn new(param_type: ContractParameterType) -> Self {
        let value = match param_type {
            ContractParameterType::Any => None,
            ContractParameterType::Signature => Some(ContractParameterValue::Signature(vec![0; 64])),
            ContractParameterType::Boolean => Some(ContractParameterValue::Boolean(false)),
            ContractParameterType::Integer => Some(ContractParameterValue::Integer(BigInt::from(0))),
            ContractParameterType::Hash160 => Some(ContractParameterValue::Hash160(UInt160::default())),
            ContractParameterType::Hash256 => Some(ContractParameterValue::Hash256(UInt256::default())),
            ContractParameterType::ByteArray => Some(ContractParameterValue::ByteArray(Vec::new())),
            ContractParameterType::PublicKey => Some(ContractParameterValue::PublicKey(ECCurve::Secp256r1.g())),
            ContractParameterType::String => Some(ContractParameterValue::String(String::new())),
            ContractParameterType::Array => Some(ContractParameterValue::Array(Vec::new())),
            ContractParameterType::Map => Some(ContractParameterValue::Map(HashMap::new())),
        };

        ContractParameter {
            param_type,
            value,
        }
    }

    /// Converts the parameter from a JSON object.
    pub fn from_json(json: &JToken) -> Result<Self, String> {
        let type_str = json.get("type")
            .and_then(|v| v.as_str())
            .ok_or("Missing or invalid 'type' field")?;

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
            _ => return Err(format!("Invalid parameter type: {}", type_str)),
        };

        let value = json.get("value").ok_or("Missing 'value' field")?;
        let param_value = match param_type {
            ContractParameterType::Any => None,
            ContractParameterType::Signature => Some(ContractParameterValue::Signature(hex::decode(value.as_str().ok_or("Invalid signature value")?).map_err(|e| e.to_string())?)),
            ContractParameterType::Boolean => Some(ContractParameterValue::Boolean(value.as_bool().ok_or("Invalid boolean value")?)),
            ContractParameterType::Integer => Some(ContractParameterValue::Integer(BigInt::parse_bytes(value.as_str().ok_or("Invalid integer value")?.as_bytes(), 10).ok_or("Failed to parse integer")?)),
            ContractParameterType::Hash160 => Some(ContractParameterValue::Hash160(UInt160::try_from(value.as_str().ok_or("Invalid Hash160 value")?).map_err(|e| e.to_string())?)),
            ContractParameterType::Hash256 => Some(ContractParameterValue::Hash256(UInt256::try_from(value.as_str().ok_or("Invalid Hash256 value")?).map_err(|e| e.to_string())?)),
            ContractParameterType::ByteArray => Some(ContractParameterValue::ByteArray(hex::decode(value.as_str().ok_or("Invalid ByteArray value")?).map_err(|e| e.to_string())?)),
            ContractParameterType::PublicKey => Some(ContractParameterValue::PublicKey(ECPoint::try_from(value.as_str().ok_or("Invalid PublicKey value")?).map_err(|e| e.to_string())?)),
            ContractParameterType::String => Some(ContractParameterValue::String(value.as_str().ok_or("Invalid string value")?.to_string())),
            ContractParameterType::Array => {
                let array = value.as_array().ok_or("Invalid array value")?;
                let params = array.iter().map(|item| ContractParameter::from_json(item.as_object().ok_or("Invalid array item")?)).collect::<Result<Vec<_>, _>>()?;
                Some(ContractParameterValue::Array(params))
            },
            ContractParameterType::Map => {
                let map = value.as_object().ok_or("Invalid map value")?;
                let params = map.iter().map(|(k, v)| {
                    Ok((
                        ContractParameter::from_json(k.as_object().ok_or("Invalid map key")?)?,
                        ContractParameter::from_json(v.as_object().ok_or("Invalid map value")?)?,
                    ))
                }).collect::<Result<HashMap<_, _>, String>>()?;
                Some(ContractParameterValue::Map(params))
            },
        };

        Ok(ContractParameter {
            param_type,
            value: param_value,
        })
    }

    /// Sets the value of the parameter from a string.
    pub fn set_value(&mut self, text: &str) -> Result<(), String> {
        self.value = match self.param_type {
            ContractParameterType::Any => None,
            ContractParameterType::Signature => Some(ContractParameterValue::Signature(hex::decode(text).map_err(|e| e.to_string())?)),
            ContractParameterType::Boolean => Some(ContractParameterValue::Boolean(text.parse().map_err(|e| e.to_string())?)),
            ContractParameterType::Integer => Some(ContractParameterValue::Integer(BigInt::parse_bytes(text.as_bytes(), 10).ok_or("Failed to parse integer")?)),
            ContractParameterType::Hash160 => Some(ContractParameterValue::Hash160(UInt160::try_from(text).map_err(|e| e.to_string())?)),
            ContractParameterType::Hash256 => Some(ContractParameterValue::Hash256(UInt256::try_from(text).map_err(|e| e.to_string())?)),
            ContractParameterType::ByteArray => Some(ContractParameterValue::ByteArray(hex::decode(text).map_err(|e| e.to_string())?)),
            ContractParameterType::PublicKey => Some(ContractParameterValue::PublicKey(ECPoint::try_from(text).map_err(|e| e.to_string())?)),
            ContractParameterType::String => Some(ContractParameterValue::String(text.to_string())),
            ContractParameterType::Array | ContractParameterType::Map => return Err("Cannot set Array or Map from string".to_string()),
        };
        Ok(())
    }

    /// Converts the parameter to a JSON object.
    pub fn to_json(&self) -> JToken {
        let mut json = JObject::new();
        json.insert("type", JValue::from(format!("{:?}", self.param_type)));
        
        if let Some(value) = &self.value {
            let value_json = match value {
                ContractParameterValue::Any => JValue::Null,
                ContractParameterValue::Signature(sig) => JValue::from(hex::encode(sig)),
                ContractParameterValue::Boolean(b) => JValue::from(*b),
                ContractParameterValue::Integer(i) => JValue::from(i.to_string()),
                ContractParameterValue::Hash160(h) => JValue::from(h.to_string()),
                ContractParameterValue::Hash256(h) => JValue::from(h.to_string()),
                ContractParameterValue::ByteArray(b) => JValue::from(hex::encode(b)),
                ContractParameterValue::PublicKey(pk) => JValue::from(pk.to_string()),
                ContractParameterValue::String(s) => JValue::from(s.clone()),
                ContractParameterValue::Array(arr) => JValue::from(arr.iter().map(|p| p.to_json()).collect::<Vec<_>>()),
                ContractParameterValue::Map(map) => {
                    let mut map_json = JObject::new();
                    for (k, v) in map {
                        map_json.insert(k.to_string(), v.to_json());
                    }
                    JValue::from(map_json)
                },
            };
            json.insert("value", value_json);
        }

        json
    }
}

impl fmt::Display for ContractParameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContractParameter {{ type: {:?}, value: ", self.param_type)?;
        match &self.value {
            Some(value) => match value {
                ContractParameterValue::Any => write!(f, "Any"),
                ContractParameterValue::Signature(sig) => write!(f, "Signature({})", hex::encode(sig)),
                ContractParameterValue::Boolean(b) => write!(f, "Boolean({})", b),
                ContractParameterValue::Integer(i) => write!(f, "Integer({})", i),
                ContractParameterValue::Hash160(h) => write!(f, "Hash160({})", h),
                ContractParameterValue::Hash256(h) => write!(f, "Hash256({})", h),
                ContractParameterValue::ByteArray(b) => write!(f, "ByteArray({})", hex::encode(b)),
                ContractParameterValue::PublicKey(pk) => write!(f, "PublicKey({})", pk),
                ContractParameterValue::String(s) => write!(f, "String({})", s),
                ContractParameterValue::Array(arr) => write!(f, "Array({:?})", arr),
                ContractParameterValue::Map(map) => write!(f, "Map({:?})", map),
            },
            None => write!(f, "None"),
        }?;
        write!(f, " }}")
    }
}
