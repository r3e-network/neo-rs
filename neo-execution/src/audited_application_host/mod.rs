//! # neo-execution::audited_application_host
//!
//! Compile-time restricted application host for guarded specializations.
//!
//! ## Boundary
//!
//! This module exposes only policy-audited access to an existing application
//! engine. It does not select candidates, publish candidate state, or replace
//! ordinary NeoVM execution as the semantic authority.
//!
//! ## Contents
//!
//! - Guarded host error and result contracts.
//! - The restricted `AuditedApplicationHost` facade.
//! - Policy checks for context, storage, fees, events, and contract calls.

use crate::StorageContext;
use crate::application_engine::{ApplicationEngine, FEE_FACTOR};
use crate::diagnostic::Diagnostic;
use crate::execution_artifact::ContextObservationValue;
use crate::host_access_audit::{
    ContractCallAccess, ContractCallKind, HostAccessAudit, HostAccessViolation, HostContextAccess,
    ResolvedStorageRangeDomain, StorageRangeAccess,
};
use crate::iterators::StorageIterator;
use crate::native_contract_provider::NativeContractProvider;
use neo_error::CoreError;
use neo_payloads::{NotifyEventArgs, VerifiableContainer};
use neo_primitives::CallFlags;
use neo_primitives::{Hardfork, TriggerType, UInt160};
use neo_storage::{CacheRead, SeekDirection, StorageKey};
use neo_vm::StackItem;
use std::sync::Arc;

/// Failure from a guarded specialization host operation.
#[derive(Debug, thiserror::Error)]
pub enum AuditedHostError {
    /// The candidate attempted an operation absent from its immutable policy.
    #[error(transparent)]
    Undeclared(#[from] HostAccessViolation),
    /// The ordinary application host rejected an otherwise-declared operation.
    #[error(transparent)]
    Host(#[from] CoreError),
}

/// Result returned by guarded specialization host operations.
pub type AuditedHostResult<T> = Result<T, AuditedHostError>;

/// Restricted facade over an [`ApplicationEngine`].
///
/// Candidate functions should receive this type rather than `ApplicationEngine`.
/// It intentionally exposes no raw snapshot, native-cache handle, VM engine, or
/// escape hatch to the wrapped application engine. Every method authorizes the
/// exact attempt before invoking the ordinary host operation.
pub struct AuditedApplicationHost<'engine, 'policy, P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    engine: &'engine mut ApplicationEngine<P, D, B>,
    audit: &'engine mut HostAccessAudit<'policy>,
}

impl<'engine, 'policy, P, D, B> AuditedApplicationHost<'engine, 'policy, P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    /// Restricts an isolated, discardable `engine` overlay to the audit policy.
    ///
    /// Construction stays crate-private so the specialization runner must own
    /// overlay isolation. A caller must discard that overlay when the audit or
    /// candidate fails; this facade never authorizes direct canonical writes.
    #[must_use]
    pub(crate) fn new(
        engine: &'engine mut ApplicationEngine<P, D, B>,
        audit: &'engine mut HostAccessAudit<'policy>,
    ) -> Self {
        Self { engine, audit }
    }

    /// Reads one exact storage key, including an absent result.
    pub fn storage_get(&mut self, key: &StorageKey) -> AuditedHostResult<Option<Vec<u8>>> {
        self.audit.storage_read(key)?;
        let context = StorageContext::read_only(key.id());
        Ok(self.engine.storage_get(context, key.suffix().to_vec())?)
    }

    /// Writes one exact storage key through the ordinary fee-charging host.
    ///
    /// Neo's put fee depends on the old value, so both `StorageRead` and
    /// `StorageWrite` declarations are required before any host effect occurs.
    pub fn storage_put(&mut self, key: &StorageKey, value: Vec<u8>) -> AuditedHostResult<()> {
        self.audit.storage_read(key)?;
        self.audit.storage_write(key, value.len())?;
        let context = StorageContext::read_write(key.id());
        self.engine
            .storage_put(context, key.suffix().to_vec(), value)?;
        Ok(())
    }

