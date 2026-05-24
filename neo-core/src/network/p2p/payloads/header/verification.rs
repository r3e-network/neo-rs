use super::{Header, Witness};
use crate::ledger::HeaderCache;
use crate::persistence::{DataCache, StoreCache};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::native::{ContractManagement, LedgerContract};
use crate::smart_contract::trigger_type::TriggerType;
use crate::smart_contract::{ContractBasicMethod, ContractParameterType};
use crate::validation::{
    validate_primary_index, validate_timestamp_bounds, validate_timestamp_progression,
    validate_witness_scripts, BlockValidationError,
};
use crate::{UInt160, UInt256};
use std::sync::Arc;
use tracing::debug;

pub(super) const HEADER_VERIFY_GAS: i64 = 300_000_000;

#[derive(Debug)]
enum HeaderSelfValidationFailure {
    Timestamp(BlockValidationError),
    PrimaryIndex(BlockValidationError),
    WitnessScripts(BlockValidationError),
}

impl HeaderSelfValidationFailure {
    fn name(&self) -> &'static str {
        match self {
            Self::Timestamp(_) => "timestamp_bounds",
            Self::PrimaryIndex(_) => "primary_index",
            Self::WitnessScripts(_) => "witness_scripts",
        }
    }

    fn error(&self) -> &BlockValidationError {
        match self {
            Self::Timestamp(error) | Self::PrimaryIndex(error) | Self::WitnessScripts(error) => {
                error
            }
        }
    }
}

impl Header {
    pub(super) fn validate_against_previous(
        &self,
        settings: &ProtocolSettings,
        prev_index: u32,
        prev_hash: &UInt256,
        prev_timestamp: u64,
    ) -> Result<(), &'static str> {
        // Validate primary index is within valid range
        if self.primary_index as i32 >= settings.validators_count {
            return Err("primary index exceeds validators count");
        }

        let Some(expected_index) = prev_index.checked_add(1) else {
            return Err("previous index overflow");
        };

        if expected_index != self.index {
            return Err("inconsistent block index");
        }

        if prev_hash != &self.prev_hash {
            return Err("previous hash mismatch");
        }

        // Validate timestamp progression using validation module
        if let Err(e) = validate_timestamp_progression(self.timestamp, prev_timestamp) {
            tracing::debug!(
                target: "neo::header",
                index = self.index,
                timestamp = self.timestamp,
                prev_timestamp = prev_timestamp,
                error = %e,
                "Timestamp progression validation failed"
            );
            return Err("non-increasing timestamp");
        }

