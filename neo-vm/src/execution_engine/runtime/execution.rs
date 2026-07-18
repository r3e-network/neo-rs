//
// execution.rs - Main execution loop and instruction execution
//

use super::{ExecutionEngine, StackItem, VMState, VmError, VmResult};
use crate::Instruction;
use std::sync::{Arc, LazyLock};

fn reference_trace_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("NEO_TRACE_VM_REFERENCES")
            .ok()
            .is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
    })
}

/// Shared implicit-RET instruction used when the instruction pointer reaches
/// the end of a script. The object is immutable, so every synthetic return can
/// reuse one allocation while retaining the C# execution path and hook input.
static IMPLICIT_RET: LazyLock<Arc<Instruction>> = LazyLock::new(|| Arc::new(Instruction::ret()));

impl<S> ExecutionEngine<S> {
    /// Starts execution of the VM.
    pub fn execute(&mut self) -> VMState {
        if self.state == VMState::BREAK {
            self.set_state(VMState::NONE);
        }

        if !self.planned_execution_enabled {
            while self.state != VMState::HALT && self.state != VMState::FAULT {
                if let Err(err) = self.execute_next() {
                    self.on_fault(err);
                }
            }
            return self.state;
        }

        self.execute_planned_session()
    }

    /// Runs a mixed opt-in session without enlarging the ordinary loop's code.
    #[inline(never)]
    fn execute_planned_session(&mut self) -> VMState {
        while self.state != VMState::HALT && self.state != VMState::FAULT {
            let result = if self
                .current_context()
                .is_some_and(|context| context.execution_plan().is_some())
            {
                self.execute_planned_block()
            } else {
                self.execute_next()
            };
            if let Err(err) = result {
                self.on_fault(err);
            }
        }

        self.state
    }

    /// Executes the next instruction.
    pub fn execute_next(&mut self) -> VmResult<()> {
        if self.state == VMState::HALT || self.state == VMState::FAULT {
            return Ok(());
        }

        if self.invocation_stack.is_empty() {
            self.set_state(VMState::HALT);
            return Ok(());
        }

        self.is_jumping = false;

        self.execute_next_internal()
    }

    /// Executes the next instruction - C# API compatibility
    /// This matches the C# `ExecutionEngine.ExecuteNextInstruction()` method exactly
    pub fn execute_next_instruction(&mut self) -> VmResult<()> {
        self.execute_next()
    }

    /// Executes the next instruction in step mode (for debugging/testing).
    /// This matches C# `ExecuteNext` behavior for step-by-step execution.
    pub fn step_next(&mut self) -> VMState {
        if self.invocation_stack.is_empty() {
            self.set_state(VMState::HALT);
            return self.state;
        }

        // Try to execute the next instruction
        match self.execute_next_internal() {
            Ok(()) => {
                // unless we're already in HALT or FAULT state
                if self.state != VMState::HALT && self.state != VMState::FAULT {
                    self.set_state(VMState::BREAK);
                }
                self.state
            }
            Err(err) => {
                self.on_fault(err);
                self.state
            }
        }
    }