    /// Deletes one exact storage key.
    pub fn storage_delete(&mut self, key: &StorageKey) -> AuditedHostResult<()> {
        self.audit.storage_delete(key)?;
        let context = StorageContext::read_write(key.id());
        self.engine.storage_delete(context, key.suffix().to_vec())?;
        Ok(())
    }

    /// Materializes one exactly declared storage range through the ordinary host.
    pub fn storage_find(
        &mut self,
        range: &StorageRangeAccess,
    ) -> AuditedHostResult<StorageIterator> {
        self.audit.storage_range(range)?;
        let ResolvedStorageRangeDomain::Prefix(prefix) = range.domain() else {
            return Err(CoreError::invalid_operation(
                "Half-open specialization ranges require an exact bounded storage primitive",
            )
            .into());
        };
        let options = range.options();
        if options.bits() & !neo_primitives::FindOptions::Backwards.bits() != 0 {
            return Err(CoreError::invalid_operation(
                "Specialization range reads expose raw key/value rows only",
            )
            .into());
        }
        let direction = match range.direction() {
            neo_vm::RangeDirection::Forward => SeekDirection::Forward,
            neo_vm::RangeDirection::Reverse => SeekDirection::Backward,
        };
        let search_key = StorageKey::new(range.contract_id(), prefix.clone());
        let maximum = usize::try_from(range.max_items()).map_err(|_| {
            CoreError::invalid_operation("Specialization range item bound does not fit usize")
        })?;
        let entries = self
            .engine
            .snapshot_cache()
            .find(Some(&search_key), direction)
            .take(maximum.saturating_add(1))
            .collect::<Vec<_>>();
        if entries.len() > maximum {
            return Err(
                CoreError::invalid_operation("Specialization range item bound exceeded").into(),
            );
        }
        if self.engine.execution_observations_enabled() {
            self.engine.observe_storage_range(range.clone(), &entries);
        }
        Ok(StorageIterator::new(entries, prefix.len(), options))
    }

    /// Loads an exactly declared dynamic contract call.
    pub fn call_contract_dynamic(
        &mut self,
        access: &ContractCallAccess,
        args: Vec<StackItem>,
    ) -> AuditedHostResult<()> {
        Self::validate_call(access, ContractCallKind::Dynamic, args.len())?;
        self.audit.contract_call(access)?;
        self.validate_contract_resolution(access)?;
        self.engine.call_contract_dynamic(
            &access.contract_hash(),
            access.method(),
            access.call_flags(),
            args,
        )?;
        Ok(())
    }

    /// Invokes an exactly declared native method.
    ///
    /// The engine's method metadata cache is an implementation cache, not the
    /// versioned native-state cache declared by `NativeCacheDependency`.
    pub fn call_native_contract(
        &mut self,
        call_access: &ContractCallAccess,
        args: &[Vec<u8>],
    ) -> AuditedHostResult<Vec<u8>> {
        Self::validate_call(call_access, ContractCallKind::Native, args.len())?;
        self.audit.contract_call(call_access)?;
        self.validate_contract_resolution(call_access)?;
        Ok(self.engine.call_native_contract(
            call_access.contract_hash(),
            call_access.method(),
            args,
        )?)
    }

    /// Executes an exactly declared returning call from a native frame.
    pub fn call_from_native_returning(
        &mut self,
        calling_script_hash: &UInt160,
        access: &ContractCallAccess,
        args: Vec<StackItem>,
    ) -> AuditedHostResult<StackItem> {
        let attempted = access
            .clone()
            .with_native_calling_script_hash(*calling_script_hash);
        Self::validate_call(
            &attempted,
            ContractCallKind::FromNativeReturning,
            args.len(),
        )?;
        self.audit.contract_call(&attempted)?;
        self.validate_contract_resolution(&attempted)?;
        Ok(self.engine.call_from_native_contract_returning(
            calling_script_hash,
            &attempted.contract_hash(),
            attempted.method(),
            args,
        )?)
    }

