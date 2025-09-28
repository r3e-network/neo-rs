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
use crate::neo_io::{MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::WitnessScope;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

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
        // This would emit: EmitDynamicCall(NativeContract.Oracle.Hash, "finish")
        // For now, return a placeholder
        vec![0x41, 0xC7, 0x24, 0x08] // Simplified script
    }

    /// Verify the oracle response attribute.
    pub fn verify(&self, _snapshot: &DataCache, tx: &super::transaction::Transaction) -> bool {
        // Check that no signers have scopes other than None
        if tx.signers.iter().any(|s| s.scopes != WitnessScope::NONE) {
            return false;
        }

        // Check that the script matches the fixed script
        if tx.script() != &Self::get_fixed_script() {
            return false;
        }

        // Additional verification would check the oracle request exists
        // and matches the response
        true
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.id.to_le_bytes())?;
        writer.write_all(&[self.code as u8])?;

        // Write result as var bytes
        if self.result.len() > MAX_RESULT_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Result too large",
            ));
        }
        writer.write_all(&(self.result.len() as u16).to_le_bytes())?;
        writer.write_all(&self.result)?;
        Ok(())
    }
}

impl Serializable for OracleResponse {
    fn size(&self) -> usize {
        8 + // Id (u64)
        1 + // Code (u8)
        2 + self.result.len() // Result with var length prefix
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_all(&self.id.to_le_bytes())?;
        writer.write_all(&[self.code as u8])?;

        // Write result as var bytes
        if self.result.len() > MAX_RESULT_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Result too large",
            ));
        }
        writer.write_all(&(self.result.len() as u16).to_le_bytes())?;
        writer.write_all(&self.result)?;

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let id = reader.read_u64().map_err(|e| e.to_string())?;

        let code_byte = reader.read_u8().map_err(|e| e.to_string())?;
        let code = OracleResponseCode::from_byte(code_byte)
            .ok_or_else(|| format!("Invalid response code: {}", code_byte))?;

        let result_len = reader.read_var_int().map_err(|e| e.to_string())?;
        if result_len > MAX_RESULT_SIZE as u64 {
            return Err("Result too large".to_string());
        }

        let result = if result_len > 0 {
            if code != OracleResponseCode::Success {
                return Err("Non-success response cannot have result".to_string());
            }
            reader
                .read_bytes(result_len as usize)
                .map_err(|e| e.to_string())?
        } else {
            Vec::new()
        };

        Ok(Self { id, code, result })
    }
}
