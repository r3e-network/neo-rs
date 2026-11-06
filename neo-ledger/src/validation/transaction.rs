use super::attribute::{attribute_network_fee, verify_attribute};
use super::transaction_context::TransactionVerificationContext;
use super::transaction_witness::{
    StandardWitnessVerification, verify_standard_witness, verify_transaction_witness,
};
use neo_io::Serializable;
use neo_native_contracts::{LedgerContract, PolicyContract};
use neo_primitives::VerifyResult;
use neo_protocol::hardfork::Hardfork;
use neo_protocol::payloads::transaction::MAX_TRANSACTION_SIZE;
use neo_protocol::payloads::{Transaction, TransactionAttributeType};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::signing_data;
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_smart_contract::helper::Helper;
use neo_storage::persistence::DataCache;

pub fn transaction_sign_data(transaction: &Transaction, network: u32) -> CoreResult<Vec<u8>> {
    signing_data::get_sign_data_vec(transaction, network)
        .map_err(|error| CoreError::invalid_operation(error.to_string()))
}

pub fn verify_transaction(
    transaction: &Transaction,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    context: Option<&TransactionVerificationContext>,
    conflicts_list: &[Transaction],
) -> VerifyResult {
    let result = verify_transaction_state_independent(transaction, settings);
    if result != VerifyResult::Succeed {
        return result;
    }
    verify_transaction_state_dependent(transaction, settings, snapshot, context, conflicts_list)
}

pub fn verify_transaction_state_dependent(
    transaction: &Transaction,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    context: Option<&TransactionVerificationContext>,
    conflicts_list: &[Transaction],
) -> VerifyResult {
    let height = LedgerContract::new().current_index(snapshot).unwrap_or(0);
    verify_transaction_state_dependent_at_height(
        transaction,
        settings,
        snapshot,
        height,
        context,
        conflicts_list,
    )
}

pub fn verify_transaction_state_dependent_at_height(
    transaction: &Transaction,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    current_height: u32,
    context: Option<&TransactionVerificationContext>,
    conflicts_list: &[Transaction],
) -> VerifyResult {
    let policy = PolicyContract::new();
    let height = current_height;
    let max_increment = policy
        .get_max_valid_until_block_increment_snapshot(snapshot, settings)
        .unwrap_or(settings.max_valid_until_block_increment);
    if transaction.valid_until_block() <= height {
        return VerifyResult::Expired;
    }
    if transaction.valid_until_block() > height.saturating_add(max_increment) {
        return VerifyResult::NotYetValid;
    }
    let hashes = transaction.signer_accounts();
    if hashes.len() != transaction.witnesses().len() {
        return VerifyResult::Invalid;
    }
    for hash in &hashes {
        if policy.is_blocked_snapshot(snapshot, hash).unwrap_or(false) {
            return VerifyResult::PolicyFail;
        }
    }
    if let Some(ctx) = context
        && !ctx.check_transaction(transaction, conflicts_list.iter(), snapshot)
    {
        return VerifyResult::InsufficientFunds;
    }
    let mut attributes_fee = 0i64;
    for attribute in transaction.attributes() {
        if attribute.type_id() == TransactionAttributeType::NotaryAssisted
            && !settings.is_hardfork_enabled(Hardfork::HfEchidna, height)
        {
            return VerifyResult::InvalidAttribute;
        }
        if !verify_attribute(attribute, settings, snapshot, transaction) {
            return VerifyResult::InvalidAttribute;
        }
        attributes_fee += attribute_network_fee(attribute, snapshot, transaction);
    }
    let fee_per_byte = policy
        .get_fee_per_byte_snapshot(snapshot)
        .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64);
    let mut net_fee_datoshi =
        transaction.network_fee() - (transaction.size() as i64 * fee_per_byte) - attributes_fee;
    if net_fee_datoshi < 0 {
        return VerifyResult::InsufficientFunds;
    }
    let max_verification_gas = Helper::MAX_VERIFICATION_GAS;
    if net_fee_datoshi > max_verification_gas {
        net_fee_datoshi = max_verification_gas;
    }
    let exec_fee_factor = policy
        .get_exec_fee_factor_snapshot(snapshot, settings, height)
        .unwrap_or(PolicyContract::DEFAULT_EXEC_FEE_FACTOR) as i64;
    let sign_data = match transaction_sign_data(transaction, settings.network) {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!("Failed to get transaction sign data: {:?}", error);
            return VerifyResult::Invalid;
        }
    };
    for (i, hash) in hashes.iter().enumerate() {
        let witness = &transaction.witnesses()[i];
        match verify_standard_witness(hash, witness, &sign_data) {
            Ok(StandardWitnessVerification::Verified {
                unscaled_verification_fee,
            }) => {
                net_fee_datoshi -= exec_fee_factor * unscaled_verification_fee;
            }
            Ok(StandardWitnessVerification::NonStandard) => {
                let mut fee = 0i64;
                if !verify_transaction_witness(
                    transaction,
                    settings,
                    snapshot,
                    hash,
                    witness,
                    net_fee_datoshi,
                    &mut fee,
                ) {
                    return VerifyResult::Invalid;
                }
                net_fee_datoshi -= fee;
            }
            Err(result) => return result,
        }
        if net_fee_datoshi < 0 {
            return VerifyResult::InsufficientFunds;
        }
    }
    VerifyResult::Succeed
}

