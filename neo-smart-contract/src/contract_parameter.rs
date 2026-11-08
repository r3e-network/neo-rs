//! Contract parameter types for RPC serialization.
//!
//! TODO: This is a stub implementation. The full ContractParameter type
//! needs to be ported from C# neo to support contract invocation via RPC.

use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};

/// Represents a contract parameter value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ContractParameterValue {
    /// Boolean value
    Boolean(bool),
    /// Integer value
    Integer(i64),
    /// Byte array value
    ByteArray(Vec<u8>),
    /// String value
    String(String),
    /// Hash160 value
    Hash160(UInt160),
    /// Hash256 value
    Hash256(UInt256),
    /// Public key value (33 bytes)
    PublicKey(Vec<u8>),
    /// Signature value (64 bytes)
    Signature(Vec<u8>),
    /// Array of parameters
    Array(Vec<ContractParameter>),
    /// Map of parameters
    Map(Vec<(ContractParameter, ContractParameter)>),
    /// Void/null type
    Void,
    /// Interop interface
    InteropInterface,
    /// Any type
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
    pub fn from_json(_json: &serde_json::Value) -> Result<Self, String> {
        // TODO: Implement JSON deserialization
        Err("ContractParameter::from_json not yet implemented".to_string())
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
}
