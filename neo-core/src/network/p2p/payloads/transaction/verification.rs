//
// verification.rs - Transaction verification logic
//

use super::*;

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

    /// Verifies the state-dependent part of the transaction.
    pub fn verify_state_dependent(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        context: Option<&crate::ledger::TransactionVerificationContext>,
        conflicts_list: &[Transaction],
    ) -> VerifyResult {
        let ledger = LedgerContract::new();
        let policy = PolicyContract::new();
        let height = ledger.current_index(snapshot).unwrap_or(0);
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

        let sign_data = self.get_sign_data(settings.network);

        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];

            if let Some(public_key) =
                Self::parse_single_signature_contract(&witness.verification_script)
            {
                if witness.script_hash() != *hash {
                    return VerifyResult::Invalid;
                }

                let Some(signature) =
                    Self::parse_single_signature_invocation(&witness.invocation_script)
                else {
                    return VerifyResult::Invalid;
                };

                let mut signature_bytes = [0u8; 64];
                signature_bytes.copy_from_slice(signature);

                let verified =
                    match Secp256r1Crypto::verify(&sign_data, &signature_bytes, public_key) {
                        Ok(result) => result,
                        Err(_) => return VerifyResult::Invalid,
                    };

                if !verified {
                    return VerifyResult::InvalidSignature;
                }

                net_fee_datoshi -= exec_fee_factor * Helper::signature_contract_cost();
            } else if let Some((m, public_keys)) =
                Helper::parse_multi_sig_contract(&witness.verification_script)
            {
                let Some(signatures) =
                    Helper::parse_multi_sig_invocation(&witness.invocation_script, m)
                else {
                    return VerifyResult::Invalid;
                };

                if witness.script_hash() != *hash {
                    return VerifyResult::Invalid;
                }

                if public_keys.is_empty() || signatures.len() != m {
                    return VerifyResult::Invalid;
                }

                let total_keys = public_keys.len();
                let mut sig_index = 0usize;
                let mut key_index = 0usize;

                while sig_index < m && key_index < total_keys {
                    let signature = &signatures[sig_index];
                    if signature.len() != 64 {
                        return VerifyResult::Invalid;
                    }

                    let mut signature_bytes = [0u8; 64];
                    signature_bytes.copy_from_slice(signature);

                    let verified = match Secp256r1Crypto::verify(
                        &sign_data,
                        &signature_bytes,
                        &public_keys[key_index],
                    ) {
                        Ok(result) => result,
                        Err(_) => return VerifyResult::Invalid,
                    };

                    if verified {
                        sig_index += 1;
                    }

                    key_index += 1;

                    if m.saturating_sub(sig_index) > total_keys.saturating_sub(key_index) {
                        return VerifyResult::InvalidSignature;
                    }
                }

                if sig_index != m {
                    return VerifyResult::InvalidSignature;
                }

                let n = public_keys.len() as i32;
                net_fee_datoshi -=
                    exec_fee_factor * Helper::multi_signature_contract_cost(m as i32, n);
            } else {
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

        if neo_vm::Script::new(self.script.clone(), true).is_err() {
            return VerifyResult::InvalidScript;
        }

        let hashes = self.get_script_hashes_for_verifying(&DataCache::new(true));
        if hashes.len() != self.witnesses.len() {
            return VerifyResult::Invalid;
        }

        let sign_data = self.get_sign_data(settings.network);

        for (i, hash) in hashes.iter().enumerate() {
            let witness = &self.witnesses[i];

            if Helper::is_signature_contract(&witness.verification_script) {
                if witness.verification_script.len() < 35 {
                    return VerifyResult::Invalid;
                }

                if witness.script_hash() != *hash {
                    return VerifyResult::Invalid;
                }

                let Some(signature) =
                    Self::parse_single_signature_invocation(&witness.invocation_script)
                else {
                    return VerifyResult::Invalid;
                };

                let mut signature_bytes = [0u8; 64];
                signature_bytes.copy_from_slice(signature);

                let pubkey = &witness.verification_script[2..35];
                let verified = match Secp256r1Crypto::verify(&sign_data, &signature_bytes, pubkey) {
                    Ok(result) => result,
                    Err(_) => return VerifyResult::Invalid,
                };

                if !verified {
                    return VerifyResult::InvalidSignature;
                }
            } else if let Some((m, public_keys)) =
                Helper::parse_multi_sig_contract(&witness.verification_script)
            {
                if witness.script_hash() != *hash {
                    return VerifyResult::Invalid;
                }

                let Some(signatures) =
                    Helper::parse_multi_sig_invocation(&witness.invocation_script, m)
                else {
                    return VerifyResult::Invalid;
                };

                if public_keys.is_empty() || signatures.len() != m {
                    return VerifyResult::Invalid;
                }

                let total_keys = public_keys.len();
                let mut sig_index = 0usize;
                let mut key_index = 0usize;

                while sig_index < m && key_index < total_keys {
                    let signature = &signatures[sig_index];
                    if signature.len() != 64 {
                        return VerifyResult::Invalid;
                    }

                    let mut signature_bytes = [0u8; 64];
                    signature_bytes.copy_from_slice(signature);

                    let verified = match Secp256r1Crypto::verify(
                        &sign_data,
                        &signature_bytes,
                        &public_keys[key_index],
                    ) {
                        Ok(result) => result,
                        Err(_) => return VerifyResult::Invalid,
                    };

                    if verified {
                        sig_index += 1;
                    }

                    key_index += 1;

                    if m.saturating_sub(sig_index) > total_keys.saturating_sub(key_index) {
                        return VerifyResult::InvalidSignature;
                    }
                }

                if sig_index != m {
                    return VerifyResult::InvalidSignature;
                }
            }
        }

        VerifyResult::Succeed
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

        if witness.script_hash() != *hash {
            return false;
        }

        let verification_gas = gas.min(Helper::MAX_VERIFICATION_GAS);

        if neo_vm::Script::new(witness.invocation_script.clone(), true).is_err() {
            return false;
        }

        let container: Arc<dyn crate::IVerifiable> = Arc::new(self.clone());
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
            if neo_vm::Script::new(verification_script.clone(), true).is_err() {
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
    pub(super) fn get_sign_data(&self, network: u32) -> Vec<u8> {
        match helper::get_sign_data_vec(self, network) {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("Failed to get sign data for transaction: {:?}", e);
                Vec::new()
            }
        }
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
        if invocation[0] != OpCode::PUSHDATA1 as u8 || invocation[1] != 0x40 {
            return None;
        }
        Some(&invocation[2..66])
    }
}
