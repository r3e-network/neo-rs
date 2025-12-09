//! ContractParameterType - matches C# Neo.SmartContract.ContractParameterType exactly

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;

/// Represents the type of ContractParameter (matches C# ContractParameterType)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ContractParameterType {
    /// Indicates that the parameter can be of any type
    #[default]
    Any = 0x00,

    /// Indicates that the parameter is of Boolean type
    Boolean = 0x10,

    /// Indicates that the parameter is an integer
    Integer = 0x11,

    /// Indicates that the parameter is a byte array
    ByteArray = 0x12,

    /// Indicates that the parameter is a string
    String = 0x13,

    /// Indicates that the parameter is a 160-bit hash
    Hash160 = 0x14,

    /// Indicates that the parameter is a 256-bit hash
    Hash256 = 0x15,

    /// Indicates that the parameter is a public key
    PublicKey = 0x16,

    /// Indicates that the parameter is a signature
    Signature = 0x17,

    /// Indicates that the parameter is an array
    Array = 0x20,

    /// Indicates that the parameter is a map
    Map = 0x22,

    /// Indicates that the parameter is an interoperable interface
    InteropInterface = 0x30,

    /// It can be only used as the return type of a method, meaning that the method has no return value
    Void = 0xff,
}

impl ContractParameterType {
    /// Returns the canonical manifest name for this parameter type (matches C# enum names).
    pub fn as_str(&self) -> &'static str {
        match self {
            ContractParameterType::Any => "Any",
            ContractParameterType::Boolean => "Boolean",
            ContractParameterType::Integer => "Integer",
            ContractParameterType::ByteArray => "ByteArray",
            ContractParameterType::String => "String",
            ContractParameterType::Hash160 => "Hash160",
            ContractParameterType::Hash256 => "Hash256",
            ContractParameterType::PublicKey => "PublicKey",
            ContractParameterType::Signature => "Signature",
            ContractParameterType::Array => "Array",
            ContractParameterType::Map => "Map",
            ContractParameterType::InteropInterface => "InteropInterface",
            ContractParameterType::Void => "Void",
        }
    }

    /// Parse from string (case-insensitive)
    pub fn from_string(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "any" => Ok(ContractParameterType::Any),
            "boolean" | "bool" => Ok(ContractParameterType::Boolean),
            "integer" | "int" => Ok(ContractParameterType::Integer),
            "bytearray" | "bytes" => Ok(ContractParameterType::ByteArray),
            "string" => Ok(ContractParameterType::String),
            "hash160" => Ok(ContractParameterType::Hash160),
            "hash256" => Ok(ContractParameterType::Hash256),
            "publickey" => Ok(ContractParameterType::PublicKey),
            "signature" => Ok(ContractParameterType::Signature),
            "array" => Ok(ContractParameterType::Array),
            "map" => Ok(ContractParameterType::Map),
            "interopinterface" => Ok(ContractParameterType::InteropInterface),
            "void" => Ok(ContractParameterType::Void),
            _ => Err(format!("unknown contract parameter type: {s}")),
        }
    }

    /// Try to convert from u8 value
    pub fn try_from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(ContractParameterType::Any),
            0x10 => Some(ContractParameterType::Boolean),
            0x11 => Some(ContractParameterType::Integer),
            0x12 => Some(ContractParameterType::ByteArray),
            0x13 => Some(ContractParameterType::String),
            0x14 => Some(ContractParameterType::Hash160),
            0x15 => Some(ContractParameterType::Hash256),
            0x16 => Some(ContractParameterType::PublicKey),
            0x17 => Some(ContractParameterType::Signature),
            0x20 => Some(ContractParameterType::Array),
            0x22 => Some(ContractParameterType::Map),
            0x30 => Some(ContractParameterType::InteropInterface),
            0xff => Some(ContractParameterType::Void),
            _ => None,
        }
    }
}

impl std::fmt::Display for ContractParameterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ContractParameterType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_string(s)
    }
}

impl Serialize for ContractParameterType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ContractParameterType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ContractParameterType::from_string(&value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contract_parameter_type_values() {
        assert_eq!(ContractParameterType::Any as u8, 0x00);
        assert_eq!(ContractParameterType::Boolean as u8, 0x10);
        assert_eq!(ContractParameterType::Integer as u8, 0x11);
        assert_eq!(ContractParameterType::ByteArray as u8, 0x12);
        assert_eq!(ContractParameterType::String as u8, 0x13);
        assert_eq!(ContractParameterType::Hash160 as u8, 0x14);
        assert_eq!(ContractParameterType::Hash256 as u8, 0x15);
        assert_eq!(ContractParameterType::PublicKey as u8, 0x16);
        assert_eq!(ContractParameterType::Signature as u8, 0x17);
        assert_eq!(ContractParameterType::Array as u8, 0x20);
        assert_eq!(ContractParameterType::Map as u8, 0x22);
        assert_eq!(ContractParameterType::InteropInterface as u8, 0x30);
        assert_eq!(ContractParameterType::Void as u8, 0xff);
    }

    #[test]
    fn test_contract_parameter_type_as_str() {
        assert_eq!(ContractParameterType::Any.as_str(), "Any");
        assert_eq!(ContractParameterType::Boolean.as_str(), "Boolean");
        assert_eq!(ContractParameterType::Hash160.as_str(), "Hash160");
    }

    #[test]
    fn test_contract_parameter_type_from_string() {
        assert_eq!(
            ContractParameterType::from_string("Boolean").unwrap(),
            ContractParameterType::Boolean
        );
        assert_eq!(
            ContractParameterType::from_string("bool").unwrap(),
            ContractParameterType::Boolean
        );
        assert!(ContractParameterType::from_string("Invalid").is_err());
    }

    #[test]
    fn test_contract_parameter_type_try_from_u8() {
        assert_eq!(
            ContractParameterType::try_from_u8(0x10),
            Some(ContractParameterType::Boolean)
        );
        assert_eq!(ContractParameterType::try_from_u8(0x99), None);
    }

    #[test]
    fn test_contract_parameter_type_serde() {
        let param_type = ContractParameterType::Hash160;
        let json = serde_json::to_string(&param_type).unwrap();
        assert_eq!(json, "\"Hash160\"");

        let parsed: ContractParameterType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ContractParameterType::Hash160);
    }

    #[test]
    fn test_contract_parameter_type_default() {
        assert_eq!(ContractParameterType::default(), ContractParameterType::Any);
    }
}
