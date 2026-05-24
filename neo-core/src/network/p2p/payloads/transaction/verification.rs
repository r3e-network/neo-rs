//
// verification.rs - Transaction verification logic
//

use super::*;

enum StandardWitnessVerification {
    NonStandard,
    Verified { unscaled_verification_fee: i64 },
}

impl Transaction {
    /// Verifies the transaction.
    pub fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult {
        let result = self.verify_state_independent(settings);
        if result != VerifyResult::Succeed {
            return result;
        }
        self.verify_state_dependent(settings, snapshot, context, conflicts_list)
    }

    /// Verifies the state-dependent part of the transaction by reading the
    /// current height from the snapshot. Use this for mempool admission /
    /// reverify; for block verification prefer `verify_state_dependent_at_height`
    /// (passing `block.index() - 1`) — during fast-sync the snapshot's
    /// `current_index` can spuriously read 0 (memtable visibility / WAL-disabled
    /// interaction), causing every legit tx with `valid_until_block > 5760` to
    /// be wrongly rejected as Expired.
    pub fn verify_state_dependent(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult {
        let height = LedgerContract::new().current_index(snapshot).unwrap_or(0);
        self.verify_state_dependent_at_height(settings, snapshot, height, context, conflicts_list)
    }

    /// Verifies the state-dependent part of the transaction with an explicit
    /// `current_height`. Pass `block.index() - 1` when verifying for inclusion
    /// in block N.
    pub fn verify_state_dependent_at_height(
        &self,
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
        if self.valid_until_block <= height || self.valid_until_block > height + max_increment {
            return VerifyResult::Expired;
        }

        let hashes = self.get_script_hashes_for_verifying(snapshot);
        if hashes.len() != self.witnesses.len() {
            return VerifyResult::Invalid;
        }
        for hash in &hashes {
            if policy.is_blocked_snapshot(snapshot, hash).unwrap_or(false) {
                return VerifyResult::PolicyFail;
            }
        }

        if let Some(ctx) = context {
            if !ctx.check_transaction(self, conflicts_list.iter(), snapshot) {
                return VerifyResult::InsufficientFunds;
            }
        }

        let mut attributes_fee = 0i64;
        for attribute in &self.attributes {
            if attribute.get_type() == TransactionAttributeType::NotaryAssisted
                && !settings.is_hardfork_enabled(Hardfork::HfEchidna, height)
            {
                return VerifyResult::InvalidAttribute;
            }
            if !attribute.verify(settings, snapshot, self) {
                return VerifyResult::InvalidAttribute;
            }
            attributes_fee += attribute.calculate_network_fee(snapshot, self);
        }

        let fee_per_byte = policy
            .get_fee_per_byte_snapshot(snapshot)
            .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64);
        let mut net_fee_datoshi =
            self.network_fee - (self.size() as i64 * fee_per_byte) - attributes_fee;

        if net_fee_datoshi < 0 {
            return VerifyResult::InsufficientFunds;
        }

        let max_verification_gas = Helper::MAX_VERIFICATION_GAS;
        if net_fee_datoshi > max_verification_gas {
            net_fee_datoshi = max_verification_gas;
        }

        let exec_fee_factor = policy
            .get_exec_fee_factor_snapshot(snapshot, settings, height)
            .unwrap_or(PolicyContract::DEFAULT_EXEC_FEE_FACTOR)
            as i64;

        let sign_data = match self.get_sign_data(settings.network) {
            Ok(data) => data,
            Err(error) => {
                tracing::warn!("Failed to get transaction sign data: {:?}", error);
                return VerifyResult::Invalid;
            }
        };

        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];

