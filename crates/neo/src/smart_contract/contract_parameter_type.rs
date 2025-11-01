//! ContractParameterType - matches C# Neo.SmartContract.ContractParameterType exactly

/// Represents the type of ContractParameter (matches C# ContractParameterType)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractParameterType {
    /// Indicates that the parameter can be of any type
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
}

impl Default for ContractParameterType {
    fn default() -> Self {
        ContractParameterType::Any
    }
}

impl serde::Serialize for ContractParameterType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ContractParameterType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        ContractParameterType::from_string(&value).map_err(|e| serde::de::Error::custom(e))
    }
}
