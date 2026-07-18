use super::super::*;
use crate::audited_application_host::AuditedApplicationHost;
use crate::host_access_audit::{
    HostAccessAudit, HostAccessDeclaration, HostAccessPolicy, HostAccessPolicyLimits,
};
use crate::specialization::{
    FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID, FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
    FLAMINGO_FACTORY_PAIR_KEY_ENTRY, SpecializationControl, SpecializationRouteDecision,
    flamingo_pair_key_candidate, try_flamingo_pair_key,
};
use neo_vm::{CandidateContract, HardforkTableIdentity, VmError};
use parking_lot::Mutex;
use std::sync::{Arc, LazyLock};

const MAX_PREPARED_FLAMINGO_IDENTITIES: usize = 32;

pub(in crate::application_engine) struct PreparedFlamingoCandidate {
    pub(in crate::application_engine) candidate: CandidateContract,
    pub(in crate::application_engine) policy: HostAccessPolicy,
}

static PREPARED_FLAMINGO_CANDIDATES: LazyLock<
    Mutex<Vec<(HardforkTableIdentity, Arc<PreparedFlamingoCandidate>)>>,
> = LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SpecializedExecutionResult {
    pub(crate) state: VMState,
    pub(crate) applied_frames: u64,
}

pub(in crate::application_engine) fn flamingo_cpu_fee_policy(
    candidate: &CandidateContract,
) -> Option<HostAccessPolicy> {
    let mut declarations = Vec::with_capacity(candidate.gas_steps().len() * 2);
    for step in candidate.gas_steps() {
        match step.amount {
            neo_vm::GasAmount::Fixed(units) => {
                declarations.push(HostAccessDeclaration::CpuFeeCharge(units));
            }
            neo_vm::GasAmount::Decision {
                when_true,
                when_false,
                ..
            } => {
                declarations.push(HostAccessDeclaration::CpuFeeCharge(when_true));
                declarations.push(HostAccessDeclaration::CpuFeeCharge(when_false));
            }
            neo_vm::GasAmount::ArgumentBytes { .. } => return None,
        }
    }
    HostAccessPolicy::try_new(declarations, HostAccessPolicyLimits::DEFAULT).ok()
}

