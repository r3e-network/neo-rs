//! `ContractParameterType` - matches C# Neo.SmartContract.ContractParameterType exactly

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;

/// Represents the type of `ContractParameter` (matches C# `ContractParameterType`)
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
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Boolean => "Boolean",
            Self::Integer => "Integer",
            Self::ByteArray => "ByteArray",
            Self::String => "String",
            Self::Hash160 => "Hash160",
            Self::Hash256 => "Hash256",
            Self::PublicKey => "PublicKey",
            Self::Signature => "Signature",
            Self::Array => "Array",
            Self::Map => "Map",
            Self::InteropInterface => "InteropInterface",
            Self::Void => "Void",
        }
    }

    /// Parse from string (case-insensitive)
    ///
    /// # Errors
    ///
    /// Returns `String` error if the input string does not match any known parameter type.
    pub fn from_string(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "any" => Ok(Self::Any),
            "boolean" | "bool" => Ok(Self::Boolean),
            "integer" | "int" => Ok(Self::Integer),
            "bytearray" | "bytes" => Ok(Self::ByteArray),
            "string" => Ok(Self::String),
            "hash160" => Ok(Self::Hash160),
            "hash256" => Ok(Self::Hash256),
            "publickey" => Ok(Self::PublicKey),
            "signature" => Ok(Self::Signature),
            "array" => Ok(Self::Array),
            "map" => Ok(Self::Map),
            "interopinterface" => Ok(Self::InteropInterface),
            "void" => Ok(Self::Void),
            _ => Err(format!("unknown contract parameter type: {s}")),
        }
    }

    /// Try to convert from u8 value
    #[must_use]
    pub const fn try_from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Any),
            0x10 => Some(Self::Boolean),
            0x11 => Some(Self::Integer),
            0x12 => Some(Self::ByteArray),
            0x13 => Some(Self::String),
            0x14 => Some(Self::Hash160),
            0x15 => Some(Self::Hash256),
            0x16 => Some(Self::PublicKey),
            0x17 => Some(Self::Signature),
            0x20 => Some(Self::Array),
            0x22 => Some(Self::Map),
            0x30 => Some(Self::InteropInterface),
            0xff => Some(Self::Void),
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
        Self::from_string(&value).map_err(serde::de::Error::custom)
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
