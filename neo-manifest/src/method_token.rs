//! `MethodToken` — NEF static-call descriptor (matches C#
//! `Neo.SmartContract.MethodToken`).
//!
//! ## Layering
//!
//! Pure data type in **Layer 1 (protocol)**. Depends only on
//! `neo-primitives` (for `UInt160`, `CallFlags`) and the serde
//! family. The `Serializable` impl lives here too because the
//! on-wire encoding is a pure data concern.

use neo_io::serializable::helper::get_var_size_str;
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
    ) -> Result<Self, String> {
        if method.starts_with('_') {
            return Err("Method name cannot start with underscore".to_string());
        }
        if method.len() > Self::MAX_METHOD_LENGTH {
            return Err("Method name too long".to_string());
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
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let obj = json
            .as_object()
            .ok_or_else(|| "Expected object for MethodToken".to_string())?;
        let hash = obj
            .get("hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing hash".to_string())?
            .parse()
            .map_err(|e| format!("Invalid hash: {e}"))?;
        let method = obj
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing method".to_string())?
            .to_string();
        let parameters_count = obj
            .get("paramcount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u16;
        let has_return_value = obj
            .get("hasreturnvalue")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let call_flags_bits = obj
            .get("callflags")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;
        let call_flags = CallFlags::from_bits(call_flags_bits)
            .ok_or_else(|| "Invalid call flags".to_string())?;
        Self::new(hash, method, parameters_count, has_return_value, call_flags)
    }
}

impl Serializable for MethodToken {
    fn size(&self) -> usize {
        UInt160::LENGTH
            + get_var_size_str(&self.method)
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
        let hash = UInt160::from_bytes(&hash_bytes)
            .map_err(|e| IoError::invalid_data(e.to_string()))?;
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
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_token() {
        let t = MethodToken::default();
        assert_eq!(t.hash, UInt160::zero());
        assert_eq!(t.method, "");
        assert_eq!(t.parameters_count, 0);
        assert!(!t.has_return_value);
        assert_eq!(t.call_flags, CallFlags::NONE);
    }

    #[test]
    fn new_rejects_underscore_prefix() {
        let result = MethodToken::new(
            UInt160::zero(),
            "_private".to_string(),
            0,
            false,
            CallFlags::NONE,
        );
        assert!(result.is_err());
    }

    #[test]
    fn new_rejects_long_method_name() {
        let result = MethodToken::new(
            UInt160::zero(),
            "a".repeat(MethodToken::MAX_METHOD_LENGTH + 1),
            0,
            false,
            CallFlags::NONE,
        );
        assert!(result.is_err());
    }
}
