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
            // neo-vm v3.10.1 `ExecuteNext` substitutes `Instruction.RET` when
            // the instruction pointer is past the end of the script and runs
            // it through the same pre/post instruction hooks as any other
            // instruction, without moving the instruction pointer.
            return self.execute_instruction(&Instruction::ret(), None);
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

        self.execute_instruction(&instruction, Some(context_index))
    }

    /// Runs one instruction through the pre hook, its handler and the post
    /// hook, matching the body of neo-vm v3.10.1 `ExecutionEngine.ExecuteNext`.
    ///
    /// `context_index` is the context whose instruction pointer is advanced
    /// when the instruction did not jump; it is `None` for the implicit `RET`
    /// at the end of a script.
    #[inline(always)]
    fn execute_instruction(
        &mut self,
        instruction: &Instruction,
        context_index: Option<usize>,
    ) -> VmResult<()> {
        self.pre_execute_instruction(instruction)?;

        if let Some(host) = self.interop_host {
            host.pre_execute_instruction(self, instruction)?;
        }

        // Execute the instruction - direct array access for optimal dispatch
        let opcode = instruction.opcode();
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
                if self.limits.catch_engine_exceptions
                    && let VmError::CatchableException { message } = err
                {
                    // A caught engine exception is handled like THROW and the
                    // instruction then completes normally, so the post
                    // instruction check below still runs.
                    let exception = StackItem::from_byte_string(message.into_bytes());
                    self.execute_throw(Some(exception))?;
                } else {
                    return Err(err);
                }
            }
        }

        self.post_execute_instruction(instruction)?;

        if !self.is_jumping
            && let Some(context_index) = context_index
            && let Some(context) = self.invocation_stack.get_mut(context_index)
        {
            context.advance_ip(instruction.size());
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
    /// Matches neo-vm v3.10.1 `ExecutionEngine.PostExecuteInstruction`: the
    /// only check is `ReferenceCounter.PostExecuteInstruction`, which faults
    /// when the reference count is above `MaxStackSize`. Nothing is collected.
    #[inline(always)]
    fn post_execute_instruction(&mut self, instruction: &Instruction) -> VmResult<()> {
        self.reference_counter
            .post_execute_instruction(self.limits.max_stack_size)?;

        if let Some(host) = self.interop_host {
            host.post_execute_instruction(self, instruction)?;
        }

        Ok(())
    }
}
