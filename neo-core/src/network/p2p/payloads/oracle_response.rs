// Copyright (C) 2015-2025 The Neo Project.
//
// oracle_response.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::oracle_response_code::OracleResponseCode;
use crate::macros::{OptionExt, ValidateLength};
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::native::{oracle_contract::OracleContract, NativeContract};
use crate::WitnessScope;
use lazy_static::lazy_static;
use neo_vm::{op_code::OpCode, ScriptBuilder};
use serde::{Deserialize, Serialize};
use tracing::error;

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
        lazy_static! {
            static ref ORACLE_FINISH_SCRIPT: Vec<u8> = {
                let oracle_hash = OracleContract::new().hash();
                let mut builder = ScriptBuilder::new();
                builder.emit_opcode(OpCode::NEWARRAY0);
                builder.emit_push_int(CallFlags::ALL.bits() as i64);
                builder.emit_push("finish".as_bytes());
                builder.emit_push(&oracle_hash.to_array());
                if let Err(err) = builder.emit_syscall("System.Contract.Call") {
                    error!(?err, "Failed to build OracleContract finish syscall script");
                    return Vec::new();
                }
                builder.to_array()
            };
        }

        ORACLE_FINISH_SCRIPT.clone()
    }

    /// Verify the oracle response attribute.
    pub fn verify(
        &self,
        _settings: &ProtocolSettings,
        _snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool {
        // Check that no signers have scopes other than None
        if tx.signers().iter().any(|s| s.scopes != WitnessScope::NONE) {
            return false;
        }

        // Check that the script matches the fixed script
        if tx.script() != Self::get_fixed_script() {
            return false;
        }

        // Additional verification would check the oracle request exists
        // and matches the response
        true
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u64(self.id)?;
        writer.write_u8(self.code as u8)?;
        // Use ValidateLength trait to reduce boilerplate
        self.result.validate_max_length(MAX_RESULT_SIZE, "Result")?;
        writer.write_var_bytes(&self.result)?;
        Ok(())
    }
}

impl Serializable for OracleResponse {
    fn size(&self) -> usize {
        8 + // Id (u64)
        1 + // Code (u8)
        get_var_size(self.result.len() as u64) + self.result.len() // Result with var length prefix
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