    /// Internal implementation of `execute_next`.
    #[inline(always)]
    fn execute_next_internal(&mut self) -> VmResult<()> {
        // Check instruction limit before executing
        let max_instructions = self.limits.max_instructions;
        if self.instructions_executed >= max_instructions {
            return Err(VmError::instruction_limit_exceeded(
                self.instructions_executed,
                max_instructions,
            ));
        }
        self.instructions_executed += 1;

        let context_index = self.invocation_stack.len() - 1;

        // C# substitutes Instruction.RET when CurrentInstruction is null at
        // end-of-script. Route that implicit return through the same handler,
        // hooks, limits, and RVCount validation as an explicit RET.
        let (instruction, has_current_instruction) = {
            let context = self
                .invocation_stack
                .get(context_index)
                .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
            if context.instruction_pointer() >= context.script().len() {
                (Arc::clone(&IMPLICIT_RET), false)
            } else {
                (context.current_instruction()?, true)
            }
        };

        // Keep the ordinary hot loop self-contained. This is deliberately
        // separate from planned execution so disabled mode preserves its
        // established release code shape and benchmark performance.
        let instruction_size = instruction.size();

        self.pre_execute_instruction(&instruction)?;

        if let Some(host) = self.interop_host {
            host.pre_execute_instruction(self, &instruction)?;
        }

        let opcode = instruction.opcode();
        if let Some(profile) = &mut self.execution_profile {
            let context = &self.invocation_stack[context_index];
            profile.record_opcode(context.script_hash(), context.script().len(), opcode);
        }
        let handler = self.jump_table.get_handler_by_u8(opcode.byte());
        let result = match handler {
            Some(h) => h(self, &instruction),
            None => Err(VmError::unsupported_operation(format!(
                "Unsupported opcode: {opcode:?}"
            ))),
        };

        match result {
            Ok(()) => {}
            Err(err) => {
                if self.limits.catch_engine_exceptions {
                    if let VmError::CatchableException { message } = &err {
                        let exception = StackItem::from_byte_string(message.clone().into_bytes());
                        self.execute_throw(Some(exception))?;
                        return Ok(());
                    }
                }
                return Err(err);
            }
        }

        self.post_execute_instruction(&instruction)?;

        if !self.is_jumping && has_current_instruction {
            if let Some(context) = self.invocation_stack.get_mut(context_index) {
                context.advance_ip(instruction_size);
            }
        }
        self.is_jumping = false;

        Ok(())
    }

    /// Executes one or more straight-line planned instructions while retaining
    /// one plan reference. Every opcode still uses the ordinary consensus
    /// handler, hooks, limits, fees, stacks, exceptions, and host interfaces.
    fn execute_planned_block(&mut self) -> VmResult<()> {
        if self.state == VMState::HALT || self.state == VMState::FAULT {
            return Ok(());
        }
        if self.invocation_stack.is_empty() {
            self.set_state(VMState::HALT);
            return Ok(());
        }

        let plan = self
            .current_context()
            .and_then(|context| context.execution_plan())
            .map(Arc::clone)
            .ok_or_else(|| VmError::invalid_operation_msg("Planned context has no plan"))?;

        loop {
            let context_index = self.invocation_stack.len() - 1;
            let (instruction_pointer, same_plan) = {
                let context = &self.invocation_stack[context_index];
                (
                    context.instruction_pointer(),
                    context
                        .execution_plan()
                        .is_some_and(|current| Arc::ptr_eq(current, &plan)),
                )
            };
            if !same_plan {
                return Ok(());
            }

            // A strict plan may be absent at the synthetic RET position. It may
            // also be conservatively unavailable for future plan formats. Fall
            // back before accounting or executing the current instruction.
            let Some(planned) = plan.instruction_at(instruction_pointer) else {
                return self.execute_next();
            };
            let control_flow = planned.control_flow();
            let expected_next = planned.next_ip() as usize;

            let max_instructions = self.limits.max_instructions;
            if self.instructions_executed >= max_instructions {
                return Err(VmError::instruction_limit_exceeded(
                    self.instructions_executed,
                    max_instructions,
                ));
            }
            self.instructions_executed += 1;
            self.is_jumping = false;
            self.execute_planned_instruction_body(context_index, planned.instruction(), true)?;

            if !matches!(control_flow, crate::PlannedControlFlow::Continue)
                || self.state == VMState::HALT
                || self.state == VMState::FAULT
                || self.invocation_stack.len() != context_index + 1
            {
                return Ok(());
            }
            let Some(context) = self.invocation_stack.get(context_index) else {
                return Ok(());
            };
            if context.instruction_pointer() != expected_next
                || !context
                    .execution_plan()
                    .is_some_and(|current| Arc::ptr_eq(current, &plan))
            {
                return Ok(());
            }
        }
    }

