//
// execution.rs - Main execution loop and instruction execution
//

use super::{ExecutionEngine, StackItem, VMState, VmError, VmResult};
use crate::Instruction;
use std::sync::{Arc, LazyLock};

/// Shared implicit-RET instruction used when the IP reaches end-of-script.
///
/// C# substitutes `Instruction.RET` in that case; reusing one `Arc` avoids
/// allocating a new instruction object on every function return / frame exit.
static IMPLICIT_RET: LazyLock<Arc<Instruction>> =
    LazyLock::new(|| Arc::new(Instruction::ret()));

impl<S> ExecutionEngine<S> {
    /// Starts execution of the VM.
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

        // Cache instruction size before execution to avoid re-fetching in move_next.
        let instruction_size = instruction.size();

        self.pre_execute_instruction(&instruction)?;

        if let Some(host) = self.interop_host {
            host.pre_execute_instruction(self, &instruction)?;
        }

        // Execute the instruction - direct array access for optimal dispatch
        let opcode = instruction.opcode();
        if let Some(profile) = &mut self.execution_profile {
            profile.record_opcode(opcode);
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
        if self.reference_counter.count() > self.limits.max_stack_size as usize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxStackSize exceed: {}/{}",
                self.reference_counter.count(),
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