pub(in crate::application_engine) fn prepared_flamingo_candidate(
    hardforks: HardforkTableIdentity,
) -> Option<Arc<PreparedFlamingoCandidate>> {
    let mut cache = PREPARED_FLAMINGO_CANDIDATES.lock();
    if let Some((_, prepared)) = cache.iter().find(|(identity, _)| *identity == hardforks) {
        return Some(Arc::clone(prepared));
    }

    let candidate = flamingo_pair_key_candidate(hardforks).ok()?;
    let policy = flamingo_cpu_fee_policy(&candidate)?;
    let prepared = Arc::new(PreparedFlamingoCandidate { candidate, policy });
    if cache.len() < MAX_PREPARED_FLAMINGO_IDENTITIES {
        cache.push((hardforks, Arc::clone(&prepared)));
    }
    Some(prepared)
}

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    /// Executes an isolated shadow-candidate engine.
    ///
    /// This is intentionally crate-private: the shadow pair runner owns the
    /// isolated overlay and keeps the ordinary twin authoritative. A
    /// shadow-only candidate can never enter this path through the normal
    /// [`Self::execute_allow_fault`] API.
    #[cfg(test)]
    pub(crate) fn execute_flamingo_shadow_candidate(
        &mut self,
        control: &SpecializationControl,
    ) -> SpecializedExecutionResult {
        if !matches!(
            control.route(
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
                FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
            ),
            SpecializationRouteDecision::Shadow
        ) {
            return SpecializedExecutionResult {
                state: self.execute_allow_fault(),
                applied_frames: 0,
            };
        }
        let Some(prepared) = prepared_flamingo_candidate(self.hardfork_plan_identity()) else {
            return SpecializedExecutionResult {
                state: self.execute_allow_fault(),
                applied_frames: 0,
            };
        };

        self.execute_prepared_flamingo_shadow_candidate(
            control,
            &prepared.candidate,
            &prepared.policy,
        )
    }

    pub(in crate::application_engine) fn execute_prepared_flamingo_shadow_candidate(
        &mut self,
        control: &SpecializationControl,
        candidate: &CandidateContract,
        policy: &HostAccessPolicy,
    ) -> SpecializedExecutionResult {
        debug_assert_eq!(
            candidate.identity().candidate_version(),
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION
        );
        let version = FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION;
        if !matches!(
            control.route(FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID, version),
            SpecializationRouteDecision::Shadow
        ) {
            return SpecializedExecutionResult {
                state: self.execute_allow_fault(),
                applied_frames: 0,
            };
        }

        let attached_here = self.attach_host();
        if self.state() == VMState::BREAK {
            self.vm_engine.engine_mut().set_state(VMState::NONE);
        }
        let mut applied_frames = 0u64;
        while !self.state().is_final() {
            let route_enabled = matches!(
                control.route(FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID, version),
                SpecializationRouteDecision::Shadow
            );
            let step = if route_enabled {
                self.try_apply_flamingo_pair_key_frame(candidate, policy)
            } else {
                Ok(false)
            };
            match step {
                Ok(true) => {
                    applied_frames = applied_frames.saturating_add(1);
                }
                Ok(false) => {
                    if let Err(error) = self.vm_engine.engine_mut().execute_next() {
                        self.vm_engine.engine_mut().handle_fault(error);
                    }
                }
                Err(error) => self.vm_engine.engine_mut().handle_fault(error),
            }
        }
        self.detach_host(attached_here);
        if self.state() == VMState::FAULT {
            self.finalize_fault();
        }
        SpecializedExecutionResult {
            state: self.state(),
            applied_frames,
        }
    }

    fn try_apply_flamingo_pair_key_frame(
        &mut self,
        candidate: &CandidateContract,
        policy: &HostAccessPolicy,
    ) -> Result<bool, VmError> {
        let contexts = self.vm_engine.engine().invocation_stack();
        let Some(context) = contexts.last() else {
            return Ok(false);
        };
        if context.instruction_pointer() != FLAMINGO_FACTORY_PAIR_KEY_ENTRY as usize
            || context.rvcount() != 0
            || context.local_variables().is_some()
            || context.arguments().is_some()
            || context.try_stack().is_some()
            || contexts.len() < 2
        {
            return Ok(false);
        }
        let caller = &contexts[contexts.len() - 2];
        if !context.shares_evaluation_stack_with(caller)
            || !context.shares_static_fields_with(caller)
            || !context.shares_state_with(caller)
        {
            return Ok(false);
        }
        let state = context.state();
        let state = state.lock();
        let Some(contract) = state.contract.as_deref() else {
            return Ok(false);
        };
        let fee_whitelisted = state.whitelisted;
        let Some(execution_key) = self.execution_plan_key(
            context.script(),
            context.instruction_pointer(),
            Some(contract),
        ) else {
            return Ok(false);
        };
        drop(state);
        if &execution_key != candidate.identity().execution() {
            return Ok(false);
        }

        let diagnostics_enabled =
            self.diagnostic.enabled() || self.vm_execution_profile().is_some();
        if diagnostics_enabled || fee_whitelisted {
            return Ok(false);
        }

        let (Ok(static_prefix), Ok(token_a), Ok(token_b)) = (
            context.load_static_field(1),
            context.peek(0),
            context.peek(1),
        ) else {
            return Ok(false);
        };
        let Ok(artifact) = try_flamingo_pair_key(&[token_a, token_b], &static_prefix, false, false)
        else {
            return Ok(false);
        };

        let instructions = artifact.instructions();
        let Some(next_instructions) = self.instructions_executed().checked_add(instructions) else {
            return Ok(false);
        };
        if next_instructions > self.execution_limits().max_instructions {
            return Ok(false);
        }
        let peak_delta = candidate.effects().stack().peak_reference_count_delta() as usize;
        if self.reference_count().saturating_add(peak_delta)
            > self.execution_limits().max_stack_size as usize
        {
            return Ok(false);
        }
        let Ok(fee_units) = i64::try_from(artifact.gas_units()) else {
            return Ok(false);
        };
        let Some(required_pico) = fee_units.checked_mul(i64::from(self.exec_fee_factor_raw()))
        else {
            return Ok(false);
        };
        let Some(next_fee) = self.fee_consumed_pico().checked_add(required_pico) else {
            return Ok(false);
        };
        if next_fee > self.fee_amount_pico() {
            return Ok(false);
        }

        let mut audit = HostAccessAudit::new(policy);
        {
            let mut host = AuditedApplicationHost::new(self, &mut audit);
            host.charge_cpu_fee_units(artifact.gas_units())
                .map_err(|error| VmError::invalid_operation_msg(error.to_string()))?;
        }
        audit
            .finish()
            .map_err(|error| VmError::invalid_operation_msg(error.to_string()))?;

        self.pop()
            .map_err(|error| VmError::invalid_operation_msg(error.to_string()))?;
        self.pop()
            .map_err(|error| VmError::invalid_operation_msg(error.to_string()))?;
        self.push(artifact.result().clone())
            .map_err(|error| VmError::invalid_operation_msg(error.to_string()))?;
        self.vm_engine.engine_mut().instructions_executed = next_instructions;
        let context_index = self.vm_engine.engine().invocation_stack().len() - 1;
        self.vm_engine.engine_mut().remove_context(context_index)?;
        self.vm_engine.engine_mut().is_jumping = true;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "../../tests/application_engine/specializations.rs"]
mod tests;