            match Self::verify_standard_witness(hash, witness, &sign_data) {
                Ok(StandardWitnessVerification::Verified {
                    unscaled_verification_fee,
                }) => {
                    net_fee_datoshi -= exec_fee_factor * unscaled_verification_fee;
                }
                Ok(StandardWitnessVerification::NonStandard) => {
                    let mut fee = 0i64;
                    if !self.verify_witness(
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
    pub fn verify_state_independent(&self, settings: &ProtocolSettings) -> VerifyResult {
        if self.size() > MAX_TRANSACTION_SIZE {
            return VerifyResult::OverSize;
        }

        if crate::script_validation::validate_strict_script(&self.script).is_err() {
            return VerifyResult::InvalidScript;
        }

        let hashes = self.get_script_hashes_for_verifying(&DataCache::new(true));
        if hashes.len() != self.witnesses.len() {
            return VerifyResult::Invalid;
        }

        let sign_data = match self.get_sign_data(settings.network) {
            Ok(data) => data,
            Err(error) => {
                tracing::warn!("Failed to get transaction sign data: {:?}", error);
                return VerifyResult::Invalid;
            }
        };

        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];

            if let Err(result) = Self::verify_standard_witness(hash, witness, &sign_data) {
                return result;
            }
        }

        VerifyResult::Succeed
    }

    fn verify_standard_witness(
        hash: &UInt160,
        witness: &Witness,
        sign_data: &[u8],
    ) -> Result<StandardWitnessVerification, VerifyResult> {
        if let Some(public_key) =
            Self::parse_single_signature_contract(&witness.verification_script)
        {
            if witness.script_hash() != *hash {
                return Err(VerifyResult::Invalid);
            }

            let Some(signature) =
                Self::parse_single_signature_invocation(&witness.invocation_script)
            else {
                return Err(VerifyResult::Invalid);
            };

            let mut signature_bytes = [0u8; 64];
            signature_bytes.copy_from_slice(signature);

            let verified = Secp256r1Crypto::verify(sign_data, &signature_bytes, public_key)
                .map_err(|_| VerifyResult::Invalid)?;

            if !verified {
                return Err(VerifyResult::InvalidSignature);
            }

            return Ok(StandardWitnessVerification::Verified {
                unscaled_verification_fee: Helper::signature_contract_cost(),
            });
        }

        let Some((m, public_keys)) = Helper::parse_multi_sig_contract(&witness.verification_script)
        else {
            return Ok(StandardWitnessVerification::NonStandard);
        };

        if witness.script_hash() != *hash {
            return Err(VerifyResult::Invalid);
        }

        let Some(signatures) = Helper::parse_multi_sig_invocation(&witness.invocation_script, m)
        else {
            return Err(VerifyResult::Invalid);
        };

        if public_keys.is_empty() || signatures.len() != m {
            return Err(VerifyResult::Invalid);
        }

        let total_keys = public_keys.len();
        let mut sig_index = 0usize;
        let mut key_index = 0usize;

        while sig_index < m && key_index < total_keys {
            let signature = &signatures[sig_index];
            if signature.len() != 64 {
                return Err(VerifyResult::Invalid);
            }

            let mut signature_bytes = [0u8; 64];
            signature_bytes.copy_from_slice(signature);

            let verified =
                Secp256r1Crypto::verify(sign_data, &signature_bytes, &public_keys[key_index])
                    .map_err(|_| VerifyResult::Invalid)?;

            if verified {
                sig_index += 1;
            }

            key_index += 1;

            if m.saturating_sub(sig_index) > total_keys.saturating_sub(key_index) {
                return Err(VerifyResult::InvalidSignature);
            }
        }

        if sig_index != m {
            return Err(VerifyResult::InvalidSignature);
        }

        let n = public_keys.len() as i32;
        Ok(StandardWitnessVerification::Verified {
            unscaled_verification_fee: Helper::multi_signature_contract_cost(m as i32, n),
        })
    }

    /// Verify a single witness.
    pub(super) fn verify_witness(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        hash: &UInt160,
        witness: &Witness,
        gas: i64,
        fee: &mut i64,
    ) -> bool {
        *fee = 0;

        if gas < 0 {
            return false;
        }

        // The script_hash != hash check belongs in the non-empty branch only.
        // Empty verification scripts (Oracle response txs, native-contract
        // signers) are valid: they trigger the contract-lookup path below.
        // C# Helper.cs:334-345 has this ordering — checking script_hash up front
        // wrongly rejects valid Oracle response txs whose first witness is empty
        // because the Oracle native contract verifies via OracleResponse.Verify.

        let verification_gas = gas.min(Helper::MAX_VERIFICATION_GAS);

        if crate::script_validation::validate_strict_script(&witness.invocation_script).is_err() {
            return false;
        }

        let container: Arc<dyn crate::Verifiable> = Arc::new(self.clone());
        let snapshot_clone = Arc::new(snapshot.clone());

        let mut engine = match ApplicationEngine::new(
            TriggerType::Verification,
            Some(container),
            snapshot_clone,
            None,
            settings.clone(),
            verification_gas,
            None,
        ) {
            Ok(engine) => engine,
            Err(_) => return false,
        };

        let verification_script = witness.verification_script.clone();

        if verification_script.is_empty() {
            let contract = match ContractManagement::get_contract_from_snapshot(snapshot, hash) {
                Ok(Some(contract)) => contract,
                _ => return false,
            };

            let mut abi = contract.manifest.abi.clone();
            let method = match abi.get_method(
                ContractBasicMethod::VERIFY,
                ContractBasicMethod::VERIFY_P_COUNT,
            ) {
                Some(descriptor) => descriptor.clone(),
                None => return false,
            };

            if method.return_type != ContractParameterType::Boolean {
                return false;
            }

            if engine
                .load_contract_method(contract, method, CallFlags::READ_ONLY)
                .is_err()
            {
                return false;
            }
        } else {
            // C# Helper.cs:344-345: native contracts must use empty verification
            // script (their verification logic is built-in); reject any non-empty
            // verification script for native-contract signers, AND verify the
            // script hash matches the signer hash.
            if witness.script_hash() != *hash {
                return false;
            }
            if crate::script_validation::validate_strict_script(&verification_script).is_err() {
                return false;
            }
            if engine
                .load_script(verification_script, CallFlags::READ_ONLY, Some(*hash))
                .is_err()
            {
                return false;
            }
        }

        if engine
            .load_script(witness.invocation_script.clone(), CallFlags::NONE, None)
            .is_err()
        {
            return false;
        }

        if engine.execute().is_err() {
            return false;
        }

        if engine.result_stack().len() != 1 {
            return false;
        }

        let Ok(result_item) = engine.result_stack().peek(0) else {
            return false;
        };

        match result_item.get_boolean() {
            Ok(true) => {
                *fee = engine.fee_consumed();
                true
            }
            _ => false,
        }
    }

    /// Get signature data for the transaction.
    pub(super) fn get_sign_data(&self, network: u32) -> CoreResult<Vec<u8>> {
        helper::get_sign_data_vec(self, network)
    }

    pub(super) fn parse_single_signature_contract(script: &[u8]) -> Option<&[u8]> {
        if !Helper::is_signature_contract(script) {
            return None;
        }
        // Signature contract pattern: PUSHDATA1(33) <33-byte pubkey> SYSCALL <CheckSig hash>
        Some(&script[2..35])
    }

    pub(super) fn parse_single_signature_invocation(invocation: &[u8]) -> Option<&[u8]> {
        if invocation.len() != 66 {
            return None;
        }
        if invocation[0] != OpCode::PUSHDATA1.byte() || invocation[1] != 0x40 {
            return None;
        }
        Some(&invocation[2..66])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WitnessScope;

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
