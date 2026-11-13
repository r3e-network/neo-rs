use neo_native_contracts::{
    LedgerContract, NativeHelpers, PolicyContract, Role, RoleManagement,
    oracle_contract::OracleContract,
};
use neo_primitives::{UInt160, WitnessScope};
use neo_protocol::payloads::{OracleResponse, Transaction, TransactionAttribute, get_fixed_script};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::helper::Helper;
use neo_storage::persistence::DataCache;
use tracing::warn;

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

/// Returns the fixed script required by oracle response transactions.
///
/// Compatibility re-export of the single source of truth in
/// `neo_protocol::payloads::get_fixed_script`; oracle response verification
/// and every response-transaction producer share those exact bytes.
pub fn oracle_fixed_script() -> Vec<u8> {
    get_fixed_script()
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
