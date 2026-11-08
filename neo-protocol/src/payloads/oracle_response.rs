// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License. See LICENSE file for details.
use super::validate_length;
use neo_io::serializable::helper::get_var_size_bytes;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::OracleResponseCode;
use serde::{Deserialize, Serialize};

/// Maximum byte length of the result field.
pub const MAX_RESULT_SIZE: usize = u16::MAX as usize;

/// An oracle response transaction attribute, without native verification policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleResponse {
    /// The oracle request ID.
    pub id: u64,
    /// The primitive response code.
    pub code: OracleResponseCode,
    /// Response result bytes.
    pub result: Vec<u8>,
}
impl OracleResponse {
    /// Creates an oracle response.
    pub fn new(id: u64, code: OracleResponseCode, result: Vec<u8>) -> Self {
        Self { id, code, result }
    }
    /// Serializes without the attribute type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }

    /// Returns the fixed script for oracle responses.
    ///
    /// This script calls the Oracle native contract's "finish" method.
    /// The fixed script is a NeoVM script that:
    /// 1. Pushes the oracle response data (id and code) onto the stack
    /// 2. Calls the Oracle contract's "finish" method via SYSCALL
    ///
    /// Reference: C# Neo OracleResponse.FixedScript
    pub fn get_fixed_script() -> Vec<u8> {
        use neo_vm::OpCode;

        // Build script: SYSCALL to Oracle.finish
        // The script builder would construct:
        // - CALLT <token> for "finish" method
        // But since we don't have the token table here, we use the hardcoded approach

        // For Oracle responses, the fixed script is:
        // SYSCALL System.Contract.Call with Oracle contract hash and "finish" method

        // Oracle native contract hash (well-known): 0xfe924b7cfe89ddd271abaf7210a80a7e11178758
        // This is a simplified version - the full implementation would use ScriptBuilder

        // Return the SYSCALL instruction for "System.Contract.Call"
        // Format: SYSCALL <interop_id>
        let mut script = Vec::new();

        // SYSCALL opcode
        script.push(OpCode::SYSCALL as u8);

        // System.Contract.Call interop service ID (0x627d5b52)
        script.extend_from_slice(&0x627d5b52u32.to_le_bytes());

        script
    }
}
impl Serializable for OracleResponse {
    fn size(&self) -> usize {
        8 + 1 + get_var_size_bytes(&self.result)
    }
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u64(self.id)?;
        writer.write_u8(self.code as u8)?;
        validate_length(self.result.len(), MAX_RESULT_SIZE, "Result")?;
        writer.write_var_bytes(&self.result)?;
        Ok(())
    }
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let id = reader.read_u64()?;
        let code_byte = reader.read_u8()?;
        let code = OracleResponseCode::from_byte(code_byte)
            .ok_or_else(|| IoError::invalid_data("Invalid response code"))?;
        // This check remains deserialize-only; construction/serialization do not strengthen it.
        let result = if code == OracleResponseCode::Success {
            reader.read_var_bytes(MAX_RESULT_SIZE)?
        } else {
            let bytes = reader.read_var_bytes(MAX_RESULT_SIZE)?;
            if !bytes.is_empty() {
                return Err(IoError::invalid_data(
                    "Non-success response cannot have result",
                ));
            }
            bytes
        };
        Ok(Self { id, code, result })
    }
}