    #[inline(always)]
    fn execute_planned_instruction_body(
        &mut self,
        context_index: usize,
        instruction: &Instruction,
        has_current_instruction: bool,
    ) -> VmResult<()> {
        // Cache instruction size before execution to avoid re-fetching in move_next.
        let instruction_size = instruction.size();

        self.pre_execute_instruction(instruction)?;

        if let Some(host) = self.interop_host {
            host.pre_execute_instruction(self, instruction)?;
        }

        // Execute the instruction - direct array access for optimal dispatch
        let opcode = instruction.opcode();
        if let Some(profile) = &mut self.execution_profile {
            let context = &self.invocation_stack[context_index];
            profile.record_opcode(context.script_hash(), context.script().len(), opcode);
        }
        let handler = self.jump_table.get_handler_by_u8(opcode.byte());
        let result = match handler {
            Some(h) => h(self, instruction),
            None => Err(VmError::unsupported_operation(format!(
                "Unsupported opcode: {opcode:?}"
            ))),
        };

        match result {
            Ok(()) => {}
            Err(err) => {
                if self.limits.catch_engine_exceptions {
                    if let VmError::CatchableException { message } = &err {
                        let exception = StackItem::from_byte_string(message.clone().into_bytes());
                        self.execute_throw(Some(exception))?;
                        return Ok(());
                    }
                }
                return Err(err);
            }
        }

        self.post_execute_instruction(instruction)?;

        if !self.is_jumping && has_current_instruction {
            if let Some(context) = self.invocation_stack.get_mut(context_index) {
                context.advance_ip(instruction_size);
            }
        }
        self.is_jumping = false;

        Ok(())
    }

    /// Called before executing an instruction.
    ///
    /// Note: C# Neo VM does NOT have a pre-execution stack check.
    /// Only the post-execution check exists. Keeping this minimal for
    /// protocol compatibility.
    #[inline(always)]
    fn pre_execute_instruction(&mut self, _instruction: &Instruction) -> VmResult<()> {
        Ok(())
    }

    /// Called after executing an instruction.
    ///
    /// # Stack Overflow Detection Strategy (H-4)
    ///
    /// The VM uses a two-phase stack overflow detection:
    ///
    /// 1. **Pre-execution check** (in `pre_execute_instruction`): Triggers when stack
    ///    usage reaches 90% of limit, performing a thorough GC check before allowing
    ///    the instruction to execute.
    ///
    /// 2. **Post-execution check** (this method): Always runs after instruction execution.
    ///    Uses a fast path when under limit, and thorough GC check when at/over limit.
    ///
    /// This dual-check approach prevents:
    /// - Instructions that create many items from overflowing before post-check
    /// - Malicious scripts from exploiting the execution-to-check gap
    #[inline(always)]
    fn post_execute_instruction(&mut self, instruction: &Instruction) -> VmResult<()> {
        // C# v3.10.1 ExecutionEngine.PostExecuteInstruction → ReferenceCounter
        // .PostExecuteInstruction: the recursive stack-reference count is exact
        // (no GC sweep), so faulting is a plain `Count > MaxStackSize` check.
        // C# faults only when STRICTLY greater than MaxStackSize; reaching
        // exactly MaxStackSize is valid (`>=` would fault a contract C# HALTs).
        let references = self.reference_counter.count();
        if let Some(profile) = &mut self.execution_profile {
            profile.observe_reference_count(references);
        }
        if self.execution_profile.is_some() && reference_trace_enabled() {
            let (ip, eval_depth) = self
                .current_context()
                .map(|context| {
                    (
                        context.instruction_pointer(),
                        context.evaluation_stack().len(),
                    )
                })
                .unwrap_or((0, 0));
            tracing::info!(
                target: "neo::profile",
                ip,
                opcode = ?instruction.opcode(),
                references,
                eval_depth,
                "VM reference-count trace"
            );
        }
        if references > self.limits.max_stack_size as usize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxStackSize exceed: {references}/{}",
                self.limits.max_stack_size
            )));
        }

        if let Some(host) = self.interop_host
            && host.post_execute_instruction_enabled()
        {
            host.post_execute_instruction(self, instruction)?;
        }

        Ok(())
    }
}
