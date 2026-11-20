use neo_native_contracts::{LedgerContract, NativeHelpers, oracle_contract::OracleContract};
use neo_primitives::UInt160;
use neo_protocol::payloads::{OracleResponse, Transaction, TransactionAttribute, get_fixed_script};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::helper::Helper;
use neo_storage::persistence::PersistenceDataCache;
use tracing::warn;

pub fn verify_attribute(
    attribute: &TransactionAttribute,
    settings: &ProtocolSettings,
    snapshot: &PersistenceDataCache,
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

/// Delegates to the canonical attribute-fee policy in `neo-native-contracts`.
///
/// Fee estimation and validation must agree on what an attribute costs, so the
/// scaling rules live with the `PolicyContract` owner and this entry point only
/// forwards to them.
pub fn attribute_network_fee(
    attribute: &TransactionAttribute,
    snapshot: &PersistenceDataCache,
    transaction: &Transaction,
) -> i64 {
    neo_native_contracts::attribute_network_fee(attribute, snapshot, transaction)
}

/// Returns the fixed script required by oracle response transactions.
///
/// Compatibility re-export of the single source of truth in
/// `neo_protocol::payloads::get_fixed_script`; oracle response verification
/// and every response-transaction producer share those exact bytes.
pub fn oracle_fixed_script() -> Vec<u8> {
    get_fixed_script()
}

/// Delegates to the canonical oracle-response verifier in `neo-native-contracts`.
///
/// The `Oracle.verify` native method and ledger attribute validation must accept
/// exactly the same responses, so the algorithm has a single owner
/// (`OracleContract::verify_oracle_response`) and this entry point only forwards
/// to it.
fn verify_oracle_response(
    attribute: &OracleResponse,
    snapshot: &PersistenceDataCache,
    transaction: &Transaction,
) -> bool {
    OracleContract::verify_oracle_response(attribute, snapshot, transaction)
}
