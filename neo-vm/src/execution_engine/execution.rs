//
// execution.rs - Main execution loop and instruction execution
//

use super::{ExecutionEngine, RunStats, VMState, VmError, VmResult};
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
            // neo-vm v3.11.2 `ExecuteNext` substitutes `Instruction.RET` when
            // the instruction pointer is past the end of the script and runs
            // it through the same pre/post instruction hooks as any other
            // instruction, without moving the instruction pointer. The post
            // hook receives `null` as the current instruction so the host
            // charges nothing for it.
            return self.execute_instruction(&Instruction::ret(), None, None);
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

        self.execute_instruction(&instruction, Some(&instruction), Some(context_index))
    }

    /// Runs one instruction through the pre hook, its handler and the post
    /// hook, matching the body of neo-vm v3.11.2 `ExecutionEngine.ExecuteNext`:
    ///
    /// ```text
    /// RunStats runStats = default;
    /// PreExecuteInstruction(instruction);
    /// try { JumpTable[instruction.OpCode](this, instruction, ref runStats); }
    /// finally { PostExecuteInstruction(currentInstruction, runStats); }
    /// ```
    ///
    /// `current_instruction` is `None` only for the implicit `RET` at the end of
    /// a script, and `context_index` is the context whose instruction pointer is
    /// advanced when the instruction did not jump.
    #[inline(always)]
    fn execute_instruction(
        &mut self,
        instruction: &Instruction,
        current_instruction: Option<&Instruction>,
        context_index: Option<usize>,
    ) -> VmResult<()> {
        self.run_stats = RunStats::default();
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

        // C# runs this in a `finally`, so it executes even when the handler
        // faulted; an error raised here replaces the handler's error, exactly
        // like an exception thrown from a `finally` block.
        let run_stats = self.run_stats;
        self.post_execute_instruction(current_instruction, &run_stats)?;

        // neo-vm v3.11.2 runs the post hook in a `finally` block and no longer
        // catches `CatchableException` in `ExecuteNext`: every handler error
        // faults the engine, and script-visible exceptions are only raised via
        // `ExecuteThrow` inside the handlers themselves (THROW, PICKITEM,
        // SETITEM, ENDFINALLY).
        result?;

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
    /// Matches neo-vm v3.11.2 `ExecutionEngine.PostExecuteInstruction`: the
    /// first check is `ReferenceCounter.PostExecuteInstruction`, which faults
    /// when the reference count is above `MaxStackSize`; the host then gets a
    /// chance to charge the instruction price and notify its diagnostic.
    #[inline(always)]
    fn post_execute_instruction(
        &mut self,
        instruction: Option<&Instruction>,
        run_stats: &RunStats,
    ) -> VmResult<()> {
        self.reference_counter
            .post_execute_instruction(self.limits.max_stack_size)?;

        if let Some(host) = self.interop_host {
            host.post_execute_instruction(self, instruction, run_stats)?;
        }

        Ok(())
    }
}
