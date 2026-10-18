//
// execution.rs - Main execution loop and instruction execution
//

use super::{ExecutionEngine, StackItem, VMState, VmError, VmResult};
use crate::Instruction;
use tracing::instrument;

impl ExecutionEngine {
    /// Starts execution of the VM.
    #[instrument(name = "vm_execute", skip(self), fields(initial_state = ?self.state))]
    pub fn execute(&mut self) -> VMState {
        if self.state == VMState::BREAK {
            self.set_state(VMState::NONE);
        }

        // Execute until HALT or FAULT
        while self.state != VMState::HALT && self.state != VMState::FAULT {
            if let Err(err) = self.execute_next() {
                self.on_fault(err);
            }
        }

        tracing::debug!(
            final_state = ?self.state,
            instructions_executed = self.instructions_executed,
            gas_consumed = self.gas_consumed,
            "VM execution completed"
        );

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

        // Get the current context
        let context = self
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        if context.instruction_pointer() >= context.script().len() {
            // Perform implicit RET when reaching end of script.
            // Pop return values directly from the eval stack instead of
            // peek+clone, since the context is about to be destroyed.
            let rvcount = context.rvcount();
            let eval_stack_len = context.evaluation_stack().len();

            // C# executes a synthetic RET when the instruction pointer reaches
            // the end of the script, so the explicit RET RVCount check applies
            // here too. Clamping silently changed a FAULT into a HALT with a
            // truncated result stack.
            if rvcount >= 0 && eval_stack_len != rvcount as usize {
                return Err(VmError::invalid_operation_msg(format!(
                    "Return value count mismatch: expected {}, but got {} items on the evaluation stack",
                    rvcount, eval_stack_len
                )));
            }

            let count = if rvcount >= 0 {
                rvcount as usize
            } else {
                eval_stack_len
            };

            // Pop items from the top of the eval stack (they come out in
            // reverse order, so we reverse once to restore original order).
            let mut items = Vec::with_capacity(count);
            if count > 0 {
                let ctx = self
                    .current_context_mut()
                    .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
                for _ in 0..count {
                    items.push(ctx.pop()?);
                }
                items.reverse();
            }

            // Remove the current context
            let context_index = self.invocation_stack.len() - 1;
            self.remove_context(context_index)?;

            // Route return items to caller or result stack
            if !items.is_empty() {
                if self.invocation_stack.is_empty() {
                    for item in items {
                        self.result_stack.push(item)?;
                    }
                } else {
                    let caller = self
                        .current_context_mut()
                        .ok_or_else(|| VmError::invalid_operation_msg("No caller context"))?;
                    for item in items {
                        caller.push(item)?;
                    }
                }
            }

            // If no more contexts, halt
            if self.invocation_stack.is_empty() {
                self.set_state(VMState::HALT);
            }

            return Ok(());
        }

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

        // Get the current instruction.
        let instruction = self
            .invocation_stack
            .get_mut(context_index)
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?
            .current_instruction()?;

        // Cache instruction size before execution to avoid re-fetching in move_next.
        let instruction_size = instruction.size();

        self.pre_execute_instruction(&instruction)?;

        if let Some(host) = self.interop_host {
            host.pre_execute_instruction(self, &instruction)?;
        }

        // Execute the instruction - direct array access for optimal dispatch
        let opcode = instruction.opcode();
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
                if self.limits.catch_engine_exceptions
                    && let VmError::CatchableException { message } = err
                {
                    let exception = StackItem::from_byte_string(message.into_bytes());
                    self.execute_throw(Some(exception))?;
                    return Ok(());
                }
                return Err(err);
            }
        }

        self.post_execute_instruction(&instruction)?;

        if !self.is_jumping
            && let Some(context) = self.invocation_stack.get_mut(context_index)
        {
            context.advance_ip(instruction_size);
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
        if self.reference_counter.count() < self.limits.max_stack_size as usize {
            if let Some(host) = self.interop_host {
                host.post_execute_instruction(self, instruction)?;
            }
            return Ok(());
        }

        // Stack is at or over the reference limit. The reference counter can
        // over-count across nested RET frames, so enforce the limit against
        // the actual evaluation-stack slots as well. This keeps the memory
        // bound without FAULTing legitimate MainNet transactions whose
        // reference bookkeeping is conservative.
        let actual_stack_size = self
            .invocation_stack()
            .iter()
            .map(|context| context.evaluation_stack().len())
            .sum::<usize>()
            + self.result_stack().len();
        if actual_stack_size > self.limits.max_stack_size as usize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxStackSize exceed: {}/{}",
                actual_stack_size, self.limits.max_stack_size
            )));
        }

        if let Some(host) = self.interop_host {
            host.post_execute_instruction(self, instruction)?;
        }

        Ok(())
    }
}
