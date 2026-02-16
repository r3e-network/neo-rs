//! MethodToken - matches C# Neo.SmartContract.MethodToken exactly

use crate::UInt160;
use crate::smart_contract::CallFlags;
use serde::{Deserialize, Serialize};

/// Represents the methods that a contract will call statically (matches C# MethodToken)
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
    /// Creates a new MethodToken
    pub fn new(
        hash: UInt160,
        method: String,
        parameters_count: u16,
        has_return_value: bool,
        call_flags: CallFlags,
    ) -> Result<Self, String> {
        // Validate method name doesn't start with underscore
        if method.starts_with('_') {
            return Err("Method name cannot start with underscore".to_string());
        }

        // Validate method name length (max 32 chars in C#)
        if method.len() > 32 {
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

    /// Gets the size in bytes when serialized
    pub fn size(&self) -> usize {
        20 + // UInt160 (hash)
        1 + self.method.len() + // VarString (method)
        2 + // u16 (parameters_count)
        1 + // bool (has_return_value)
        1 // CallFlags (1 byte)
    }

    /// Deserialize from bytes
    pub fn deserialize(reader: &mut &[u8]) -> Result<Self, String> {
        use std::io::Read;

        // Read hash
        let mut hash_bytes = [0u8; 20];
        reader
            .read_exact(&mut hash_bytes)
            .map_err(|e| e.to_string())?;
        let hash = UInt160::from_bytes(&hash_bytes).map_err(|e| e.to_string())?;

        // Read method name (var string)
        let mut len_buf = [0u8; 1];
        reader.read_exact(&mut len_buf).map_err(|e| e.to_string())?;
        let len = len_buf[0] as usize;
        if len > 32 {
            return Err("Method name too long".to_string());
        }
        let mut method_bytes = vec![0u8; len];
        reader
            .read_exact(&mut method_bytes)
            .map_err(|e| e.to_string())?;
        let method = String::from_utf8(method_bytes).map_err(|e| e.to_string())?;

        if method.starts_with('_') {
            return Err("Method name cannot start with underscore".to_string());
        }

        // Read parameters count
        let mut params_bytes = [0u8; 2];
        reader
            .read_exact(&mut params_bytes)
            .map_err(|e| e.to_string())?;
        let parameters_count = u16::from_le_bytes(params_bytes);

        // Read has return value
        let mut has_return_bytes = [0u8; 1];
        reader
            .read_exact(&mut has_return_bytes)
            .map_err(|e| e.to_string())?;
        let has_return_value = has_return_bytes[0] != 0;

        // Read call flags
        let mut flags_bytes = [0u8; 1];
        reader
            .read_exact(&mut flags_bytes)
            .map_err(|e| e.to_string())?;
        let call_flags =
            CallFlags::from_bits(flags_bytes[0]).ok_or_else(|| "Invalid call flags".to_string())?;

        Ok(Self {
            hash,
            method,
            parameters_count,
            has_return_value,
            call_flags,
        })
    }

    /// Serialize to bytes
    pub fn serialize(&self, writer: &mut Vec<u8>) {
        // Write hash
        writer.extend_from_slice(&self.hash.to_bytes());

        // Write method name (var string)
        writer.push(self.method.len() as u8);
        writer.extend_from_slice(self.method.as_bytes());

        // Write parameters count
        writer.extend_from_slice(&self.parameters_count.to_le_bytes());

        // Write has return value
        writer.push(if self.has_return_value { 1 } else { 0 });

        // Write call flags
        writer.push(self.call_flags.bits());
    }

    /// Converts to JSON representation
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