pub fn verify_transaction_state_independent(
    transaction: &Transaction,
    settings: &ProtocolSettings,
) -> VerifyResult {
    if transaction.size() > MAX_TRANSACTION_SIZE {
        return VerifyResult::OverSize;
    }
    if neo_vm::validate_strict_script(transaction.script()).is_err() {
        return VerifyResult::InvalidScript;
    }
    let hashes = transaction.signer_accounts();
    if hashes.len() != transaction.witnesses().len() {
        return VerifyResult::Invalid;
    }
    let sign_data = match transaction_sign_data(transaction, settings.network) {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!("Failed to get transaction sign data: {:?}", error);
            return VerifyResult::Invalid;
        }
    };
    for (i, hash) in hashes.iter().enumerate() {
        let witness = &transaction.witnesses()[i];
        if let Err(result) = verify_standard_witness(hash, witness, &sign_data) {
            return result;
        }
    }
    VerifyResult::Succeed
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::{UInt160, WitnessScope};
    use neo_protocol::payloads::Signer;
    use neo_protocol::witness::Witness;
    use neo_vm::OpCode;

    fn transaction_with_oversized_script() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(0x0102_0304);
        tx.set_system_fee(1);
        tx.set_network_fee(100_000_000);
        tx.set_valid_until_block(42);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    #[test]
    fn get_sign_data_rejects_unserializable_transaction() {
        let tx = transaction_with_oversized_script();
        let settings = ProtocolSettings::default_settings();
        assert!(transaction_sign_data(&tx, settings.network).is_err());
    }

    #[test]
    fn helper_get_sign_data_vec_rejects_unserializable_transaction() {
        let tx = transaction_with_oversized_script();
        let settings = ProtocolSettings::default_settings();
        assert!(signing_data::get_sign_data_vec(&tx, settings.network).is_err());
    }

    #[test]
    fn verify_state_independent_rejects_unserializable_sign_data() {
        let tx = transaction_with_oversized_script();
        let settings = ProtocolSettings::default_settings();
        assert_eq!(
            verify_transaction_state_independent(&tx, &settings),
            VerifyResult::Invalid
        );
    }

    #[test]
    fn verify_state_dependent_rejects_unserializable_sign_data() {
        let tx = transaction_with_oversized_script();
        let settings = ProtocolSettings::default_settings();
        let snapshot = DataCache::new(true);
        assert_eq!(
            verify_transaction_state_dependent_at_height(&tx, &settings, &snapshot, 1, None, &[]),
            VerifyResult::Invalid,
        );
    }
}
