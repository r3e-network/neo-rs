//! `ContractParameterType` - matches C# Neo.SmartContract.ContractParameterType exactly

use crate::{impl_protocol_enum_from_str, protocol_enum_repr};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

protocol_enum_repr! {
    all;
    /// Represents the type of `ContractParameter` (matches C# `ContractParameterType`)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub ContractParameterType {
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
}

impl ContractParameterType {
    /// Parse from string (case-insensitive)
    ///
    /// # Errors
    ///
    /// Returns `String` error if the input string does not match any known parameter type.
    pub fn from_string(s: &str) -> Result<Self, String> {
        s.parse()
    }

    /// Compatibility alias for callers that still use the older helper name.
    #[must_use]
    pub const fn try_from_u8(value: u8) -> Option<Self> {
        Self::from_byte(value)
    }
}

impl_protocol_enum_from_str! {
    ContractParameterType {
        error = |value: &str| format!("unknown contract parameter type: {value}");
        aliases = [
            "bool" => Boolean,
            "int" => Integer,
            "bytes" => ByteArray,
        ];
    }
}

impl Serialize for ContractParameterType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str((*self).as_str())
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
#[path = "tests/contract_parameter_type.rs"]
mod tests;
