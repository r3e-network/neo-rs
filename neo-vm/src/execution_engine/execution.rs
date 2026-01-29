//
// execution.rs - Main execution loop and instruction execution
//

use super::{ExecutionEngine, Instruction, StackItem, VMState, VmError, VmResult};

impl ExecutionEngine {
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

        // Get the current context
        let context = self
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        if context.instruction_pointer() >= context.script().len() {
            // Perform implicit RET when reaching end of script
            // Get return value count from the current context
            let rvcount = context.rvcount();

            // Collect items to transfer before removing the context
            let mut items = Vec::new();
            if rvcount != 0 {
                let eval_stack_len = context.evaluation_stack().len();

                if rvcount == -1 {
                    // Return all items
                    for i in 0..eval_stack_len {
                        if let Ok(item) = context.evaluation_stack().peek(i) {
                            items.push(item.clone());
                        }
                    }
                } else if rvcount > 0 {
                    // Return specific number of items
                    let count = (rvcount as usize).min(eval_stack_len);
                    for i in 0..count {
                        if let Ok(item) = context.evaluation_stack().peek(i) {
                            items.push(item.clone());
                        }
                    }
                }

                // Preserve original order when pushing to target stack
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

        // Get the current context
        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        // Get the current instruction and snapshot the context for host hooks.
        // Note: ExecutionContext uses Arc<Script> which makes clone cheap (O(1)).
        // The reference counter is also Arc-based, so this is efficient.
        let instruction = context.current_instruction()?;
        let context_snapshot = context.clone();

        self.pre_execute_instruction(&instruction)?;
        if let Some(host_ptr) = self.interop_host {
            // SAFETY: See interop_host field documentation for invariants
            unsafe { (*host_ptr).pre_execute_instruction(self, &context_snapshot, &instruction)? };
        }

        // Execute the instruction - direct array access for optimal dispatch
        let opcode = instruction.opcode();
        // SAFETY: Opcode is guaranteed to be in range 0-255
        let handler = unsafe { *self.jump_table.handlers.get_unchecked(opcode as usize) };
        let result = match handler {
            Some(h) => h(self, &instruction),
            None => Err(VmError::unsupported_operation_msg(format!(
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

        if !self.is_jumping {
            if let Some(context) = self.current_context_mut() {
                let _ = context.move_next(); // Ignore errors here
            }
        }
        self.is_jumping = false;

        Ok(())
    }

    /// Called before executing an instruction.
    #[inline(always)]
    fn pre_execute_instruction(&mut self, _instruction: &Instruction) -> VmResult<()> {
        // SECURITY FIX (H-4): Pre-execution stack overflow check
        // Check stack size BEFORE executing instructions that could significantly
        // increase stack usage. This prevents attackers from exploiting the gap
        // between instruction execution and post-execution check.
        //
        // We use a threshold of 90% of max_stack_size to trigger early warning.
        // This gives headroom for instructions that create multiple items.
        let stack_threshold = (self.limits.max_stack_size as usize * 9) / 10;
        if self.reference_counter.count() >= stack_threshold {
            // Perform thorough check when approaching limit
            let current = self.reference_counter.check_zero_referred();
            if current >= self.limits.max_stack_size as usize {
                return Err(VmError::invalid_operation_msg(format!(
                    "MaxStackSize exceeded (pre-check): {}/{}",
                    current, self.limits.max_stack_size
                )));
            }
        }

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
            if let Some(host_ptr) = self.interop_host {
                if let Some(context) = self.current_context().cloned() {
                    // SAFETY: See interop_host field documentation for invariants
                    unsafe { (*host_ptr).post_execute_instruction(self, &context, instruction)? };
                }
            }
            return Ok(());
        }

        // Stack is at or over limit - perform thorough check with GC
        let current = self.reference_counter.check_zero_referred();
        if current > self.limits.max_stack_size as usize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxStackSize exceeded: {}/{}",
                current, self.limits.max_stack_size
            )));
        }

        if let Some(host_ptr) = self.interop_host {
            if let Some(context) = self.current_context().cloned() {
                // SAFETY: See interop_host field documentation for invariants
                unsafe { (*host_ptr).post_execute_instruction(self, &context, instruction)? };
            }
        }

        Ok(())
    }
}
