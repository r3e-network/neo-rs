use super::resolver::LedgerWitnessHashResolver;
use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_native_contracts::{ContractManagement, composition::standard_native_services};
use neo_primitives::{
    CallFlags, ContractBasicMethod, ContractParameterType, TriggerType, UInt160, Verifiable,
    VerifyResult,
};
use neo_protocol::payloads::Transaction;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::Witness;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::helper::Helper;
use neo_storage::persistence::DataCache;
use neo_vm::validate_strict_script;
use std::sync::Arc;

pub enum StandardWitnessVerification {
    NonStandard,
    Verified { unscaled_verification_fee: i64 },
}

pub fn verify_standard_witness(
    hash: &UInt160,
    witness: &Witness,
    sign_data: &[u8],
) -> Result<StandardWitnessVerification, VerifyResult> {
    if let Some(public_key) = parse_single_signature_contract(witness.verification_script()) {
        if witness.script_hash() != *hash {
            return Err(VerifyResult::Invalid);
        }
        let Some(signature) = parse_single_signature_invocation(witness.invocation_script()) else {
            // R04: a standard contract with a non-quick-format invocation must use the VM path.
            return Ok(StandardWitnessVerification::NonStandard);
        };
        let mut signature_bytes = [0u8; 64];
        signature_bytes.copy_from_slice(signature);
        let verified = Secp256r1Crypto::verify_prehash(
            &Crypto::sha256(sign_data),
            &signature_bytes,
            public_key,
        )
        .map_err(|_| VerifyResult::Invalid)?;
        if !verified {
            return Err(VerifyResult::InvalidSignature);
        }
        return Ok(StandardWitnessVerification::Verified {
            unscaled_verification_fee: Helper::signature_contract_cost(),
        });
    }
    let Some((m, public_keys)) = Helper::parse_multi_sig_contract(witness.verification_script())
    else {
        return Ok(StandardWitnessVerification::NonStandard);
    };
    if witness.script_hash() != *hash {
        return Err(VerifyResult::Invalid);
    }
    let Some(signatures) = Helper::parse_multi_sig_invocation(witness.invocation_script(), m)
    else {
        // R04: invocation formats outside the quick checker defer to full VM execution.
        return Ok(StandardWitnessVerification::NonStandard);
    };
    if public_keys.is_empty() || signatures.len() != m {
        return Err(VerifyResult::Invalid);
    }
    let total_keys = public_keys.len();
    // Every ordered slot verifies against the same prehash, so hash the signing
    // bytes once instead of re-hashing them for each candidate public key.
    let sign_hash = Crypto::sha256(sign_data);
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
            Secp256r1Crypto::verify_prehash(&sign_hash, &signature_bytes, &public_keys[key_index])
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

pub fn verify_transaction_witness(
    transaction: &Transaction,
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
    let verification_gas = gas.min(Helper::MAX_VERIFICATION_GAS);
    if validate_strict_script(witness.invocation_script()).is_err() {
        return false;
    }
    let container: Arc<dyn Verifiable> = Arc::new(transaction.clone());
    let snapshot_clone = Arc::new(snapshot.clone());
    let Ok(mut engine) = ApplicationEngine::new(
        TriggerType::Verification,
        Some(container),
        snapshot_clone,
        None,
        settings.clone(),
        verification_gas,
        None,
        standard_native_services(Arc::new(LedgerWitnessHashResolver)),
    ) else {
        return false;
    };
    let verification_script = witness.verification_script().to_vec();
    if verification_script.is_empty() {
        let Ok(Some(contract)) = ContractManagement::get_contract_from_snapshot(snapshot, hash)
        else {
            return false;
        };
        let mut abi = contract.manifest.abi.clone();
        let Some(method) = abi
            .get_method(
                ContractBasicMethod::VERIFY,
                ContractBasicMethod::VERIFY_P_COUNT,
            )
            .cloned()
        else {
            return false;
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
        // Native verification is only valid with an empty verification script.
        if engine.is_native_contract_hash(hash) {
            return false;
        }
        if witness.script_hash() != *hash {
            return false;
        }
        if validate_strict_script(&verification_script).is_err() {
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
        .load_script(witness.invocation_script().to_vec(), CallFlags::NONE, None)
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
    match result_item.as_boolean() {
        Ok(true) => {
            *fee = engine.fee_consumed();
            true
        }
        _ => false,
    }
}

pub fn parse_single_signature_contract(script: &[u8]) -> Option<&[u8]> {
    if !Helper::is_signature_contract(script) {
        return None;
    }
    Some(&script[2..35])
}

/// Extracts the 64-byte signature from a single-signature invocation script.
///
/// The layout is owned by `neo-vm`; this stays as the public ledger path.
pub fn parse_single_signature_invocation(invocation: &[u8]) -> Option<&[u8]> {
    neo_vm::script::parse_single_signature_invocation(invocation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::Secp256r1Crypto;
    use neo_protocol::witness::Witness;
    use neo_smart_contract::helper::Helper;
    use neo_vm::OpCode;

    /// Key pairs ordered by compressed public key, which is the order a multi-sig
    /// redeem script lays its keys out in.
    fn ordered_key_pairs(count: usize) -> Vec<([u8; 32], Vec<u8>)> {
        let mut pairs: Vec<([u8; 32], Vec<u8>)> = (0..count)
            .map(|_| {
                let private_key = Secp256r1Crypto::generate_private_key();
                let public_key =
                    Secp256r1Crypto::derive_public_key(&private_key).expect("derive public key");
                (private_key, public_key)
            })
            .collect();
        pairs.sort_by(|left, right| left.1.cmp(&right.1));
        pairs
    }

    /// Builds the quick-checker invocation layout: one `PUSHDATA1` push per signature.
    fn multi_sig_invocation(signatures: &[[u8; 64]]) -> Vec<u8> {
        let mut invocation = Vec::with_capacity(signatures.len() * 66);
        for signature in signatures {
            invocation.push(OpCode::PUSHDATA1.byte());
            invocation.push(64);
            invocation.extend_from_slice(signature);
        }
        invocation
    }

    fn sign_slots(
        pairs: &[([u8; 32], Vec<u8>)],
        slots: &[usize],
        digest: &[u8; 32],
    ) -> Vec<[u8; 64]> {
        slots
            .iter()
            .map(|slot| Secp256r1Crypto::sign_prehash(digest, &pairs[*slot].0).expect("sign"))
            .collect()
    }

    /// Returns the key pairs, redeem script, and account hash of a 2-of-3 contract.
    fn two_of_three() -> (Vec<([u8; 32], Vec<u8>)>, Vec<u8>, UInt160) {
        let pairs = ordered_key_pairs(3);
        let public_keys: Vec<Vec<u8>> = pairs.iter().map(|(_, key)| key.clone()).collect();
        let verification_script = Helper::multi_sig_redeem_script(2, &public_keys);
        let hash = UInt160::from_script(&verification_script);
        (pairs, verification_script, hash)
    }

    #[test]
    fn multisig_quick_path_keeps_ordered_matching_and_fee() {
        let (pairs, verification_script, hash) = two_of_three();
        let sign_data = b"neo-ledger multisig sign data";

        // Signers 0 and 2 leave the middle key unsigned: the ordered walk has to
        // consume key 1 without advancing the signature index.
        let signatures = sign_slots(&pairs, &[0, 2], &Crypto::sha256(sign_data));
        let witness = Witness::new_with_scripts(
            multi_sig_invocation(&signatures),
            verification_script.clone(),
        );

        match verify_standard_witness(&hash, &witness, sign_data) {
            Ok(StandardWitnessVerification::Verified {
                unscaled_verification_fee,
            }) => assert_eq!(
                unscaled_verification_fee,
                Helper::multi_signature_contract_cost(2, 3),
                "the quick path must keep charging the 2-of-3 fee"
            ),
            Ok(StandardWitnessVerification::NonStandard) => {
                panic!("a canonical 2-of-3 witness must stay on the quick path")
            }
            Err(result) => panic!("valid 2-of-3 witness rejected: {result:?}"),
        }

        // Swapping the slots breaks the ordered walk: slot 0 is compared against
        // key 0 first and the matcher never backtracks.
        let mut reordered = signatures;
        reordered.reverse();
        let reordered_witness =
            Witness::new_with_scripts(multi_sig_invocation(&reordered), verification_script);
        assert!(matches!(
            verify_standard_witness(&hash, &reordered_witness, sign_data),
            Err(VerifyResult::InvalidSignature)
        ));
    }

    #[test]
    fn multisig_quick_path_verifies_exactly_sha256_of_the_signing_bytes() {
        let (pairs, verification_script, hash) = two_of_three();
        let sign_data = b"digest boundary";
        let witness_over = |digest: &[u8; 32]| {
            let signatures = sign_slots(&pairs, &[0, 1], digest);
            Witness::new_with_scripts(
                multi_sig_invocation(&signatures),
                verification_script.clone(),
            )
        };

        // The single SHA-256 prehash of the signing bytes is what reaches the
        // verifier, so signatures over it must succeed.
        assert!(matches!(
            verify_standard_witness(&hash, &witness_over(&Crypto::sha256(sign_data)), sign_data),
            Ok(StandardWitnessVerification::Verified { .. })
        ));

        // Re-hashing the digest would mean the verifier hashed twice.
        assert!(matches!(
            verify_standard_witness(
                &hash,
                &witness_over(&Crypto::sha256(&Crypto::sha256(sign_data))),
                sign_data
            ),
            Err(VerifyResult::InvalidSignature)
        ));

        // A digest of different bytes must not be accepted for this sign data.
        assert!(matches!(
            verify_standard_witness(
                &hash,
                &witness_over(&Crypto::sha256(b"different signing bytes")),
                sign_data
            ),
            Err(VerifyResult::InvalidSignature)
        ));
    }
}
