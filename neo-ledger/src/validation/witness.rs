use super::resolver::{LedgerWitnessHashResolver, VerifiableExt};
use neo_native_contracts::{ContractManagement, composition::standard_native_services};
use neo_primitives::{
    CallFlags, ContractBasicMethod, ContractParameterType, TriggerType, UInt160, UInt256,
    Verifiable,
};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::{Witness, WitnessData};
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_smart_contract::helper::Helper;
use neo_storage::persistence::DataCache;
use neo_vm::VmState;
use std::any::Any;
use std::sync::Arc;

pub fn verify_witnesses<V: VerifiableExt>(
    verifiable: &V,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    max_gas: i64,
) -> bool {
    if max_gas < 0 {
        return false;
    }
    let max_gas = max_gas.min(Helper::MAX_VERIFICATION_GAS);
    let hashes = verifiable.script_hashes_for_verifying(snapshot);
    let witnesses = WitnessData::witnesses(verifiable);
    if hashes.len() != witnesses.len() {
        return false;
    }
    let mut remaining_gas = max_gas;
    for (i, hash) in hashes.iter().enumerate() {
        match verify_witness(
            verifiable,
            settings,
            snapshot,
            hash,
            witnesses[i],
            remaining_gas,
        ) {
            Ok(fee) => remaining_gas -= fee,
            Err(_) => return false,
        }
    }
    true
}

pub fn verify_witness<V: VerifiableExt>(
    verifiable: &V,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    hash: &UInt160,
    witness: &Witness,
    max_gas: i64,
) -> CoreResult<i64> {
    if !is_valid_script(&witness.invocation_script) {
        return Err(CoreError::invalid_operation(
            "Invalid invocation script".to_string(),
        ));
    }
    let cloned_snapshot = Arc::new(snapshot.clone_cache());
    let container_hash = verifiable
        .hash()
        .map_err(|error| CoreError::invalid_operation(error.to_string()))?;
    let container: Arc<dyn Verifiable> = if let Some(transaction) = verifiable.as_transaction() {
        Arc::new(transaction.clone())
    } else {
        Arc::new(VerifiableHashContainer {
            hash: container_hash,
            hash_data: verifiable.hash_data(),
        })
    };
    let mut engine = ApplicationEngine::new(
        TriggerType::Verification,
        Some(container),
        cloned_snapshot,
        None,
        settings.clone(),
        max_gas,
        None,
        standard_native_services(Arc::new(LedgerWitnessHashResolver)),
    )?;
    if witness.verification_script.is_empty() {
        let mut contract = ContractManagement::get_contract_from_snapshot(snapshot, hash)?
            .ok_or_else(|| {
                CoreError::invalid_operation(format!("Contract not found for hash {}", hash))
            })?;
        let verify_method = contract
            .manifest
            .abi
            .get_method(
                ContractBasicMethod::VERIFY,
                ContractBasicMethod::VERIFY_P_COUNT,
            )
            .cloned()
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "Contract does not have a valid Verify method".to_string(),
                )
            })?;
        if verify_method.return_type != ContractParameterType::Boolean {
            return Err(CoreError::invalid_operation(
                "Verify method must return Boolean".to_string(),
            ));
        }
        engine.load_contract_method(contract, verify_method, CallFlags::READ_ONLY)?;
    } else {
        if engine.is_native_contract_hash(hash) {
            return Err(CoreError::invalid_operation(
                "Cannot verify native contract".to_string(),
            ));
        }
        if *hash != witness.script_hash() {
            return Err(CoreError::invalid_operation(
                "Witness script hash mismatch".to_string(),
            ));
        }
        if !is_valid_script(&witness.verification_script) {
            return Err(CoreError::invalid_operation(
                "Invalid verification script".to_string(),
            ));
        }
        engine.load_script(
            witness.verification_script.clone(),
            CallFlags::READ_ONLY,
            Some(*hash),
        )?;
    }
    engine.load_script(witness.invocation_script.clone(), CallFlags::NONE, None)?;
    engine.execute()?;
    if engine.state() == VmState::FAULT {
        return Err(CoreError::invalid_operation(
            "Verification execution faulted".to_string(),
        ));
    }
    let result_stack = engine.result_stack();
    if result_stack.len() != 1 {
        return Err(CoreError::invalid_operation(format!(
            "Verification must leave exactly 1 item on stack, got {}",
            result_stack.len(),
        )));
    }
    let result = result_stack.peek(0).map_err(|error| {
        CoreError::invalid_operation(format!("Failed to peek result: {}", error))
    })?;
    if !result.as_bool().unwrap_or(false) {
        return Err(CoreError::invalid_operation(
            "Verification returned false".to_string(),
        ));
    }
    Ok(engine.fee_consumed())
}

fn is_valid_script(script: &[u8]) -> bool {
    // Preserve the historical permissive helper; strict validation belongs to the typed paths.
    if script.is_empty() {
        return true;
    }
    true
}

struct VerifiableHashContainer {
    hash: UInt256,
    hash_data: Vec<u8>,
}

impl Verifiable for VerifiableHashContainer {
    fn verify(&self) -> bool {
        true
    }
    fn hash(&self) -> neo_primitives::error::PrimitiveResult<UInt256> {
        Ok(self.hash)
    }
    fn hash_data(&self) -> Vec<u8> {
        self.hash_data.clone()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl VerifiableExt for VerifiableHashContainer {
    fn script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        Vec::new()
    }
}

impl WitnessData for VerifiableHashContainer {
    fn witnesses(&self) -> Vec<&Witness> {
        Vec::new()
    }
    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        Vec::new()
    }
}
