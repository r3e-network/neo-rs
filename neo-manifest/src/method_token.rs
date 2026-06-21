//! `MethodToken` — NEF static-call descriptor (matches C#
//! `Neo.SmartContract.MethodToken`).
//!
//! ## Layering
//!
//! Pure data type in **Layer 1 (protocol)**. Depends only on
//! `neo-primitives` (for `UInt160`, `CallFlags`) and the serde
//! family. The `Serializable` impl lives here too because the
//! on-wire encoding is a pure data concern.

use neo_error::{CoreError, CoreResult};
use neo_io::serializable::helper::SerializeHelper;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::{CallFlags, UInt160};
use serde::{Deserialize, Serialize};

/// Represents the methods that a contract will call statically
/// (matches C# MethodToken)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodToken {
    /// The hash of the contract to be called
    pub hash: UInt160,

    /// The name of the method to be called
    pub method: String,

    /// The number of parameters of the method to be called
    pub parameters_count: u16,

    /// Indicates whether the method to be called has a return value
    pub has_return_value: bool,

    /// The CallFlags to be used to call the contract
    pub call_flags: CallFlags,
}

impl Default for MethodToken {
    fn default() -> Self {
        Self {
            hash: UInt160::zero(),
            method: String::new(),
            parameters_count: 0,
            has_return_value: false,
            call_flags: CallFlags::NONE,
        }
    }
}

impl MethodToken {
    /// The maximum length of a method name.
    pub const MAX_METHOD_LENGTH: usize = 32;

    /// Creates a new MethodToken with validation.
    pub fn new(
        hash: UInt160,
        method: String,
        parameters_count: u16,
        has_return_value: bool,
        call_flags: CallFlags,
    ) -> CoreResult<Self> {
        if method.starts_with('_') {
            return Err(CoreError::other("Method name cannot start with underscore"));
        }
        if method.len() > Self::MAX_METHOD_LENGTH {
            return Err(CoreError::other("Method name too long"));
        }
        Ok(Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }

    /// Converts to JSON representation.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "hash": self.hash.to_string(),
            "method": self.method,
            "paramcount": self.parameters_count,
            "hasreturnvalue": self.has_return_value,
            "callflags": self.call_flags.bits(),
        })
    }

    /// Creates from JSON representation.
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected object for MethodToken"))?;
        let hash = obj
            .get("hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Missing hash"))?
            .parse()
            .map_err(|e| CoreError::other(format!("Invalid hash: {e}")))?;
        let method = obj
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Missing method"))?
            .to_string();
        let parameters_count = obj.get("paramcount").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
        let has_return_value = obj
            .get("hasreturnvalue")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let call_flags_bits = obj.get("callflags").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
        let call_flags = CallFlags::from_bits(call_flags_bits)
            .ok_or_else(|| CoreError::other("Invalid call flags"))?;
        Self::new(hash, method, parameters_count, has_return_value, call_flags)
    }
}

impl Serializable for MethodToken {
    fn size(&self) -> usize {
        UInt160::LENGTH
            + SerializeHelper::get_var_size_str(&self.method)
            + 2 // ParametersCount (u16)
            + 1 // HasReturnValue (bool)
            + 1 // CallFlags (u8)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        if self.method.starts_with('_') {
            return Err(IoError::invalid_data(
                "Method name cannot start with '_'".to_string(),
            ));
        }
        if self.method.len() > Self::MAX_METHOD_LENGTH {
            return Err(IoError::invalid_data("Method name too long"));
        }

        writer.write_bytes(&self.hash.as_bytes())?;
        writer.write_var_string(&self.method)?;
        writer.write_u16(self.parameters_count)?;
        writer.write_bool(self.has_return_value)?;
        writer.write_u8(self.call_flags.bits())?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let hash_bytes = reader.read_bytes(UInt160::LENGTH)?;
        let hash =
            UInt160::from_bytes(&hash_bytes).map_err(|e| IoError::invalid_data(e.to_string()))?;
        let method = reader.read_var_string(Self::MAX_METHOD_LENGTH)?;
        if method.starts_with('_') {
            return Err(IoError::invalid_data(
                "Method name cannot start with '_'".to_string(),
            ));
        }
        let parameters_count = reader.read_uint16()?;
        let has_return_value = reader.read_boolean()?;
        let call_flags_bits = reader.read_byte()?;
        let call_flags = CallFlags::from_bits(call_flags_bits)
            .ok_or_else(|| IoError::invalid_data("CallFlags is not valid"))?;

        Ok(MethodToken {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }
}

#[cfg(test)]
#[path = "tests/method_token.rs"]
mod tests;