    /// Executes an exactly declared void call from a native frame.
    pub fn call_from_native_void(
        &mut self,
        calling_script_hash: &UInt160,
        access: &ContractCallAccess,
        args: Vec<StackItem>,
    ) -> AuditedHostResult<()> {
        let attempted = access
            .clone()
            .with_native_calling_script_hash(*calling_script_hash);
        Self::validate_call(&attempted, ContractCallKind::FromNativeVoid, args.len())?;
        self.audit.contract_call(&attempted)?;
        self.validate_contract_resolution(&attempted)?;
        self.engine.call_from_native_contract_void(
            calling_script_hash,
            &attempted.contract_hash(),
            attempted.method(),
            args,
        )?;
        Ok(())
    }

    /// Emits an exactly declared notification through the ordinary host.
    pub fn send_notification(
        &mut self,
        script_hash: UInt160,
        event_name: &str,
        state: Vec<StackItem>,
    ) -> AuditedHostResult<()> {
        self.audit
            .notification(script_hash, event_name, state.len())?;
        self.engine
            .send_notification(script_hash, event_name.to_string(), state)?;
        Ok(())
    }

    /// Emits a bounded declared log message through the ordinary host.
    pub fn log(&mut self, message: String) -> AuditedHostResult<()> {
        let actual_hash = self
            .engine
            .current_script_hash()
            .unwrap_or_else(UInt160::zero);
        self.audit.log(actual_hash, message.len())?;
        self.engine.log(message)?;
        Ok(())
    }

    /// Charges an exactly declared execution fee in datoshi.
    pub fn charge_execution_fee(&mut self, fee: u64) -> AuditedHostResult<()> {
        self.audit.fee(fee)?;
        self.engine.charge_execution_fee(fee)?;
        Ok(())
    }

    /// Charges declared opcode CPU units through the current Policy factor.
    ///
    /// A shadow runner must perform this before candidate stack or host effects
    /// and discard its overlay on failure, so insufficient gas re-enters the
    /// ordinary VM at the exact per-opcode fault point.
    pub fn charge_cpu_fee_units(&mut self, fee_units: u64) -> AuditedHostResult<()> {
        self.audit.cpu_fee(fee_units)?;
        let fee_units = i64::try_from(fee_units)
            .map_err(|_| CoreError::invalid_operation("CPU fee units do not fit into i64"))?;
        self.engine.add_cpu_fee(fee_units)?;
        Ok(())
    }

    /// Performs one exactly declared witness check.
    pub fn check_witness(&mut self, hash: &UInt160) -> AuditedHostResult<bool> {
        self.audit.witness(*hash)?;
        Ok(self.engine.check_witness(hash)?)
    }

    /// Reads the declared execution trigger.
    pub fn trigger(&mut self) -> AuditedHostResult<TriggerType> {
        self.audit.context(HostContextAccess::Trigger)?;
        let value = self.engine.trigger();
        self.engine.observe_context(
            HostContextAccess::Trigger,
            ContextObservationValue::Trigger(value),
        );
        Ok(value)
    }

    /// Reads the declared network magic.
    pub fn network(&mut self) -> AuditedHostResult<u32> {
        self.audit.context(HostContextAccess::Network)?;
        let value = self.engine.protocol_settings().network;
        self.engine.observe_context(
            HostContextAccess::Network,
            ContextObservationValue::U32(value),
        );
        Ok(value)
    }

    /// Reads the declared address-version byte.
    pub fn address_version(&mut self) -> AuditedHostResult<u8> {
        self.audit.context(HostContextAccess::AddressVersion)?;
        let value = self.engine.protocol_settings().address_version;
        self.engine.observe_context(
            HostContextAccess::AddressVersion,
            ContextObservationValue::U8(value),
        );
        Ok(value)
    }

    /// Reads the declared current block index.
    pub fn block_index(&mut self) -> AuditedHostResult<u32> {
        self.audit.context(HostContextAccess::BlockIndex)?;
        let value = self.engine.current_block_index();
        self.engine.observe_context(
            HostContextAccess::BlockIndex,
            ContextObservationValue::U32(value),
        );
        Ok(value)
    }

