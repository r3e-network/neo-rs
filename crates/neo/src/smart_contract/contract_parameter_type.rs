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
