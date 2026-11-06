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
use neo_vm::{OpCode, validate_strict_script};
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
    if let Some(public_key) = parse_single_signature_contract(&witness.verification_script) {
        if witness.script_hash() != *hash {
            return Err(VerifyResult::Invalid);
        }
        let Some(signature) = parse_single_signature_invocation(&witness.invocation_script) else {
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
    let Some((m, public_keys)) = Helper::parse_multi_sig_contract(&witness.verification_script)
    else {
        return Ok(StandardWitnessVerification::NonStandard);
    };
    if witness.script_hash() != *hash {
        return Err(VerifyResult::Invalid);
    }
    let Some(signatures) = Helper::parse_multi_sig_invocation(&witness.invocation_script, m) else {
        // R04: invocation formats outside the quick checker defer to full VM execution.
        return Ok(StandardWitnessVerification::NonStandard);
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
        let verified = Secp256r1Crypto::verify_prehash(
            &Crypto::sha256(sign_data),
            &signature_bytes,
            &public_keys[key_index],
        )
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
    if validate_strict_script(&witness.invocation_script).is_err() {
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
    let verification_script = witness.verification_script.clone();
    if verification_script.is_empty() {
        let Ok(Some(contract)) = ContractManagement::get_contract_from_snapshot(snapshot, hash)
        else {
            return false;
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

pub fn parse_single_signature_invocation(invocation: &[u8]) -> Option<&[u8]> {
    if invocation.len() != 66 {
        return None;
    }
    if invocation[0] != OpCode::PUSHDATA1.byte() || invocation[1] != 0x40 {
        return None;
    }
    Some(&invocation[2..66])
}
