use neo_io::macros::{OptionExt, ValidateLength};
use neo_io::serializable::helper::get_var_size_bytes;
use neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_primitives::CallFlags;
use neo_primitives::OracleResponseCode;
use neo_primitives::UInt160;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use serde::{Deserialize, Serialize};

/// Indicates the maximum size of the Result field.
pub const MAX_RESULT_SIZE: usize = u16::MAX as usize;

/// Indicates that the transaction is an oracle response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleResponse {
    /// The ID of the oracle request.
    pub id: u64,

    /// The response code for the oracle request.
    pub code: OracleResponseCode,

    /// The result for the oracle request.
    pub result: Vec<u8>,
}

impl OracleResponse {
    /// Creates a new oracle response attribute.
    pub fn new(id: u64, code: OracleResponseCode, result: Vec<u8>) -> Self {
        Self { id, code, result }
    }

    /// Get the fixed script for oracle response transactions.
    pub fn get_fixed_script() -> Vec<u8> {
        // C# OracleResponse.FixedScript:
        // `new ScriptBuilder().EmitDynamicCall(NativeContract.Oracle.Hash, "finish")`.
        let oracle_hash = UInt160::parse("0xfe924b7cfe89ddd271abaf7210a80a7e11178758")
            .expect("Oracle native contract hash is valid");
        let mut builder = ScriptBuilder::new();
        builder.emit_opcode(OpCode::NEWARRAY0);
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push(b"finish");
        builder.emit_push(&oracle_hash.to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");
        builder.to_array()
    }

    // verify: Mirrors C# `OracleResponse.Verify`
    // (Neo/Network/P2P/Payloads/OracleResponse.cs), all five checks.
    // Handled by TransactionAttribute dispatch.

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}

impl Serializable for OracleResponse {
    fn size(&self) -> usize {
        8 + // Id (u64)
        1 + // Code (u8)
        get_var_size_bytes(&self.result) // Result with var length prefix
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u64(self.id)?;
        writer.write_u8(self.code as u8)?;
        // Use ValidateLength trait to reduce boilerplate
        self.result.validate_max_length(MAX_RESULT_SIZE, "Result")?;
        writer.write_var_bytes(&self.result)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let id = reader.read_u64()?;

        let code_byte = reader.read_u8()?;
        // Use OptionExt trait to reduce boilerplate
        let code =
            OracleResponseCode::from_byte(code_byte).ok_or_invalid_data("Invalid response code")?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_script_matches_csharp_dynamic_call_finish() {
        let expected = [
            0xc2, // NEWARRAY0
            0x1f, // PUSH15 (CallFlags.All)
            0x0c, 0x06, b'f', b'i', b'n', b'i', b's', b'h', 0x0c,
            0x14, // Oracle native contract hash
            0x58, 0x87, 0x17, 0x11, 0x7e, 0x0a, 0xa8, 0x10, 0x72, 0xaf, 0xab, 0x71, 0xd2, 0xdd,
            0x89, 0xfe, 0x7c, 0x4b, 0x92, 0xfe, 0x41, 0x62, 0x7d, 0x5b,
            0x52, // SYSCALL System.Contract.Call
        ];

        assert_eq!(OracleResponse::get_fixed_script().as_slice(), expected);
    }
}
