
use std::convert::TryInto;

/// A helper module related to wallets.
pub mod helper {
    use NeoRust::prelude::VarSizeTrait;
    use neo_vm::vm::VMState;
    use crate::contract::Policy;
    use crate::cryptography::{Base58, Crypto};
    use crate::neo_contract::application_engine::ApplicationEngine;
    use crate::neo_contract::call_flags::CallFlags;
    use crate::neo_contract::helper::helper::{multi_signature_contract_cost, signature_contract_cost};
    use crate::neo_contract::trigger_type::TriggerType;
    use crate::network::payloads::{IVerifiable, Transaction};
    use crate::persistence::DataCache;
    use crate::protocol_settings::ProtocolSettings;
    use crate::UInt160;
    use crate::wallet::KeyPair;
    use super::*;

    /// Signs an `IVerifiable` with the specified private key.
    ///
    /// # Arguments
    ///
    /// * `verifiable` - The `IVerifiable` to sign.
    /// * `key` - The private key to be used.
    /// * `network` - The magic number of the NEO network.
    ///
    /// # Returns
    ///
    /// The signature for the `IVerifiable`.
    pub fn sign(verifiable: &impl IVerifiable, key: &KeyPair, network: u32) -> Vec<u8> {
        Crypto::sign(&verifiable.get_sign_data(network), &key.private_key())
    }

    /// Converts the specified script hash to an address.
    ///
    /// # Arguments
    ///
    /// * `script_hash` - The script hash to convert.
    /// * `version` - The address version.
    ///
    /// # Returns
    ///
    /// The converted address.
    pub fn to_address(script_hash: &UInt160, version: u8) -> String {
        let mut data = vec![version];
        data.extend_from_slice(script_hash.to_array());
        Base58::base58_check_encode(&data)
    }

    /// Converts the specified address to a script hash.
    ///
    /// # Arguments
    ///
    /// * `address` - The address to convert.
    /// * `version` - The address version.
    ///
    /// # Returns
    ///
    /// The converted script hash.
    pub fn to_script_hash(address: &str, version: u8) -> Result<UInt160, String> {
        let data = Base58::base58_check_decode(address).map_err(|e| e.to_string())?;
        if data.len() != 21 {
            return Err("Invalid address length".into());
        }
        if data[0] != version {
            return Err("Invalid address version".into());
        }
        Ok(UInt160::try_from(&data[1..]).unwrap())
    }

    /// XOR operation on two byte arrays.
    pub(crate) fn xor(x: &[u8], y: &[u8]) -> Result<Vec<u8>, String> {
        if x.len() != y.len() {
            return Err("Arrays must have the same length".into());
        }
        Ok(x.iter().zip(y.iter()).map(|(&a, &b)| a ^ b).collect())
    }

    /// Calculates the network fee for the specified transaction.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    ///
    /// # Arguments
    ///
    /// * `tx` - The transaction to calculate.
    /// * `snapshot` - The snapshot used to read data.
    /// * `settings` - The protocol settings to use.
    /// * `account_script` - Function to retrieve the script's account from a hash.
    /// * `max_execution_cost` - The maximum cost that can be spent when a contract is executed.
    ///
    /// # Returns
    ///
    /// The network fee of the transaction.
    pub fn calculate_network_fee(
        tx: &Transaction,
        snapshot: &dyn DataCache,
        settings: &ProtocolSettings,
        account_script: impl Fn(&UInt160) -> Option<Vec<u8>>,
        max_execution_cost: i64,
    ) -> Result<i64, String> {
        let hashes = tx.get_script_hashes_for_verifying(snapshot)?;

        // base size for transaction: includes const_header + signers + attributes + script + hashes
        let mut size = Transaction::HEADER_SIZE
            + tx.signers.var_size()
            + tx.attributes.var_size()
            + tx.script.var_size()
            + io::helper::var_size(hashes.len() as u64);
        let exec_fee_factor = Policy::get_exec_fee_factor(snapshot);
        let mut network_fee = 0;

        for (index, hash) in hashes.iter().enumerate() {
            let witness_script = account_script(hash);
            let mut invocation_script = None;

            if tx.witnesses.is_some() && witness_script.is_none() {
                // Try to find the script in the witnesses
                if let Some(witness) = tx.witnesses.as_ref().unwrap().get(index) {
                    let verification_script = witness.verification_script.to_vec();
                    if verification_script.is_empty() {
                        // Then it's a contract-based witness, so try to get the corresponding invocation script for it
                        invocation_script = Some(witness.invocation_script.to_vec());
                    } else {
                        witness_script = Some(verification_script);
                    }
                }
            }

            if witness_script.is_none() || witness_script.as_ref().unwrap().is_empty() {
                let contract = NativeContract::ContractManagement::get_contract(snapshot, hash)
                    .ok_or_else(|| format!("The smart contract or address {} ({}) is not found. If this is your wallet address and you want to sign a transaction with it, make sure you have opened this wallet.", hash, to_address(hash, settings.address_version)))?;
                let md = contract.manifest.abi.get_method("verify", 0)
                    .ok_or_else(|| format!("The smart contract {} hasn't got verify method", contract.hash))?;
                if md.return_type != ContractParameterType::Boolean {
                    return Err("The verify method doesn't return boolean value.".into());
                }
                if !md.parameters.is_empty() && invocation_script.is_none() {
                    return Err("The verify method requires parameters that need to be passed via the witness' invocation script.".into());
                }

                // Empty verification and non-empty invocation scripts
                let inv_size = invocation_script.as_ref().map_or(0, |s| s.var_size());
                size += Vec::<u8>::new().var_size() + inv_size;

                // Check verify cost
                let mut engine = ApplicationEngine::new(TriggerType::VERIFICATION, tx, snapshot.clone_cache(), settings, max_execution_cost);
                engine.load_contract(&contract, md, CallFlags::READ_ONLY);
                if let Some(script) = invocation_script {
                    engine.load_script(&script, |s| s.call_flags = CallFlags::NONE);
                }
                if engine.execute()? == VMState::Fault {
                    return Err(format!("Smart contract {} verification fault.", contract.hash));
                }
                if !engine.result_stack.pop().unwrap().get_boolean()? {
                    return Err(format!("Smart contract {} returns false.", contract.hash));
                }

                max_execution_cost -= engine.fee_consumed;
                if max_execution_cost <= 0 {
                    return Err("Insufficient GAS.".into());
                }
                network_fee += engine.fee_consumed;
            } else if is_signature_contract(witness_script.as_ref().unwrap()) {
                size += 67 + witness_script.as_ref().unwrap().var_size();
                network_fee += exec_fee_factor * signature_contract_cost();
            } else if let Some((m, n)) = is_multi_sig_contract(witness_script.as_ref().unwrap()) {
                let size_inv = 66 * m;
                size += io::helper::var_size(size_inv as u64) + size_inv + witness_script.as_ref().unwrap().var_size();
                network_fee += exec_fee_factor * multi_signature_contract_cost(m, n);
            }
            // We can support more contract vm_types in the future.
        }
        network_fee += size as i64 * Policy::get_fee_per_byte(snapshot);
        for attr in &tx.attributes {
            network_fee += attr.calculate_network_fee(snapshot, tx)?;
        }
        Ok(network_fee)
    }
}
