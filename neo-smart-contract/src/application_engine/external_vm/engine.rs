use super::*;
impl ApplicationEngine {
    pub(super) fn prepare_external_vm_execution(&self) -> Option<ExternalVmExecution> {
        // The bridge remains an explicit experiment until it supports method slots
        // and beats canonical execution in no-profiler A/B replay. Keep the disable
        // switch as well so one experimental binary can run alternating comparisons.
        static ENABLE_EXTERNAL_VM: OnceLock<bool> = OnceLock::new();
        static DISABLE_EXTERNAL_VM: OnceLock<bool> = OnceLock::new();
        let enabled = *ENABLE_EXTERNAL_VM
            .get_or_init(|| crate::env_flags::env_flag_enabled("NEO_RS_ENABLE_EXTERNAL_VM", false));
        if !enabled
            || *DISABLE_EXTERNAL_VM
                .get_or_init(|| std::env::var_os("NEO_RS_DISABLE_EXTERNAL_VM").is_some())
        {
            return None;
        }
        if external_vm_depth_exhausted() {
            return None;
        }
        // Diagnostics must not change consensus semantics by forcing the jump-table path.
        if !self.pending_native_calls.is_empty() {
            return None;
        }
        let engine = self.vm_engine.engine();
        if matches!(engine.state(), VMState::HALT | VMState::FAULT) {
            return None;
        }
        let context = engine.current_context()?;
        if context.local_variables().is_some()
            || context.arguments().is_some()
            || context.try_stack().is_some()
            || context.has_static_fields()
        {
            return None;
        }
        let invocation_stack = engine.invocation_stack();
        let shares_caller_stack = invocation_stack
            .len()
            .checked_sub(2)
            .and_then(|caller_index| invocation_stack.get(caller_index))
            .is_some_and(|caller| context.shares_evaluation_stack_with(caller));
        let script = context.script().as_bytes();
        if script_uses_application_engine_host(script) {
            return None;
        }
        // The interpreter refuses values nested deeper than its limit, and the
        // bridge's own walk would never end on a value that contains itself.
        // Such a context remains on the canonical engine.
        if context
            .evaluation_stack()
            .iter()
            .any(|item| canonical_nesting_exceeds(item, neo_vm::MAX_COMPOUND_NESTING_DEPTH))
        {
            return None;
        }
        // Host-only objects cannot be manufactured without their runtime
        // identity. Such a context remains on the canonical engine; objects
        // returned later by syscalls use the bridge's handle table instead.
        let initial_stack = context
            .evaluation_stack()
            .iter()
            .cloned()
            .map(VmStackValue::try_from)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        Some(ExternalVmExecution {
            script: script.to_vec(),
            initial_stack,
            initial_ip: context.instruction_pointer(),
            result_stack_limit: external_result_stack_limit(context.rvcount(), shares_caller_stack),
            max_instructions: engine.limits().max_instructions,
            instructions_executed: engine.instructions_executed,
        })
    }
    pub(super) fn apply_external_vm_result(
        &mut self,
        result: neo_vm::ExecutionResult,
        instructions_executed: u64,
    ) -> VMState {
        self.vm_engine.engine_mut().instructions_executed = instructions_executed;
        match result.state {
            VMState::HALT => self.apply_external_vm_halt(instructions_executed),
            VMState::FAULT => {
                let message = external_vm_fault_message(result.fault_message, result.fault_ip);
                self.apply_external_vm_fault(message, instructions_executed)
            }
            other => {
                self.vm_engine.engine_mut().set_state(other);
                other
            }
        }
    }
    fn apply_external_vm_halt(&mut self, instructions_executed: u64) -> VMState {
        // Reuse the canonical RET path after synchronizing the evaluation stack.
        // This preserves rvcount checks, return propagation, dynamic-call null
        // normalization, child snapshot commits, and notification rollback.
        let result = neo_vm::jump_table::control::ret(
            self.vm_engine.engine_mut(),
            &neo_vm::Instruction::ret(),
        );
        match result {
            Ok(()) => self.vm_engine.engine().state(),
            Err(error) => self.apply_external_vm_fault(error.to_string(), instructions_executed),
        }
    }
    pub(super) fn apply_external_vm_fault(
        &mut self,
        message: String,
        instructions_executed: u64,
    ) -> VMState {
        if instructions_executed > 0 {
            self.vm_engine.engine_mut().instructions_executed = instructions_executed;
        }
        self.vm_engine
            .engine_mut()
            .set_uncaught_exception(Some(StackItem::from_byte_string(
                message.clone().into_bytes(),
            )));
        self.vm_engine.engine_mut().set_state(VMState::FAULT);
        self.fault_exception = Some(message);
        VMState::FAULT
    }
}
