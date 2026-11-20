//! `MethodToken` — matches C# `Neo.SmartContract.MethodToken` exactly.
//!
//! This is a smart-contract concept: it describes a method a contract calls
//! statically (contract hash, method name, argument count, return flag and
//! `CallFlags`). It therefore lives with the smart-contract model instead of
//! the generic `neo-io` codec crate, which only supplies the serialization
//! primitives used below.
//!
//! Serialization has exactly one validated path: the [`Serializable`]
//! implementation. The former byte-slice `serialize`/`deserialize`/`size`
//! helpers bypassed the method-name restrictions and are intentionally gone —
//! callers go through [`Serializable`] (or `BinarySerializer`), so a token can
//! never be written or read in a shape that validation would reject.

use neo_io::serializable::helper::get_var_size_str;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::{CallFlags, UInt160};
use serde::{Deserialize, Serialize};

/// Longest method name accepted by C# (`MethodToken.Method` is VarString with
/// a 32-byte cap).
const MAX_METHOD_NAME_LENGTH: usize = 32;

/// Represents the methods that a contract will call statically (matches C#
/// `MethodToken`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodToken {
    /// The hash of the contract to be called.
    pub hash: UInt160,

    /// The name of the method to be called.
    pub method: String,

    /// The number of parameters of the method to be called.
    pub parameters_count: u16,

    /// Indicates whether the method to be called has a return value.
    pub has_return_value: bool,

    /// The `CallFlags` to be used to call the contract.
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

/// Validates the method-name rules shared by construction and serialization.
fn validate_method_name(method: &str) -> IoResult<()> {
    if method.starts_with('_') {
        return Err(IoError::invalid_data(
            "Method name cannot start with '_'".to_string(),
        ));
    }
    if method.len() > MAX_METHOD_NAME_LENGTH {
        return Err(IoError::invalid_data("Method name too long"));
    }
    Ok(())
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
        validate_method_name(&self.method)?;

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
        let method = reader.read_var_string(MAX_METHOD_NAME_LENGTH)?;
        validate_method_name(&method)?;
        let parameters_count = reader.read_uint16()?;
        let has_return_value = reader.read_boolean()?;
        let call_flags_bits = reader.read_byte()?;
        let call_flags = CallFlags::from_bits(call_flags_bits)
            .ok_or_else(|| IoError::invalid_data("CallFlags is not valid"))?;

        Ok(Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }
}

impl MethodToken {
    /// Creates a new `MethodToken`, applying the method-name restrictions.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message when the method name starts with `_` or
    /// exceeds 32 bytes.
    pub fn new(
        hash: UInt160,
        method: String,
        parameters_count: u16,
        has_return_value: bool,
        call_flags: CallFlags,
    ) -> Result<Self, String> {
        validate_method_name(&method).map_err(|error| error.to_string())?;

        Ok(Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }

    /// Converts to JSON representation (C# `MethodToken.ToJson`).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "hash": self.hash.to_string(),
            "method": self.method,
            "paramcount": self.parameters_count,
            "hasreturnvalue": self.has_return_value,
            "callflags": self.call_flags.bits(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_io::BinaryWriter;

    fn token(method: &str) -> MethodToken {
        MethodToken::new(UInt160::zero(), method.to_string(), 2, true, CallFlags::ALL)
            .expect("valid method name")
    }

    #[test]
    fn round_trips_through_the_validated_codec() {
        let token = token("transfer");
        let mut writer = BinaryWriter::new();
        writer.write_serializable(&token).expect("serialize");

        let bytes = writer.into_bytes();
        assert_eq!(bytes.len(), token.size());

        let mut reader = MemoryReader::new(&bytes);
        let decoded = <MethodToken as Serializable>::deserialize(&mut reader).expect("deserialize");
        assert_eq!(decoded, token);
    }

    #[test]
    fn constructor_rejects_underscore_prefix() {
        assert!(
            MethodToken::new(
                UInt160::zero(),
                "_reserved".to_string(),
                0,
                false,
                CallFlags::NONE
            )
            .is_err()
        );
    }

    #[test]
    fn constructor_rejects_over_long_names() {
        let long = "a".repeat(MAX_METHOD_NAME_LENGTH + 1);
        assert!(MethodToken::new(UInt160::zero(), long, 0, false, CallFlags::NONE).is_err());
    }

    #[test]
    fn serialization_rejects_a_token_written_past_validation() {
        // The codec must never emit a shape that `deserialize` would reject.
        let mut token = token("ok");
        token.method = "_bypass".to_string();

        let mut writer = BinaryWriter::new();
        assert!(writer.write_serializable(&token).is_err());
    }

    #[test]
    fn deserialization_rejects_an_underscore_prefixed_name() {
        // Hand-build a payload that the constructor would refuse.
        let mut writer = BinaryWriter::new();
        writer.write_bytes(&UInt160::zero().as_bytes()).unwrap();
        writer.write_var_string("_hidden").unwrap();
        writer.write_u16(0).unwrap();
        writer.write_bool(false).unwrap();
        writer.write_u8(CallFlags::NONE.bits()).unwrap();

        let bytes = writer.into_bytes();
        let mut reader = MemoryReader::new(&bytes);
        assert!(<MethodToken as Serializable>::deserialize(&mut reader).is_err());
    }
}
