use super::header_cache::HeaderCache;
use super::resolver::LedgerWitnessHashResolver;
use super::rules::{BlockValidationError, validate_primary_index, validate_timestamp_progression};
use neo_native_contracts::{
    ContractManagement, LedgerContract, composition::standard_native_services,
};
use neo_primitives::{
    CallFlags, ContractBasicMethod, ContractParameterType, TriggerType, UInt160, UInt256,
    Verifiable,
};
use neo_protocol::payloads::Header;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::Witness;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::helper::Helper;
use neo_storage::persistence::{DataCache, StoreCache};
use std::sync::Arc;
use tracing::debug;

pub const HEADER_VERIFY_GAS: i64 = 300_000_000;

#[derive(Debug)]
enum HeaderSelfValidationFailure {
    PrimaryIndex(BlockValidationError),
}

impl HeaderSelfValidationFailure {
    fn name(&self) -> &'static str {
        match self {
            Self::PrimaryIndex(_) => "primary_index",
        }
    }

    fn error(&self) -> &BlockValidationError {
        match self {
            Self::PrimaryIndex(error) => error,
        }
    }
}

pub fn validate_header_against_previous(
    header: &Header,
    settings: &ProtocolSettings,
    prev_index: u32,
    prev_hash: &UInt256,
    prev_timestamp: u64,
) -> Result<(), &'static str> {
    if header.primary_index() as i32 >= settings.validators_count {
        return Err("primary index exceeds validators count");
    }
    let Some(expected_index) = prev_index.checked_add(1) else {
        return Err("previous index overflow");
    };
    if expected_index != header.index() {
        return Err("inconsistent block index");
    }
    if prev_hash != header.prev_hash() {
        return Err("previous hash mismatch");
    }
    if let Err(e) = validate_timestamp_progression(header.timestamp(), prev_timestamp) {
        tracing::debug!(
            target: "neo::header",
            index = header.index(),
            timestamp = header.timestamp(),
            prev_timestamp,
            error = %e,
            "Timestamp progression validation failed"
        );
        return Err("non-increasing timestamp");
    }
    Ok(())
}

fn validate_self(
    header: &Header,
    settings: &ProtocolSettings,
) -> Result<(), HeaderSelfValidationFailure> {
    // Header.Verify applies no wall-clock or structural witness-script checks.
    validate_primary_index(header.primary_index(), settings.validators_count)
        .map_err(HeaderSelfValidationFailure::PrimaryIndex)?;
    Ok(())
}

fn log_self_validation_failure(
    header: &Header,
    settings: &ProtocolSettings,
    failure: &HeaderSelfValidationFailure,
    cached: bool,
) {
    let validation = failure.name();
    let error = failure.error();
    if cached {
        tracing::warn!(
            target: "neo",
            index = header.index(),
            timestamp = header.timestamp(),
            primary_index = header.primary_index(),
            validators_count = settings.validators_count,
            validation,
            error = %error,
            "Header self validation failed (cached)"
        );
    } else {
        debug!(
            target: "neo",
            index = header.index(),
            timestamp = header.timestamp(),
            primary_index = header.primary_index(),
            validators_count = settings.validators_count,
            validation,
            error = %error,
            "Header self validation failed"
        );
    }
}

pub fn verify_header_witness_against_hash(
    header: &Header,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    script_hash: &UInt160,
    witness: &Witness,
    gas_limit: i64,
) -> bool {
    debug!(target: "neo", %script_hash, gas_limit, "verifying witness against script hash");
    if gas_limit < 0 {
        debug!(target: "neo", %script_hash, gas_limit, "gas limit below zero");
        return false;
    }
    let verification_gas = gas_limit.min(Helper::MAX_VERIFICATION_GAS);
    if let Err(e) = neo_vm::validate_strict_script(&witness.invocation_script) {
        tracing::warn!(target: "neo", %script_hash, error = %e, invocation_len = witness.invocation_script.len(), "invocation script is invalid");
        return false;
    }
    let container: Arc<dyn Verifiable> = Arc::new(header.clone());
    let snapshot_arc = Arc::new(snapshot.clone());
    let mut engine = match ApplicationEngine::new(
        TriggerType::Verification,
        Some(container),
        snapshot_arc,
        None,
        settings.clone(),
        verification_gas,
        None,
        standard_native_services(Arc::new(LedgerWitnessHashResolver)),
    ) {
        Ok(engine) => engine,
        Err(e) => {
            tracing::warn!(target: "neo", %script_hash, error = %e, "failed to create application engine");
            return false;
        }
    };
    let verification_script = witness.verification_script.clone();
    if verification_script.is_empty() {
        let Ok(Some(contract)) =
            ContractManagement::get_contract_from_snapshot(snapshot, script_hash)
        else {
            debug!(target: "neo", %script_hash, "contract not found for verification");
            return false;
        };
        let mut abi = contract.manifest.abi.clone();
        let method = match abi.get_method(
            ContractBasicMethod::VERIFY,
            ContractBasicMethod::VERIFY_P_COUNT,
        ) {
            Some(descriptor) => descriptor.clone(),
            None => {
                debug!(target: "neo", %script_hash, "verify method not found in contract ABI");
                return false;
            }
        };
        if method.return_type != ContractParameterType::Boolean {
            debug!(target: "neo", %script_hash, return_type = ?method.return_type, "verify method return type is not boolean");
            return false;
        }
        if engine
            .load_contract_method(contract, method, CallFlags::READ_ONLY)
            .is_err()
        {
            debug!(target: "neo", %script_hash, "failed to load contract verification method");
            return false;
        }
    } else {
        let witness_script_hash = witness.script_hash();
        debug!(target: "neo", %witness_script_hash, %script_hash, "comparing witness script hash with expected script hash");
        if witness_script_hash != *script_hash {
            tracing::warn!(target: "neo", %witness_script_hash, %script_hash, "witness script hash mismatch");
            return false;
        }
        if let Err(e) = neo_vm::validate_strict_script(&verification_script) {
            tracing::warn!(target: "neo", %script_hash, error = %e, verification_len = verification_script.len(), "verification script is invalid");
            return false;
        }
        if engine
            .load_script_with_state(verification_script, -1, 0, |state| {
                state.call_flags = CallFlags::READ_ONLY;
                state.script_hash = Some(*script_hash);
            })
            .is_err()
        {
            debug!(target: "neo", %script_hash, "failed to load verification script with state");
            return false;
        }
    }
    if engine
        .load_script_with_state(witness.invocation_script.clone(), -1, 0, |state| {
            state.call_flags = CallFlags::NONE;
        })
        .is_err()
    {
        debug!(target: "neo", %script_hash, "failed to load invocation script with state");
        return false;
    }
    if let Err(e) = engine.execute() {
        tracing::warn!(target: "neo", %script_hash, error = %e, "engine execution failed");
        return false;
    }
    let result_item = if engine.result_stack().len() == 1 {
        engine.result_stack().peek(0).ok().cloned()
    } else {
        None
    };
    match result_item {
        Some(item) => match item.as_bool() {
            Ok(result) => {
                debug!(target: "neo", %script_hash, result, "witness verification result from stack");
                result
            }
            Err(err) => {
                debug!(target: "neo", %script_hash, ?err, "failed to read boolean result from stack");
                false
            }
        },
        None => {
            debug!(target: "neo", %script_hash, "result stack item missing");
            false
        }
    }
}

