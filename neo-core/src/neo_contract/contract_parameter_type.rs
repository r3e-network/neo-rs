use std::convert::TryFrom;
use serde::{Serialize, Deserialize};

/// Represents the type of ContractParameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ContractParameterType {
    /// Indicates that the parameter can be of any type.
    Any = 0x00,

    /// Indicates that the parameter is of Boolean type.
    Boolean = 0x10,

    /// Indicates that the parameter is an integer.
    Integer = 0x11,

    /// Indicates that the parameter is a byte array.
    ByteArray = 0x12,

    /// Indicates that the parameter is a string.
    String = 0x13,

    /// Indicates that the parameter is a 160-bit hash.
    Hash160 = 0x14,

    /// Indicates that the parameter is a 256-bit hash.
    Hash256 = 0x15,

    /// Indicates that the parameter is a public key.
    PublicKey = 0x16,

    /// Indicates that the parameter is a signature.
    Signature = 0x17,

    /// Indicates that the parameter is an array.
    Array = 0x20,

    /// Indicates that the parameter is a map.
    Map = 0x22,

    /// Indicates that the parameter is an interoperable interface.
    InteropInterface = 0x30,

    /// It can be only used as the return type of a method, meaning that the method has no return value.
    Void = 0xff,
}

impl TryFrom<u8> for ContractParameterType {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(ContractParameterType::Any),
            0x10 => Ok(ContractParameterType::Boolean),
            0x11 => Ok(ContractParameterType::Integer),
            0x12 => Ok(ContractParameterType::ByteArray),
            0x13 => Ok(ContractParameterType::String),
            0x14 => Ok(ContractParameterType::Hash160),
            0x15 => Ok(ContractParameterType::Hash256),
            0x16 => Ok(ContractParameterType::PublicKey),
            0x17 => Ok(ContractParameterType::Signature),
            0x20 => Ok(ContractParameterType::Array),
            0x22 => Ok(ContractParameterType::Map),
            0x30 => Ok(ContractParameterType::InteropInterface),
            0xff => Ok(ContractParameterType::Void),
            _ => Err("Invalid ContractParameterType value"),
        }
    }
}
