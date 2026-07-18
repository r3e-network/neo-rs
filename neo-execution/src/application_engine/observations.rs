//! Live, bounded observations used only by explicitly prepared shadow twins.

use super::*;
use crate::execution_artifact::{
    CallObservationOutcome, ContextObservationValue, DiagnosticObservationKind,
    ExecutionArtifactError, WitnessObservationOutcome,
};
use crate::host_access_audit::{
    ContractCallAccess, ContractCallKind, HostContextAccess, StorageRangeAccess,
};
use neo_primitives::Verifiable;

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    pub(crate) fn observe_storage_read(&self, key: StorageKey, value: Option<Vec<u8>>) {
        if self.snapshot_cache.has_read_observer() {
            return;
        }
        self.observe_execution(|journal| journal.record_storage_read(key, value));
    }

    pub(crate) fn observe_storage_range(
        &self,
        access: StorageRangeAccess,
        rows: &[(StorageKey, StorageItem)],
    ) {
        if self.snapshot_cache.has_read_observer() {
            self.observe_execution(|journal| journal.refine_last_storage_range_access(access));
            return;
        }
        self.observe_execution(|journal| journal.record_storage_range_borrowed(&access, rows));
    }

    pub(crate) fn observe_context(
        &self,
        access: HostContextAccess,
        value: ContextObservationValue,
    ) {
        self.observe_execution(|journal| journal.record_context(access, value));
    }

    pub(crate) fn observe_script_container_context(&self) {
        if !self.execution_observations_enabled() {
            return;
        }
        match self
            .script_container()
            .map(|container| container.hash())
            .transpose()
        {
            Ok(hash) => self.observe_context(
                HostContextAccess::ScriptContainer,
                ContextObservationValue::Hash256(hash),
            ),
            Err(error) => {
                self.fail_execution_observation(ExecutionArtifactError::InvalidHash {
                    kind: "observed script container",
                    message: error.to_string(),
                });
            }
        }
    }

    pub(crate) fn observe_witness(&self, hash: UInt160, outcome: WitnessObservationOutcome) {
        self.observe_execution(|journal| journal.record_witness(hash, outcome));
    }

    pub(crate) fn observe_fee_charge(&self, fee: u64) {
        self.observe_execution(|journal| journal.record_fee_charge(fee));
    }

    pub(crate) fn observe_completed_call(
        &self,
        access: ContractCallAccess,
        arguments: Vec<StackItem>,
        outcome: CallObservationOutcome,
    ) {
        self.observe_execution(|journal| journal.record_call(access, arguments, outcome));
    }

    pub(crate) fn begin_observed_contract_call(
        &self,
        context: &ExecutionContext<B>,
        contract: &ContractState,
        method: &ContractMethodDescriptor,
        effective_flags: CallFlags,
        arguments: &[StackItem],
    ) {
        let Some(observations) = &self.execution_observations else {
            return;
        };
        let Ok(entry_ip) = u32::try_from(method.offset) else {
            self.fail_execution_observation(ExecutionArtifactError::NumericOverflow {
                field: "observed contract call entry",
            });
            return;
        };
        let access = ContractCallAccess::new(
            ContractCallKind::Dynamic,
            neo_vm::ContractResolutionIdentity::new(
                contract.hash,
                contract.id,
                contract.update_counter,
                contract.nef.checksum,
            ),
            entry_ip,
            method.name.clone(),
            effective_flags,
            arguments.len(),
            usize::from(method.return_type != ContractParameterType::Void),
        );
        observations.lock().begin_call(
            Self::context_observation_identity(context),
            access,
            arguments.to_vec(),
        );
    }

    pub(crate) fn retarget_observed_contract_call(
        &self,
        context: &ExecutionContext<B>,
        kind: ContractCallKind,
        native_calling_script_hash: Option<UInt160>,
    ) {
        let Some(observations) = &self.execution_observations else {
            return;
        };
        observations.lock().retarget_call(
            Self::context_observation_identity(context),
            kind,
            native_calling_script_hash,
        );
    }

    pub(crate) fn complete_observed_contract_call(
        &self,
        engine: &ExecutionEngine<B>,
        context: &ExecutionContext<B>,
    ) {
        let Some(observations) = &self.execution_observations else {
            return;
        };
        let identity = Self::context_observation_identity(context);
        if engine
            .invocation_stack()
            .iter()
            .any(|remaining| Self::context_observation_identity(remaining) == identity)
        {
            return;
        }
        let outcome = match engine.uncaught_exception() {
            Some(exception) => CallObservationOutcome::Fault {
                message: String::new(),
                exception: Some(exception.clone()),
            },
            None => CallObservationOutcome::Returned(context.evaluation_stack().to_vec()),
        };
        observations.lock().complete_call(identity, outcome);
    }

    pub(crate) fn fault_pending_observed_calls(&self) {
        let Some(observations) = &self.execution_observations else {
            return;
        };
        let outcome = CallObservationOutcome::Fault {
            message: self.fault_exception.clone().unwrap_or_default(),
            exception: self.vm_engine.engine().uncaught_exception().cloned(),
        };
        observations.lock().complete_all_calls(outcome);
    }

    pub(crate) fn native_call_access(
        &self,
        native: &P::Contract,
        method: &crate::native_contract_cache::ResolvedNativeMethod,
        argument_count: usize,
    ) -> Option<ContractCallAccess> {
        if !self.execution_observations_enabled() {
            return None;
        }
        let block_height = self.current_block_index();
        let Some(contract) = native.contract_state(&self.protocol_settings, block_height) else {
            self.fail_execution_observation(ExecutionArtifactError::ObservationFailed {
                kind: "native call identity",
                message: "active native contract did not expose its contract state".to_string(),
            });
            return None;
        };
        let Some(descriptor) = contract
            .manifest
            .abi
            .get_method_ref(&method.method().name, argument_count)
        else {
            self.fail_execution_observation(ExecutionArtifactError::ObservationFailed {
                kind: "native call identity",
                message: "resolved native method is absent from its contract manifest".to_string(),
            });
            return None;
        };
        let Ok(entry_ip) = u32::try_from(descriptor.offset) else {
            self.fail_execution_observation(ExecutionArtifactError::NumericOverflow {
                field: "native call entry",
            });
            return None;
        };
        Some(ContractCallAccess::new(
            ContractCallKind::Native,
            neo_vm::ContractResolutionIdentity::new(
                contract.hash,
                contract.id,
                contract.update_counter,
                contract.nef.checksum,
            ),
            entry_ip,
            method.method().name.clone(),
            self.call_flags,
            argument_count,
            usize::from(method.method().return_type != ContractParameterType::Void),
        ))
    }

    pub(crate) fn observe_diagnostic_context(
        &self,
        kind: DiagnosticObservationKind,
        context: &ExecutionContext<B>,
    ) {
        if !self.execution_observations_enabled() || !self.diagnostic.enabled() {
            return;
        }
        self.observe_diagnostic(kind, Some(context), None);
    }

    pub(crate) fn observe_diagnostic_instruction(
        &self,
        kind: DiagnosticObservationKind,
        instruction: &Instruction,
    ) {
        if !self.execution_observations_enabled() || !self.diagnostic.enabled() {
            return;
        }
        self.observe_diagnostic(
            kind,
            self.vm_engine.engine().current_context(),
            Some(instruction),
        );
    }

    fn observe_diagnostic(
        &self,
        kind: DiagnosticObservationKind,
        context: Option<&ExecutionContext<B>>,
        instruction: Option<&Instruction>,
    ) {
        let (script_hash, instruction_pointer, stack) =
            context.map_or((None, None, Vec::new()), |context| {
                let state = context.state();
                let script_hash = state
                    .lock()
                    .script_hash
                    .or_else(|| UInt160::from_bytes(&context.script_hash()).ok());
                let instruction_pointer = u64::try_from(context.instruction_pointer()).ok();
                (
                    script_hash,
                    instruction_pointer,
                    context.evaluation_stack().to_vec(),
                )
            });
        if context.is_some() && instruction_pointer.is_none() {
            self.fail_execution_observation(ExecutionArtifactError::NumericOverflow {
                field: "diagnostic instruction pointer",
            });
            return;
        }
        let instruction = match instruction.map(Self::instruction_bytes).transpose() {
            Ok(Some(instruction)) => instruction,
            Ok(None) => Vec::new(),
            Err(error) => {
                self.fail_execution_observation(error);
                return;
            }
        };
        self.observe_execution(|journal| {
            journal.record_diagnostic(kind, script_hash, instruction_pointer, instruction, stack)
        });
    }

    fn instruction_bytes(instruction: &Instruction) -> Result<Vec<u8>, ExecutionArtifactError> {
        let opcode = instruction.opcode();
        let operand = instruction.operand();
        let mut bytes = Vec::with_capacity(instruction.size());
        bytes.push(opcode.byte());
        match opcode.operand_prefix() {
            1 => bytes.push(u8::try_from(operand.len()).map_err(|_| {
                ExecutionArtifactError::NumericOverflow {
                    field: "diagnostic instruction operand",
                }
            })?),
            2 => bytes.extend_from_slice(
                &u16::try_from(operand.len())
                    .map_err(|_| ExecutionArtifactError::NumericOverflow {
                        field: "diagnostic instruction operand",
                    })?
                    .to_le_bytes(),
            ),
            4 => bytes.extend_from_slice(
                &u32::try_from(operand.len())
                    .map_err(|_| ExecutionArtifactError::NumericOverflow {
                        field: "diagnostic instruction operand",
                    })?
                    .to_le_bytes(),
            ),
            _ => {}
        }
        bytes.extend_from_slice(operand);
        Ok(bytes)
    }

    fn context_observation_identity(context: &ExecutionContext<B>) -> usize {
        let state = context.state();
        Arc::as_ptr(&state) as usize
    }
}
