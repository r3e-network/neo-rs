use neo_native_contracts::{
    LedgerContract, NativeContract, NativeHelpers, PolicyContract, Role, RoleManagement,
    oracle_contract::OracleContract,
};
use neo_primitives::{CallFlags, UInt160, WitnessScope};
use neo_protocol::payloads::{OracleResponse, Transaction, TransactionAttribute};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::helper::Helper;
use neo_storage::persistence::DataCache;
use neo_vm::{OpCode, ScriptBuilder};
use tracing::{error, warn};

pub fn verify_attribute(
    attribute: &TransactionAttribute,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    transaction: &Transaction,
) -> bool {
    match attribute {
        TransactionAttribute::HighPriority => {
            if settings.standby_committee.is_empty() {
                return false;
            }
            let address = NativeHelpers::committee_address(settings, Some(snapshot));
            transaction
                .signers()
                .iter()
                .any(|signer| signer.account == address)
        }
        TransactionAttribute::Conflicts(attribute) => {
            match LedgerContract::new().contains_transaction(snapshot, &attribute.hash) {
                Ok(exists) => !exists,
                Err(err) => {
                    warn!(target: "neo", hash = %attribute.hash, error = %err, "failed to verify conflicts attribute against ledger");
                    false
                }
            }
        }
        TransactionAttribute::NotValidBefore(attribute) => {
            let current_height = LedgerContract::new().current_index(snapshot).unwrap_or(0);
            current_height >= attribute.height
        }
        TransactionAttribute::NotaryAssisted(_) => {
            let notary_hash = Helper::get_contract_hash(&UInt160::zero(), 0, "Notary");
            if transaction.sender() == Some(notary_hash) {
                return transaction.signers().len() == 2;
            }
            transaction
                .signers()
                .iter()
                .any(|signer| signer.account == notary_hash)
        }
        TransactionAttribute::OracleResponse(attribute) => {
            verify_oracle_response(attribute, snapshot, transaction)
        }
    }
}

pub fn attribute_network_fee(
    attribute: &TransactionAttribute,
    snapshot: &DataCache,
    transaction: &Transaction,
) -> i64 {
    let policy = PolicyContract::new();
    let base_fee = policy
        .get_attribute_fee_snapshot(snapshot, attribute.type_id())
        .unwrap_or(PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64);
    match attribute {
        TransactionAttribute::Conflicts(_) => transaction.signers().len() as i64 * base_fee,
        TransactionAttribute::NotaryAssisted(attribute) => (attribute.nkeys as i64 + 1) * base_fee,
        _ => base_fee,
    }
}

pub fn oracle_fixed_script() -> Vec<u8> {
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

fn verify_oracle_response(
    attribute: &OracleResponse,
    snapshot: &DataCache,
    transaction: &Transaction,
) -> bool {
    if transaction
        .signers()
        .iter()
        .any(|signer| signer.scopes != WitnessScope::NONE)
    {
        return false;
    }
    if transaction.script() != oracle_fixed_script() {
        return false;
    }
    let oracle = OracleContract::new();
    let Ok(Some(request)) = oracle.get_request(snapshot, attribute.id) else {
        return false;
    };
    let Some(total_fee) = transaction
        .network_fee()
        .checked_add(transaction.system_fee())
    else {
        return false;
    };
    if total_fee != request.gas_for_response {
        return false;
    }
    let Ok(current_index) = LedgerContract::new().current_index(snapshot) else {
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
    transaction
        .signers()
        .iter()
        .any(|signer| signer.account == oracle_account)
}
