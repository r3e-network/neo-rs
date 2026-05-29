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
use crate::neo_io::serializable::helper::get_var_size_bytes;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::ScriptBuilder;
use crate::smart_contract::CallFlags;
use crate::smart_contract::native::{
    oracle_contract::OracleContract, LedgerContract, NativeContract, NativeHelpers, Role,
    RoleManagement,
};
use crate::WitnessScope;
use neo_vm_rs::OpCode;
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
        static ORACLE_FINISH_SCRIPT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        ORACLE_FINISH_SCRIPT
            .get_or_init(|| {
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
            })
            .clone()
    }

    /// Verify the oracle response attribute. Mirrors C# `OracleResponse.Verify`
    /// (Neo/Network/P2P/Payloads/OracleResponse.cs), all five checks:
    pub fn verify(
        &self,
        _settings: &ProtocolSettings,
        snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool {
        // 1. Every signer must use WitnessScope.None.
        if tx.signers().iter().any(|s| s.scopes != WitnessScope::NONE) {
            return false;
        }

        // 2. The transaction script must be exactly the Oracle.finish fixed script.
        if tx.script() != Self::get_fixed_script() {
            return false;
        }

        // 3. The referenced oracle request must still exist.
        let request = match OracleContract::new().get_request(snapshot, self.id) {
            Ok(Some(request)) => request,
            _ => return false,
        };

        // 4. NetworkFee + SystemFee must equal the request's GasForResponse.
        if tx.network_fee().saturating_add(tx.system_fee()) != request.gas_for_response {
            return false;
        }

        // 5. A signer must be the BFT address of the Oracle-role nodes designated
        //    for the next block (CurrentIndex + 1), matching C#.
        let next_index = match LedgerContract::new().current_index(snapshot) {
            Ok(index) => index.saturating_add(1),
            Err(_) => return false,
        };
        let oracle_nodes = match RoleManagement::new().get_designated_by_role_at(
            snapshot,
            Role::Oracle,
            next_index,
        ) {
            Ok(nodes) if !nodes.is_empty() => nodes,
            _ => return false,
        };
        let oracle_account = NativeHelpers::get_bft_address(&oracle_nodes);
        tx.signers().iter().any(|s| s.account == oracle_account)
    }

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
