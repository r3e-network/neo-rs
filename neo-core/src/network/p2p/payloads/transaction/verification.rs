//
// verification.rs - Transaction verification logic
//

use super::witness_verification::{
    StandardWitnessVerification, verify_standard_witness, verify_transaction_witness,
};
use super::{MAX_TRANSACTION_SIZE, Transaction, transaction_to_stack_value};
use crate::hardfork::Hardfork;
use crate::ledger::VerifyResult;
use crate::network::p2p::helper;
use crate::network::p2p::payloads::{TransactionAttributeExt, TransactionAttributeType};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::native::{LedgerContract, PolicyContract};
use crate::witness::Witness;
use neo_io_crate::Serializable;
use neo_primitives::UInt160;
use neo_smart_contract::error::CoreResult;
use neo_vm::StackValue;

/// Snapshot / VM dependent transaction operations that neo-core adds on top of
/// the canonical `neo-protocol` [`Transaction`].
///
/// The structural API (getters/setters, hashing, wire serialization, JSON,
/// `neo_primitives::Verifiable`) lives on the re-exported `Transaction` itself;
/// this extension trait carries the neo-core-only verification and VM
/// projection helpers, mirroring the `WitnessExt` / `TransactionAttributeExt`
/// pattern used for the other unified leaf types.
pub trait TransactionExt {
    /// Verifies the transaction against a snapshot.
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult;

    /// Verifies against the current snapshot height for mempool admission.
    fn verify_state_dependent(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult;

    /// Verifies for block inclusion using its explicit predecessor height.
    fn verify_state_dependent_at_height(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        current_height: u32,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult;

    /// Verifies the state-independent part of the transaction.
    fn verify_state_independent(&self, settings: &ProtocolSettings) -> VerifyResult;

    /// Converts the transaction to a neo-vm-rs stack value.
    fn to_stack_value(&self) -> CoreResult<StackValue>;

    /// Gets the signature data for the transaction.
    fn get_sign_data(&self, network: u32) -> CoreResult<Vec<u8>>;

    /// Verifies a single witness against the transaction.
    fn verify_witness(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        hash: &UInt160,
        witness: &Witness,
        gas: i64,
        fee: &mut i64,
    ) -> bool;
}

/// Verifies the transaction.
pub fn verify_transaction(
    transaction: &Transaction,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    context: Option<&crate::ledger::TransactionVerificationContext>,
    conflicts_list: &[Transaction],
) -> VerifyResult {
    let result = verify_transaction_state_independent(transaction, settings);
    if result != VerifyResult::Succeed {
        return result;
    }
    verify_transaction_state_dependent(transaction, settings, snapshot, context, conflicts_list)
}

/// Verifies the state-dependent part of the transaction by reading the
/// current height from the snapshot. Use this for mempool admission /
/// reverify; for block verification prefer `verify_state_dependent_at_height`
/// (passing `block.index() - 1`) — during fast-sync the snapshot's
/// `current_index` can spuriously read 0 (memtable visibility / WAL-disabled
/// interaction), causing every legit tx with `valid_until_block > 5760` to
/// be wrongly rejected as Expired.
pub fn verify_transaction_state_dependent(
    transaction: &Transaction,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    context: Option<&crate::ledger::TransactionVerificationContext>,
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

/// Verifies the state-dependent part of the transaction with an explicit
/// `current_height`. Pass `block.index() - 1` when verifying for inclusion
/// in block N.
pub fn verify_transaction_state_dependent_at_height(
    transaction: &Transaction,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    current_height: u32,
    context: Option<&crate::ledger::TransactionVerificationContext>,
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
        if !attribute.verify(settings, snapshot, transaction) {
            return VerifyResult::InvalidAttribute;
        }
        attributes_fee += attribute.calculate_network_fee(snapshot, transaction);
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

    let sign_data = match helper::get_sign_data_vec(transaction, settings.network) {
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

/// Verifies the state-independent part of the transaction.
pub fn verify_transaction_state_independent(
    transaction: &Transaction,
    settings: &ProtocolSettings,
) -> VerifyResult {
    if transaction.size() > MAX_TRANSACTION_SIZE {
        return VerifyResult::OverSize;
    }

    if crate::script_validation::validate_strict_script(transaction.script()).is_err() {
        return VerifyResult::InvalidScript;
    }

    let hashes = transaction.signer_accounts();
    if hashes.len() != transaction.witnesses().len() {
        return VerifyResult::Invalid;
    }

    let sign_data = match helper::get_sign_data_vec(transaction, settings.network) {
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

impl TransactionExt for Transaction {
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult {
        verify_transaction(self, settings, snapshot, context, conflicts_list)
    }

    fn verify_state_dependent(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult {
        verify_transaction_state_dependent(self, settings, snapshot, context, conflicts_list)
    }

    fn verify_state_dependent_at_height(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        current_height: u32,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult {
        verify_transaction_state_dependent_at_height(
            self,
            settings,
            snapshot,
            current_height,
            context,
            conflicts_list,
        )
    }

    fn verify_state_independent(&self, settings: &ProtocolSettings) -> VerifyResult {
        verify_transaction_state_independent(self, settings)
    }

    fn to_stack_value(&self) -> CoreResult<StackValue> {
        transaction_to_stack_value(self)
    }

    fn get_sign_data(&self, network: u32) -> CoreResult<Vec<u8>> {
        helper::get_sign_data_vec(self, network)
    }

    fn verify_witness(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        hash: &UInt160,
        witness: &Witness,
        gas: i64,
        fee: &mut i64,
    ) -> bool {
        verify_transaction_witness(self, settings, snapshot, hash, witness, gas, fee)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::Signer;
    use crate::witness::Witness;
    use crate::{UInt160, WitnessScope};
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

        assert!(tx.get_sign_data(settings.network).is_err());
    }

    #[test]
    fn helper_get_sign_data_vec_rejects_unserializable_transaction() {
        let tx = transaction_with_oversized_script();
        let settings = ProtocolSettings::default_settings();

        assert!(helper::get_sign_data_vec(&tx, settings.network).is_err());
    }

    #[test]
    fn verify_state_independent_rejects_unserializable_sign_data() {
        let tx = transaction_with_oversized_script();
        let settings = ProtocolSettings::default_settings();

        assert_eq!(
            tx.verify_state_independent(&settings),
            VerifyResult::Invalid
        );
    }

    #[test]
    fn verify_state_dependent_rejects_unserializable_sign_data() {
        let tx = transaction_with_oversized_script();
        let settings = ProtocolSettings::default_settings();
        let snapshot = DataCache::new(true);

        assert_eq!(
            tx.verify_state_dependent_at_height(&settings, &snapshot, 1, None, &[]),
            VerifyResult::Invalid
        );
    }
}
