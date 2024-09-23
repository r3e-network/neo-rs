use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use base64::{decode as base64_decode, encode as base64_encode};
use hex::{decode as hex_decode, encode as hex_encode};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::crypto::keys::PublicKey;
use crate::types::{Hash160, Hash256};
use crate::vm::stackitem::Item;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    #[serde(rename = "type")]
    pub param_type: ParamType,
    pub value: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterPair {
    pub key: Parameter,
    pub value: Parameter,
}

pub trait Convertible {
    fn to_sc_parameter(&self) -> Result<Parameter, Box<dyn std::error::Error>>;
}

impl Parameter {
    pub fn new(param_type: ParamType) -> Self {
        Self {
            param_type,
            value: None,
        }
    }

    pub fn from_string(input: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut chars = input.chars().peekable();
        let mut type_str = String::new();
        let mut value_str = String::new();
        let mut escaped = false;
        let mut had_type = false;

        while let Some(c) = chars.next() {
            if c == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if c == ':' && !escaped && !had_type {
                had_type = true;
                continue;
            }
            if had_type {
                value_str.push(c);
            } else {
                type_str.push(c);
            }
            escaped = false;
        }

        let param_type = if had_type {
            ParamType::from_str(&type_str)?
        } else {
            infer_param_type(&value_str)
        };

        // Handle unsupported types
        match param_type {
            ParamType::Array | ParamType::Map | ParamType::InteropInterface | ParamType::Void => {
                return Err(format!("Unsupported type: {}", param_type).into());
            }
            _ => {}
        }

        let value = if param_type == ParamType::ByteArray && type_str == "file" {
            let bytes = fs::read(value_str)?;
            Some(Value::String(base64_encode(bytes)))
        } else {
            Some(adjust_val_to_type(param_type, &value_str)?)
        };

        Ok(Self { param_type, value })
    }

    pub fn from_value(value: Value) -> Result<Self, Box<dyn std::error::Error>> {
        let (param_type, adjusted_value) = match value {
            Value::String(_) => (ParamType::String, value),
            Value::Bool(_) => (ParamType::Bool, value),
            Value::Number(n) => {
                let big_int = BigInt::from_str(&n.to_string())?;
                (ParamType::Integer, json!(big_int.to_string()))
            }
            Value::Array(arr) => {
                let params: Result<Vec<Parameter>, _> = arr
                    .into_iter()
                    .map(|v| Parameter::from_value(v))
                    .collect();
                (ParamType::Array, json!(params?))
            }
            Value::Object(map) => {
                let pairs: Result<Vec<ParameterPair>, _> = map
                    .into_iter()
                    .map(|(k, v)| {
                        Ok(ParameterPair {
                            key: Parameter::from_value(Value::String(k))?,
                            value: Parameter::from_value(v)?,
                        })
                    })
                    .collect();
                (ParamType::Map, json!(pairs?))
            }
            Value::Null => (ParamType::Any, value),
        };

        Ok(Self {
            param_type,
            value: Some(adjusted_value),
        })
    }

    pub fn to_stack_item(&self) -> Result<Item, Box<dyn std::error::Error>> {
        // Implementation depends on the Item type from your VM
        unimplemented!("to_stack_item not implemented")
    }
}

fn infer_param_type(value: &str) -> ParamType {
    // Simple inference logic, can be expanded
    if value == "true" || value == "false" {
        ParamType::Bool
    } else if value.starts_with("0x") {
        ParamType::ByteArray
    } else if value.parse::<i64>().is_ok() {
        ParamType::Integer
    } else {
        ParamType::String
    }
}

fn adjust_val_to_type(param_type: ParamType, value: &str) -> Result<Value, Box<dyn std::error::Error>> {
    match param_type {
        ParamType::String => Ok(Value::String(value.to_string())),
        ParamType::Bool => Ok(Value::Bool(value.parse()?)),
        ParamType::Integer => {
            let big_int = BigInt::from_str(value)?;
            Ok(json!(big_int.to_string()))
        }
        ParamType::ByteArray | ParamType::Signature => {
            let bytes = if value.starts_with("0x") {
                hex_decode(&value[2..])?
            } else {
                value.as_bytes().to_vec()
            };
            Ok(Value::String(base64_encode(bytes)))
        }
        ParamType::PublicKey => {
            let bytes = hex_decode(value)?;
            Ok(Value::String(hex_encode(bytes)))
        }
        ParamType::Hash160 => {
            let hash = Hash160::from_str(value)?;
            Ok(json!(hash))
        }
        ParamType::Hash256 => {
            let hash = Hash256::from_str(value)?;
            Ok(json!(hash))
        }
        _ => Err(format!("Unsupported type: {}", param_type).into()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamType {
    Signature,
    Bool,
    Integer,
    Hash160,
    Hash256,
    ByteArray,
    PublicKey,
    String,
    Array,
    Map,
    InteropInterface,
    Void,
}

impl FromStr for ParamType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "signature" => Ok(ParamType::Signature),
            "bool" => Ok(ParamType::Bool),
            "integer" | "int" => Ok(ParamType::Integer),
            "hash160" => Ok(ParamType::Hash160),
            "hash256" => Ok(ParamType::Hash256),
            "bytearray" | "bytes" | "filebytes" => Ok(ParamType::ByteArray),
            "publickey" | "key" => Ok(ParamType::PublicKey),
            "string" => Ok(ParamType::String),
            "array" => Ok(ParamType::Array),
            "map" => Ok(ParamType::Map),
            "interopinterface" => Ok(ParamType::InteropInterface),
            "void" => Ok(ParamType::Void),
            _ => Err(()),
        }
    }
}

impl fmt::Display for ParamType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamType::Signature => write!(f, "Signature"),
            ParamType::Bool => write!(f, "Boolean"),
            ParamType::Integer => write!(f, "Integer"),
            ParamType::Hash160 => write!(f, "Hash160"),
            ParamType::Hash256 => write!(f, "Hash256"),
            ParamType::ByteArray => write!(f, "ByteArray"),
            ParamType::PublicKey => write!(f, "PublicKey"),
            ParamType::String => write!(f, "String"),
            ParamType::Array => write!(f, "Array"),
            ParamType::Map => write!(f, "Map"),
            ParamType::InteropInterface => write!(f, "InteropInterface"),
            ParamType::Void => write!(f, "Void"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash160([u8; 20]);

impl Hash160 {
    pub fn from_slice(slice: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if slice.len() != 20 {
            return Err("Invalid Hash160 length".into());
        }
        let mut hash = [0u8; 20];
        hash.copy_from_slice(slice);
        Ok(Hash160(hash))
    }
}

impl FromStr for Hash160 {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex_decode(s)?;
        Self::from_slice(&bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash256([u8; 32]);

impl Hash256 {
    pub fn from_slice(slice: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if slice.len() != 32 {
            return Err("Invalid Hash256 length".into());
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(slice);
        Ok(Hash256(hash))
    }
}

impl FromStr for Hash256 {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex_decode(s)?;
        Self::from_slice(&bytes)
    }
}