pub fn verify_header(
    header: &Header,
    settings: &ProtocolSettings,
    store_cache: &StoreCache,
) -> bool {
    if let Err(error) = validate_self(header, settings) {
        log_self_validation_failure(header, settings, &error, false);
        return false;
    }
    let prev_trimmed = match LedgerContract::new()
        .get_trimmed_block(store_cache, header.prev_hash())
    {
        Ok(Some(block)) => block,
        Ok(None) => {
            debug!(target: "neo", index = header.index(), prev_hash = %header.prev_hash(), "verify: get_trimmed_block returned None for prev_hash");
            return false;
        }
        Err(err) => {
            debug!(target: "neo", index = header.index(), prev_hash = %header.prev_hash(), error = %err, "verify: get_trimmed_block failed");
            return false;
        }
    };
    let prev_index = prev_trimmed.index();
    let prev_hash = match prev_trimmed.try_hash() {
        Ok(hash) => hash,
        Err(error) => {
            debug!(target: "neo", index = header.index(), error = %error, "verify: failed to hash previous trimmed block");
            return false;
        }
    };
    let prev_header = prev_trimmed.header.clone();
    let prev_timestamp = prev_header.timestamp;
    let script_hash = prev_header.next_consensus;
    if let Err(reason) =
        validate_header_against_previous(header, settings, prev_index, &prev_hash, prev_timestamp)
    {
        debug!(target: "neo", index = header.index(), %prev_hash, prev_index, %reason, "header failed validation against previous block");
        return false;
    }
    let verified = verify_header_witness_against_hash(
        header,
        settings,
        store_cache.data_cache(),
        &script_hash,
        &header.witness,
        HEADER_VERIFY_GAS,
    );
    if !verified {
        debug!(target: "neo", index = header.index(), %script_hash, prev_index, %prev_hash, "header witness verification failed against previous block");
    }
    verified
}

pub fn verify_header_with_cache(
    header: &Header,
    settings: &ProtocolSettings,
    store_cache: &StoreCache,
    header_cache: &HeaderCache,
) -> bool {
    if let Err(error) = validate_self(header, settings) {
        log_self_validation_failure(header, settings, &error, true);
        return false;
    }
    if let Some(mut prev_header) = header_cache.last() {
        let prev_hash = match prev_header.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(target: "neo", index = header.index(), error = %error, "failed to hash cached previous header");
                return false;
            }
        };
        let prev_index = prev_header.index();
        let prev_timestamp = prev_header.timestamp();
        let script_hash = *prev_header.next_consensus();
        debug!(target: "neo", index = header.index(), prev_index, %prev_hash, "verifying header index {} against previous index {}", header.index(), prev_index);
        debug!(target: "neo", index = header.index(), prev_index, %prev_hash, "attempting validate_against_previous");
        if let Err(reason) = validate_header_against_previous(
            header,
            settings,
            prev_index,
            &prev_hash,
            prev_timestamp,
        ) {
            tracing::warn!(target: "neo", index = header.index(), %prev_hash, prev_index, %reason, "header failed validation against cached previous");
            return false;
        }
        debug!(target: "neo", index = header.index(), prev_index, %prev_hash, %script_hash, "verifying witness against script_hash: {}", script_hash);
        let verified = verify_header_witness_against_hash(
            header,
            settings,
            store_cache.data_cache(),
            &script_hash,
            &header.witness,
            HEADER_VERIFY_GAS,
        );
        if !verified {
            tracing::warn!(target: "neo", index = header.index(), %script_hash, prev_index, %prev_hash, failed_check = "verify_witness_against_hash", "header witness verification failed against cached previous");
        }
        return verified;
    }
    tracing::warn!(target: "neo", index = header.index(), "header_cache empty, falling back to full verification");
    verify_header(header, settings, store_cache)
}
