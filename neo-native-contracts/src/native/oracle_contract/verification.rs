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
use neo_protocol::payloads::get_fixed_script;
use neo_protocol::payloads::transaction_attribute::TransactionAttribute;
use neo_protocol::payloads::{OracleResponse, Transaction, WitnessScope};
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::CoreResult as Result;
use neo_storage::persistence::DataCache;

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

    /// Verify the oracle response attribute against on-chain state.
    ///
    /// The fixed script comparison uses the single source of truth in
    /// `neo_protocol::payloads::get_fixed_script`, so this verifier and the
    /// transaction producers can never drift apart.
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
        if tx.script() != get_fixed_script() {
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

#[cfg(test)]
mod tests {
    use super::OracleContract;
    use neo_protocol::payloads::get_fixed_script;
    use neo_smart_contract::application_engine::native_contract::NativeContract;
    use neo_vm::{OpCode, ScriptBuilder};

    /// Byte-level consistency check between the protocol-layer fixed script
    /// and the Oracle native contract identity: the script embedded hash must
    /// equal `OracleContract::new().hash()`, proving the protocol constant and
    /// the native contract registry agree on the Oracle contract address.
    #[test]
    fn protocol_fixed_script_embeds_native_oracle_hash() {
        let script = get_fixed_script();
        let oracle_hash = OracleContract::new().hash().to_array();

        // Layout: NEWARRAY0(1) PUSH15(1) PUSHDATA1+"finish"(8) PUSHDATA1+hash(22) SYSCALL(5).
        assert_eq!(script.len(), 37, "fixed script length is stable");
        assert_eq!(script[0], OpCode::NEWARRAY0 as u8);
        assert_eq!(script[1], 0x1f, "PUSH15 encodes CallFlags::ALL");
        assert_eq!(script[2], OpCode::PUSHDATA1 as u8);
        assert_eq!(script[3], 6, "pushed method name length");
        assert_eq!(&script[4..10], b"finish");
        assert_eq!(&script[10..12], &[OpCode::PUSHDATA1 as u8, 20][..]);
        assert_eq!(&script[12..32], &oracle_hash[..]);
        assert_eq!(script[32], OpCode::SYSCALL as u8);
        // The interop hash 0x627d5b52 is serialized big-endian in the script
        // (the trailing bytes of the golden hex ...41627d5b52).
        assert_eq!(&script[33..37], &0x627d5b52u32.to_be_bytes()[..]);
    }

    /// Rebuilding the script locally from the native contract hash must
    /// reproduce the protocol-layer bytes exactly (guards against any future
    /// divergence between the two construction orders).
    #[test]
    fn local_rebuild_from_native_hash_matches_protocol_script() {
        let oracle_hash = OracleContract::new().hash().to_array();
        let mut builder = ScriptBuilder::new();
        builder.emit_opcode(OpCode::NEWARRAY0);
        builder.emit_push_int(neo_primitives::CallFlags::ALL.bits() as i64);
        builder.emit_push(b"finish");
        builder.emit_push(&oracle_hash);
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call interop name is valid");
        assert_eq!(builder.to_array(), get_fixed_script());
    }
}