    /// Reads the declared persisting-block timestamp.
    pub fn block_timestamp(&mut self) -> AuditedHostResult<u64> {
        self.audit.context(HostContextAccess::BlockTimestamp)?;
        let value = self.engine.current_block_timestamp()?;
        self.engine.observe_context(
            HostContextAccess::BlockTimestamp,
            ContextObservationValue::U64(value),
        );
        Ok(value)
    }

    /// Reads the declared current script container.
    pub fn script_container(&mut self) -> AuditedHostResult<Option<&Arc<VerifiableContainer>>> {
        self.audit.context(HostContextAccess::ScriptContainer)?;
        self.engine.observe_script_container_context();
        Ok(self.engine.script_container())
    }

    /// Reads the declared executing script hash.
    pub fn executing_script_hash(&mut self) -> AuditedHostResult<Option<UInt160>> {
        self.audit.context(HostContextAccess::ExecutingScriptHash)?;
        let value = self.engine.current_script_hash();
        self.engine.observe_context(
            HostContextAccess::ExecutingScriptHash,
            ContextObservationValue::Hash160(value),
        );
        Ok(value)
    }

    /// Reads the declared calling script hash.
    pub fn calling_script_hash(&mut self) -> AuditedHostResult<Option<UInt160>> {
        self.audit.context(HostContextAccess::CallingScriptHash)?;
        let value = self.engine.get_calling_script_hash();
        self.engine.observe_context(
            HostContextAccess::CallingScriptHash,
            ContextObservationValue::Hash160(value),
        );
        Ok(value)
    }

    /// Reads the declared entry script hash.
    pub fn entry_script_hash(&mut self) -> AuditedHostResult<Option<UInt160>> {
        self.audit.context(HostContextAccess::EntryScriptHash)?;
        let value = self.engine.entry_script_hash();
        self.engine.observe_context(
            HostContextAccess::EntryScriptHash,
            ContextObservationValue::Hash160(value),
        );
        Ok(value)
    }

    /// Reads the declared transaction sender.
    pub fn transaction_sender(&mut self) -> AuditedHostResult<Option<UInt160>> {
        self.audit.context(HostContextAccess::TransactionSender)?;
        let value = self.engine.get_transaction_sender();
        self.engine.observe_context(
            HostContextAccess::TransactionSender,
            ContextObservationValue::Hash160(value),
        );
        Ok(value)
    }

    /// Reads the declared effective call flags.
    pub fn call_flags(&mut self) -> AuditedHostResult<CallFlags> {
        self.audit.context(HostContextAccess::CallFlags)?;
        let value = self
            .engine
            .get_current_call_flags()
            .map_err(|error| CoreError::invalid_operation(error.to_string()))?;
        self.engine.observe_context(
            HostContextAccess::CallFlags,
            ContextObservationValue::CallFlags(value.bits()),
        );
        Ok(value)
    }

    /// Reads the declared remaining gas in datoshi.
    pub fn gas_left(&mut self) -> AuditedHostResult<i64> {
        self.audit.context(HostContextAccess::GasLeft)?;
        let value = self
            .engine
            .fee_amount_pico()
            .saturating_sub(self.engine.fee_consumed_pico())
            .saturating_div(FEE_FACTOR);
        self.engine.observe_context(
            HostContextAccess::GasLeft,
            ContextObservationValue::I64(value),
        );
        Ok(value)
    }

    /// Reads whether the current context bypasses per-opcode execution fees.
    pub fn fee_whitelisted(&mut self) -> AuditedHostResult<bool> {
        self.audit.context(HostContextAccess::FeeWhitelist)?;
        let state = self
            .engine
            .current_execution_state()
            .map_err(|error| CoreError::invalid_operation(error.to_string()))?;
        let whitelisted = state.lock().whitelisted;
        self.engine.observe_context(
            HostContextAccess::FeeWhitelist,
            ContextObservationValue::Boolean(whitelisted),
        );
        Ok(whitelisted)
    }

