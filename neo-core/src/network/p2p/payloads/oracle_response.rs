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

//! Oracle response transaction attribute.
//!
//! The canonical `OracleResponse` payload (and `MAX_RESULT_SIZE`) is owned by
//! `neo-protocol` and re-exported here for backward compatibility (R2 leaf-type
//! unification). All structural API (`new`, the `id`/`code`/`result` fields,
//! and wire `Serializable`) is provided by `neo-protocol`.
//!
//! neo-core keeps only the VM/native-contract aware helpers (`get_fixed_script`
//! and `verify`), exposed via the [`OracleResponseExt`] extension trait because
//! they cannot be inherent methods on a foreign type (orphan rule).

use crate::ScriptBuilder;
use crate::WitnessScope;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::CallFlags;
use crate::smart_contract::native::{
    LedgerContract, NativeContract, NativeHelpers, Role, RoleManagement,
    oracle_contract::OracleContract,
};
use neo_vm::OpCode;
use tracing::error;

/// Maximum size of the oracle response result field, owned by `neo-protocol`.
pub use neo_protocol::payloads::oracle_response::MAX_RESULT_SIZE;
/// Canonical oracle response transaction attribute, owned by `neo-protocol`.
pub use neo_protocol::payloads::oracle_response::OracleResponse;

/// neo-core-specific VM/native helpers for [`OracleResponse`].
pub trait OracleResponseExt {
    /// Get the fixed script for oracle response transactions.
    fn get_fixed_script() -> Vec<u8>;

    /// Verify the oracle response attribute.
    /// Matches C# Verify method.
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool;
}

impl OracleResponseExt for OracleResponse {
    fn get_fixed_script() -> Vec<u8> {
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

    fn verify(
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

        let oracle = OracleContract::new();
        let Ok(Some(request)) = oracle.get_request(_snapshot, self.id) else {
            return false;
        };

        // The response transaction must consume exactly the GAS reserved by the request.
        let Some(total_fee) = tx.network_fee().checked_add(tx.system_fee()) else {
            return false;
        };
        if total_fee != request.gas_for_response {
            return false;
        }

        // Oracle responses are authorized by the BFT address of the designated oracle
        // nodes for the next block (the same height used by the C# implementation).
        let ledger = LedgerContract::new();
        let Ok(current_index) = ledger.current_index(_snapshot) else {
            return false;
        };
        let designated = match RoleManagement::new().get_designated_by_role_at(
            _snapshot,
            Role::Oracle,
            current_index.saturating_add(1),
        ) {
            Ok(nodes) if !nodes.is_empty() => nodes,
            _ => return false,
        };
        let oracle_account = NativeHelpers::get_bft_address(&designated);
        tx.signers().iter().any(|s| s.account == oracle_account)
    }
}
