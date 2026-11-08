//! Oracle verify method.
//!
//! Migrated from `neo-core/src/smart_contract/native/oracle_contract/verification.rs`.
//! Import adaptation: `crate::error::*` → `neo_smart_contract::error::*`;
//! `crate::network::p2p::payloads::*` → `neo_protocol::payloads::*`;
//! `crate::smart_contract::application_engine::*` →
//! `neo_smart_contract::application_engine::*`.
//!
//! The oracle-response verification logic that `neo-core` kept on the
//! `OracleResponse` payload type is reconstructed here in the native layer,
//! because it depends on native contracts (`OracleContract`, `LedgerContract`,
//! `RoleManagement`, `NativeHelpers`) that are not visible from `neo-protocol`.
//! The behaviour is unchanged from the original `OracleResponse::verify`.

use super::OracleContract;
use crate::native::{LedgerContract, NativeHelpers, Role, RoleManagement};
use neo_primitives::CallFlags;
use neo_protocol::payloads::transaction_attribute::TransactionAttribute;
use neo_protocol::payloads::{OracleResponse, Transaction, WitnessScope};
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::CoreResult as Result;
use neo_storage::persistence::DataCache;
use neo_vm::{OpCode, ScriptBuilder};
use tracing::error;

impl OracleContract {
    pub(super) fn verify(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let Some(tx) = engine
            .script_container()
            .and_then(|container| container.as_any().downcast_ref::<Transaction>())
        else {
            return Ok(vec![0]);
        };
        let snapshot = engine.snapshot_cache();
        let valid = tx.attributes().iter().any(|attr| match attr {
            TransactionAttribute::OracleResponse(response) => {
                Self::verify_oracle_response(response, snapshot.as_ref(), tx)
            }
            _ => false,
        });
        Ok(vec![if valid { 1 } else { 0 }])
    }

    /// Get the fixed script for oracle response transactions.
    fn get_fixed_script() -> Vec<u8> {
        static ORACLE_FINISH_SCRIPT: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        ORACLE_FINISH_SCRIPT
            .get_or_init(|| {
                let oracle_hash = OracleContract::new().hash;
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

    /// Verify the oracle response attribute against on-chain state.
    fn verify_oracle_response(
        response: &OracleResponse,
        snapshot: &DataCache,
        tx: &Transaction,
    ) -> bool {
        // Check that no signers have scopes other than None.
        if tx.signers().iter().any(|s| s.scopes != WitnessScope::NONE) {
            return false;
        }

        // Check that the script matches the fixed script.
        if tx.script() != Self::get_fixed_script() {
            return false;
        }

        let oracle = OracleContract::new();
        let Ok(Some(request)) = oracle.get_request(snapshot, response.id) else {
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
        let Ok(current_index) = ledger.current_index(snapshot) else {
            return false;
        };
        let designated = match RoleManagement::new().get_designated_by_role_at(
            snapshot,
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