    /// Reads whether one declared hardfork applies to this execution.
    pub fn is_hardfork_enabled(&mut self, hardfork: Hardfork) -> AuditedHostResult<bool> {
        self.audit.context(HostContextAccess::Hardfork(hardfork))?;
        let value = self.engine.is_hardfork_enabled(hardfork);
        self.engine.observe_context(
            HostContextAccess::Hardfork(hardfork),
            ContextObservationValue::Boolean(value),
        );
        Ok(value)
    }

    /// Reads one declared logical script invocation count.
    pub fn invocation_counter(&mut self, script_hash: UInt160) -> AuditedHostResult<u32> {
        self.audit
            .context(HostContextAccess::InvocationCounter(script_hash))?;
        let value = self.engine.get_invocation_counter(&script_hash);
        self.engine.observe_context(
            HostContextAccess::InvocationCounter(script_hash),
            ContextObservationValue::U32(value),
        );
        Ok(value)
    }

    /// Reads notifications visible to the current execution.
    pub fn notifications(&mut self) -> AuditedHostResult<&[NotifyEventArgs]> {
        self.audit.context(HostContextAccess::Notifications)?;
        if self.engine.execution_observations_enabled() {
            match self.engine.get_notifications(None) {
                Ok(values) => self.engine.observe_context(
                    HostContextAccess::Notifications,
                    ContextObservationValue::StackItems(values),
                ),
                Err(error) => self.engine.fail_execution_observation(
                    crate::execution_artifact::ExecutionArtifactError::ObservationFailed {
                        kind: "notification context",
                        message: error.to_string(),
                    },
                ),
            }
        }
        Ok(self.engine.notifications())
    }

    fn validate_call(
        access: &ContractCallAccess,
        expected_kind: ContractCallKind,
        actual_argument_count: usize,
    ) -> Result<(), CoreError> {
        if access.kind() != expected_kind {
            return Err(CoreError::invalid_operation(
                "Contract call declaration uses the wrong call route",
            ));
        }
        if access.argument_count() != actual_argument_count {
            return Err(CoreError::invalid_operation(
                "Contract call declaration argument count does not match the invocation",
            ));
        }
        let is_native_origin = matches!(
            expected_kind,
            ContractCallKind::FromNativeReturning | ContractCallKind::FromNativeVoid
        );
        if is_native_origin != access.native_calling_script_hash().is_some() {
            return Err(CoreError::invalid_operation(
                "Native-origin call declarations must bind exactly one calling script hash",
            ));
        }
        Ok(())
    }

    fn validate_contract_resolution(&self, access: &ContractCallAccess) -> Result<(), CoreError> {
        let expected = access.contract();
        let contracts = self.engine.contracts_snapshot();
        let contract = contracts.get(&expected.contract_hash()).ok_or_else(|| {
            CoreError::invalid_operation(
                "Specialized contract calls require a pre-resolved exact target",
            )
        })?;
        if contract.id != expected.contract_id()
            || contract.update_counter != expected.update_counter()
            || contract.nef.checksum != expected.nef_checksum()
        {
            return Err(CoreError::invalid_operation(
                "Specialized contract call target version changed",
            ));
        }
        let method = contract
            .manifest
            .abi
            .get_method_ref(access.method(), access.argument_count())
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "Specialized contract call method is absent from the resolved target",
                )
            })?;
        let method_entry = u32::try_from(method.offset).map_err(|_| {
            CoreError::invalid_operation("Specialized contract call method offset is negative")
        })?;
        if method_entry != access.entry_ip() {
            return Err(CoreError::invalid_operation(
                "Specialized contract call entry changed",
            ));
        }
        let result_count =
            usize::from(method.return_type != neo_primitives::ContractParameterType::Void);
        if result_count != access.result_count() {
            return Err(CoreError::invalid_operation(
                "Specialized contract call result shape changed",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/host_access_audit/facade.rs"]
mod tests;