        Ok(())
    }

    fn validate_self(
        &self,
        settings: &ProtocolSettings,
    ) -> Result<(), HeaderSelfValidationFailure> {
        validate_timestamp_bounds(self.timestamp)
            .map_err(HeaderSelfValidationFailure::Timestamp)?;
        validate_primary_index(self.primary_index, settings.validators_count)
            .map_err(HeaderSelfValidationFailure::PrimaryIndex)?;
        validate_witness_scripts(self).map_err(HeaderSelfValidationFailure::WitnessScripts)?;
        Ok(())
    }

    fn log_self_validation_failure(
        &self,
        settings: &ProtocolSettings,
        failure: &HeaderSelfValidationFailure,
        cached: bool,
    ) {
        let validation = failure.name();
        let error = failure.error();
        if cached {
            tracing::warn!(
                target: "neo",
                index = self.index,
                timestamp = self.timestamp,
                primary_index = self.primary_index,
                validators_count = settings.validators_count,
                validation,
                error = %error,
                "Header self validation failed (cached)"
            );
        } else {
            debug!(
                target: "neo",
                index = self.index,
                timestamp = self.timestamp,
                primary_index = self.primary_index,
                validators_count = settings.validators_count,
                validation,
                error = %error,
                "Header self validation failed"
            );
        }
    }

    pub(super) fn verify_witness_against_hash(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        script_hash: &UInt160,
        witness: &Witness,
        gas_limit: i64,
    ) -> bool {
        debug!(
            target: "neo",
            %script_hash,
            gas_limit,
            "verifying witness against script hash"
        );
        if gas_limit < 0 {
            debug!(target: "neo", %script_hash, gas_limit, "gas limit below zero");
            return false;
        }

        let verification_gas = gas_limit.min(Helper::MAX_VERIFICATION_GAS);

        if let Err(e) = crate::script_validation::validate_strict_script(&witness.invocation_script)
        {
            tracing::warn!(
                target: "neo",
                %script_hash,
                error = %e,
                invocation_len = witness.invocation_script.len(),
                "invocation script is invalid"
            );
            return false;
        }

        let container: Arc<dyn crate::Verifiable> = Arc::new(self.clone());
        let snapshot_arc = Arc::new(snapshot.clone());

        let mut engine = match ApplicationEngine::new(
            TriggerType::Verification,
            Some(container),
            snapshot_arc,
            None,
            settings.clone(),
            verification_gas,
            None,
        ) {
            Ok(engine) => engine,
            Err(e) => {
                tracing::warn!(
                    target: "neo",
                    %script_hash,
                    error = %e,
                    "failed to create application engine"
                );
                return false;
            }
        };

        let verification_script = witness.verification_script.clone();

        if verification_script.is_empty() {
            let contract =
                match ContractManagement::get_contract_from_snapshot(snapshot, script_hash) {
                    Ok(Some(contract)) => contract,
                    _ => {
                        debug!(
                            target: "neo",
                            %script_hash,
                            "contract not found for verification"
                        );
                        return false;
                    }
                };

            let mut abi = contract.manifest.abi.clone();
            let method = match abi.get_method(
                ContractBasicMethod::VERIFY,
                ContractBasicMethod::VERIFY_P_COUNT,
            ) {
                Some(descriptor) => descriptor.clone(),
                None => {
                    debug!(
                        target: "neo",
                        %script_hash,
                        "verify method not found in contract ABI"
                    );
                    return false;
                }
            };

            if method.return_type != ContractParameterType::Boolean {
                debug!(
                    target: "neo",
                    %script_hash,
                    return_type = ?method.return_type,
                    "verify method return type is not boolean"
                );
                return false;
            }

            if engine
                .load_contract_method(contract, method, CallFlags::READ_ONLY)
                .is_err()
            {
                debug!(
                    target: "neo",
                    %script_hash,
                    "failed to load contract verification method"
                );
                return false;
            }
        } else {
            let witness_script_hash = witness.script_hash();
            debug!(
                target: "neo",
                %witness_script_hash,
                %script_hash,
                "comparing witness script hash with expected script hash"
            );
            if witness_script_hash != *script_hash {
                tracing::warn!(
                    target: "neo",
                    %witness_script_hash,
                    %script_hash,
                    "witness script hash mismatch"
                );
                return false;
            }

            if let Err(e) = crate::script_validation::validate_strict_script(&verification_script) {
                tracing::warn!(
                    target: "neo",
                    %script_hash,
                    error = %e,
                    verification_len = verification_script.len(),
                    "verification script is invalid"
                );
                return false;
            }

            if engine
                .load_script_with_state(verification_script, -1, 0, |state| {
                    state.call_flags = CallFlags::READ_ONLY;
                    state.script_hash = Some(*script_hash);
                })
                .is_err()
            {
                debug!(
                    target: "neo",
                    %script_hash,
                    "failed to load verification script with state"
                );
                return false;
            }
        }

        if engine
            .load_script_with_state(witness.invocation_script.clone(), -1, 0, |state| {
                state.call_flags = CallFlags::NONE;
            })
            .is_err()
        {
            debug!(
                target: "neo",
                %script_hash,
                "failed to load invocation script with state"
            );
            return false;
        }

        if let Err(e) = engine.execute() {
            tracing::warn!(
                target: "neo",
                %script_hash,
                error = %e,
                "engine execution failed"
            );
            return false;
        }

        let mut result_item = if engine.result_stack().len() == 1 {
            engine.result_stack().peek(0).ok().cloned()
        } else {
            None
        };

        if result_item.is_none() {
            if let Some(stack) = engine.current_evaluation_stack() {
                if stack.len() == 1 {
                    result_item = stack.peek(0).ok().cloned();
                }
            }
        }

        match result_item {
            Some(item) => match item.get_boolean() {
                Ok(result) => {
                    debug!(
                        target: "neo",
                        %script_hash,
                        result,
                        "witness verification result from stack"
                    );
                    result
                }
                Err(err) => {
                    debug!(
                        target: "neo",
                        %script_hash,
                        ?err,
                        "failed to read boolean result from stack"
                    );
                    false
                }
            },
            None => {
                debug!(
                    target: "neo",
                    %script_hash,
                    "result stack item missing"
                );
                false
            }
        }
    }

    /// Verifies the header using the provided store cache.
    ///
    /// Performs comprehensive validation including:
    /// - Timestamp bounds (within 15 minutes of current time)
    /// - Primary index validation
    /// - Witness script validation
    /// - Chain continuity checks
    /// - Witness verification against consensus
    pub fn verify(&self, settings: &ProtocolSettings, store_cache: &StoreCache) -> bool {
        if let Err(error) = self.validate_self(settings) {
            self.log_self_validation_failure(settings, &error, false);
            return false;
        }

        // Step 4: Get previous block for chain validation
        let ledger = LedgerContract::new();
        let prev_trimmed = match ledger.get_trimmed_block(store_cache, &self.prev_hash) {
            Ok(Some(block)) => block,
            Ok(None) => {
                debug!(
                    target: "neo",
                    index = self.index,
                    prev_hash = %self.prev_hash,
                    "verify: get_trimmed_block returned None for prev_hash"
                );
                return false;
            }
            Err(err) => {
                debug!(
                    target: "neo",
                    index = self.index,
                    prev_hash = %self.prev_hash,
                    error = %err,
                    "verify: get_trimmed_block failed"
                );
                return false;
            }
        };

        let prev_index = prev_trimmed.index();
        let prev_hash = match prev_trimmed.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                debug!(
                    target: "neo",
                    index = self.index,
                    error = %error,
                    "verify: failed to hash previous trimmed block"
                );
                return false;
            }
        };
        let prev_header = prev_trimmed.header.clone();
        let prev_timestamp = prev_header.timestamp;
        let script_hash = prev_header.next_consensus;

        // Step 5: Validate against previous block
        if let Err(reason) =
            self.validate_against_previous(settings, prev_index, &prev_hash, prev_timestamp)
        {
            debug!(
                target: "neo",
                index = self.index,
                %prev_hash,
                prev_index,
                %reason,
                "header failed validation against previous block"
            );
            return false;
        }

        // Step 6: Verify witness against consensus script hash
        let snapshot = store_cache.data_cache();
        let verified = self.verify_witness_against_hash(
            settings,
            snapshot,
            &script_hash,
            &self.witness,
            HEADER_VERIFY_GAS,
        );

        if !verified {
            debug!(
                target: "neo",
                index = self.index,
                %script_hash,
                prev_index,
                %prev_hash,
                "header witness verification failed against previous block"
            );
        }

        verified
    }

    /// Verifies the header using persisted state and cached headers.
    ///
    /// Performs the same validations as `verify` but uses the header cache for efficiency.
    pub fn verify_with_cache(
        &self,
        settings: &ProtocolSettings,
        store_cache: &StoreCache,
        header_cache: &HeaderCache,
    ) -> bool {
        if let Err(error) = self.validate_self(settings) {
            self.log_self_validation_failure(settings, &error, true);
            return false;
        }

        // Step 4: Use cache for previous header if available
        if let Some(mut prev_header) = header_cache.last() {
            let prev_hash = match prev_header.try_hash() {
                Ok(hash) => hash,
                Err(error) => {
                    tracing::warn!(
                        target: "neo",
                        index = self.index,
                        error = %error,
                        "failed to hash cached previous header"
                    );
                    return false;
                }
            };
            let prev_index = prev_header.index();
            let prev_timestamp = prev_header.timestamp();
            let script_hash = *prev_header.next_consensus();

            debug!(
                target: "neo",
                index = self.index,
                prev_index,
                %prev_hash,
                "verifying header index {} against previous index {}",
                self.index,
                prev_index
            );
            debug!(
                target: "neo",
                index = self.index,
                prev_index,
                %prev_hash,
                "attempting validate_against_previous"
            );
            if let Err(reason) =
                self.validate_against_previous(settings, prev_index, &prev_hash, prev_timestamp)
            {
                tracing::warn!(
                    target: "neo",
                    index = self.index,
                    %prev_hash,
                    prev_index,
                    %reason,
                    "header failed validation against cached previous"
                );
                return false;
            }

            let snapshot = store_cache.data_cache();
            debug!(
                target: "neo",
                index = self.index,
                prev_index,
                %prev_hash,
                %script_hash,
                "verifying witness against script_hash: {}",
                script_hash
            );
            let verified = self.verify_witness_against_hash(
                settings,
                snapshot,
                &script_hash,
                &self.witness,
                HEADER_VERIFY_GAS,
            );

            if !verified {
                tracing::warn!(
                    target: "neo",
                    index = self.index,
                    %script_hash,
                    prev_index,
                    %prev_hash,
                    failed_check = "verify_witness_against_hash",
                    "header witness verification failed against cached previous"
                );
            }

            return verified;
        }

        // Fall back to full verification if cache is empty
        tracing::warn!(
            target: "neo",
            index = self.index,
            "header_cache empty, falling back to full verification"
        );
        self.verify(settings, store_cache)
    }
}
