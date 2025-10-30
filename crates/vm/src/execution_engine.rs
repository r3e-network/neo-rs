//! Execution engine module for the Neo Virtual Machine.
//!
//! This module provides the execution engine implementation for the Neo VM.

use crate::call_flags::CallFlags;
use crate::error::VmError;
use crate::error::VmResult;
use crate::evaluation_stack::EvaluationStack;
use crate::execution_context::ExecutionContext;
// use crate::gas_calculator::{GasCalculator, GasError}; // Removed - no C# counterpart
use crate::instruction::Instruction;
use crate::interop_service::{InteropHost, InteropService};
use crate::jump_table::JumpTable;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::stack_item::StackItem;

use std::convert::TryFrom;

const HASH_SIZE: usize = 32;

pub use crate::execution_engine_limits::ExecutionEngineLimits;
pub use crate::vm_state::VMState;

/// The execution engine for the Neo VM.
pub struct ExecutionEngine {
    /// The current state of the VM
    state: VMState,

    /// Flag indicating if the engine is in the middle of a jump
    pub is_jumping: bool,

    /// The jump table used to execute instructions
    jump_table: JumpTable,

    /// Restrictions on the VM
    limits: ExecutionEngineLimits,

    /// Used for reference counting of objects in the VM
    reference_counter: ReferenceCounter,

    /// Gas calculator for execution cost tracking (matches C# ApplicationEngine)
    // gas_calculator: GasCalculator, // Removed - no C# counterpart

    /// Optional interop service used for handling syscalls
    interop_service: Option<InteropService>,

    /// Host responsible for advanced syscall execution (ApplicationEngine)
    interop_host: Option<*mut dyn InteropHost>,

    /// Effective call flags for the current execution context
    call_flags: CallFlags,

    /// The invocation stack of the VM
    invocation_stack: Vec<ExecutionContext>,

    /// The stack to store the return values
    result_stack: EvaluationStack,

    /// The VM object representing the uncaught exception
    uncaught_exception: Option<StackItem>,
}

impl ExecutionEngine {
    /// Creates a new execution engine with the specified jump table.
    pub fn new(jump_table: Option<JumpTable>) -> Self {
        let reference_counter = ReferenceCounter::new();
        Self::new_with_limits(
            jump_table,
            reference_counter,
            ExecutionEngineLimits::default(),
        )
    }

    /// Creates a new execution engine with the specified reference counter and limits.
    pub fn new_with_limits(
        jump_table: Option<JumpTable>,
        reference_counter: ReferenceCounter,
        limits: ExecutionEngineLimits,
    ) -> Self {
        Self {
            state: VMState::BREAK,
            is_jumping: false,
            jump_table: jump_table.unwrap_or_else(JumpTable::default),
            limits,
            reference_counter: reference_counter.clone(),
            // gas_calculator: GasCalculator::new(1_000_000_000, 30), // Removed - no C# counterpart and fee factor
            interop_service: Some(InteropService::new()),
            interop_host: None,
            call_flags: CallFlags::ALL,
            invocation_stack: Vec::new(),
            result_stack: EvaluationStack::new(reference_counter),
            uncaught_exception: None,
        }
    }

    /// Returns the current state of the VM.
    pub fn state(&self) -> VMState {
        self.state
    }

    /// Sets the state of the VM.
    pub fn set_state(&mut self, state: VMState) {
        if self.state != state {
            self.state = state;
            self.on_state_changed();
        }
    }

    /// Called when the VM state changes.
    fn on_state_changed(&mut self) {}

    /// Called when an exception causes the VM to enter the FAULT state.
    fn on_fault(&mut self, err: VmError) {
        #[cfg(debug_assertions)]
        println!("ExecutionEngine fault: {:?}", err);
        self.set_state(VMState::FAULT);
    }

    /// Returns the reference counter.
    pub fn reference_counter(&self) -> &ReferenceCounter {
        &self.reference_counter
    }

    /// Returns the execution limits configured for this engine.
    pub fn limits(&self) -> &ExecutionEngineLimits {
        &self.limits
    }

    /// Returns the invocation stack.
    pub fn invocation_stack(&self) -> &[ExecutionContext] {
        &self.invocation_stack
    }

    /// Returns a mutable handle to the invocation stack.
    pub(crate) fn invocation_stack_mut(&mut self) -> &mut Vec<ExecutionContext> {
        &mut self.invocation_stack
    }

    /// Returns the current context, if any.
    pub fn current_context(&self) -> Option<&ExecutionContext> {
        self.invocation_stack.last()
    }

    /// Returns the current context (mutable), if any.
    pub fn current_context_mut(&mut self) -> Option<&mut ExecutionContext> {
        self.invocation_stack.last_mut()
    }

    /// Returns the entry context, if any.
    pub fn entry_context(&self) -> Option<&ExecutionContext> {
        self.invocation_stack.first()
    }

    /// Returns the result stack.
    pub fn result_stack(&self) -> &EvaluationStack {
        &self.result_stack
    }

    /// Returns the result stack (mutable).
    pub fn result_stack_mut(&mut self) -> &mut EvaluationStack {
        &mut self.result_stack
    }

    /// Returns the uncaught exception, if any.
    pub fn uncaught_exception(&self) -> Option<&StackItem> {
        self.uncaught_exception.as_ref()
    }

    /// Sets the uncaught exception.
    pub fn set_uncaught_exception(&mut self, exception: Option<StackItem>) {
        self.uncaught_exception = exception;
    }

    /// Gets the uncaught exception (matches C# UncaughtException property exactly).
    pub fn get_uncaught_exception(&self) -> Option<&StackItem> {
        self.uncaught_exception.as_ref()
    }

    /// Handles an exception by setting it as uncaught and transitioning to FAULT state.
    /// Returns true if the exception was handled, false otherwise.
    /// This matches C# exception handling behavior exactly.
    pub fn handle_exception(&mut self) -> bool {
        if self.uncaught_exception.is_some() {
            self.set_state(VMState::FAULT);
            true
        } else {
            false
        }
    }

    /// Sets the interop service used for syscall dispatch.
    pub fn set_interop_service(&mut self, service: InteropService) {
        self.interop_service = Some(service);
    }

    /// Clears the currently assigned interop service.
    pub fn clear_interop_service(&mut self) {
        self.interop_service = None;
    }

    /// Returns a reference to the configured interop service, if any.
    pub fn interop_service(&self) -> Option<&InteropService> {
        self.interop_service.as_ref()
    }

    /// Returns a mutable reference to the configured interop service, if any.
    pub fn interop_service_mut(&mut self) -> Option<&mut InteropService> {
        self.interop_service.as_mut()
    }

    /// Assigns the host responsible for advanced interop handling.
    pub fn set_interop_host(&mut self, host: *mut dyn InteropHost) {
        self.interop_host = Some(host);
    }

    /// Clears the registered interop host.
    pub fn clear_interop_host(&mut self) {
        self.interop_host = None;
    }

    /// Returns a mutable reference to the configured interop host, if any.
    pub fn interop_host_mut(&mut self) -> Option<&mut dyn InteropHost> {
        self.interop_host.map(|ptr| unsafe { &mut *ptr })
    }

    /// Returns the raw pointer to the configured interop host, if any.
    pub fn interop_host_ptr(&self) -> Option<*mut dyn InteropHost> {
        self.interop_host
    }

    /// Returns the effective call flags for this engine.
    pub fn call_flags(&self) -> CallFlags {
        self.call_flags
    }

    /// Sets the effective call flags for this engine.
    pub fn set_call_flags(&mut self, flags: CallFlags) {
        self.call_flags = flags;
    }

    /// Checks whether the required call flags are satisfied.
    pub fn has_call_flags(&self, required: CallFlags) -> bool {
        required.is_empty() || self.call_flags.contains(required)
    }

    /// Returns the jump table.
    pub fn jump_table(&self) -> &JumpTable {
        &self.jump_table
    }

    /// Returns the jump table (mutable).
    pub fn jump_table_mut(&mut self) -> &mut JumpTable {
        &mut self.jump_table
    }

    /// Sets the jump table.
    pub fn set_jump_table(&mut self, jump_table: JumpTable) {
        self.jump_table = jump_table;
    }

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

            if rvcount != 0 {
                let eval_stack_len = context.evaluation_stack().len();

                // Collect items to transfer
                let mut items = Vec::new();

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

                // Push to result stack in reverse order
                items.reverse();
                for item in items {
                    self.result_stack.push(item);
                }
            }

            // Remove the current context
            let context_index = self.invocation_stack.len() - 1;
            self.remove_context(context_index)?;

            // If no more contexts, halt
            if self.invocation_stack.is_empty() {
                self.set_state(VMState::HALT);
            }

            return Ok(());
        }

        // Get the current instruction
        let instruction = context.current_instruction()?;

        // Get the instruction handler
        let handler = self
            .jump_table
            .get_handler(instruction.opcode())
            .ok_or_else(|| {
                VmError::invalid_operation_msg(format!(
                    "No handler for opcode: {:?}",
                    instruction.opcode()
                ))
            })?;

        // Execute the instruction
        handler(self, &instruction)?;

        if !self.is_jumping {
            if let Some(context) = self.current_context_mut() {
                let next_position = context.instruction_pointer() + instruction.size();
                context.set_instruction_pointer(next_position);
            }
        }

        Ok(())
    }

    /// Executes the next instruction - C# API compatibility
    /// This matches the C# ExecutionEngine.ExecuteNextInstruction() method exactly
    pub fn execute_next_instruction(&mut self) -> VmResult<()> {
        self.execute_next()
    }

    /// Executes the next instruction in step mode (for debugging/testing).
    /// This matches C# ExecuteNext behavior for step-by-step execution.
    pub fn step_next(&mut self) -> VMState {
        if self.invocation_stack.is_empty() {
            self.set_state(VMState::HALT);
            return self.state;
        }

        // Try to execute the next instruction
        match self.execute_next_internal() {
            Ok(_) => {
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

    /// Internal implementation of execute_next.
    fn execute_next_internal(&mut self) -> VmResult<()> {
        // Get the current context
        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        // Get the current instruction
        let instruction = match context.current_instruction() {
            Ok(instruction) => instruction,
            Err(err) => {
                let error_msg = format!("{err:?}");
                if error_msg.contains("Instruction pointer is out of range") {
                    Instruction::ret()
                } else {
                    // Instruction parsing error - this should cause a FAULT
                    return Err(err);
                }
            }
        };

        self.pre_execute_instruction(&instruction)?;

        // Execute the instruction
        // We need to avoid borrowing conflicts by extracting the jump table temporarily
        // But we must preserve the custom handlers that were set up
        let jump_table = std::mem::take(&mut self.jump_table);
        let result = jump_table.execute(self, &instruction);

        match result {
            Ok(()) => {
                self.jump_table = jump_table;
            }
            Err(err) => {
                if self.limits.catch_engine_exceptions {
                    if let VmError::CatchableException { message } = &err {
                        self.jump_table = jump_table;
                        let exception = StackItem::from_byte_string(message.clone().into_bytes());
                        self.execute_throw(Some(exception))?;
                        return Ok(());
                    }
                }

                self.jump_table = jump_table;
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
    fn pre_execute_instruction(&mut self, instruction: &Instruction) -> VmResult<()> {
        // Consume gas for instruction execution (disabled - no C# counterpart)
        // if let Err(gas_error) = self.gas_calculator.consume_gas(instruction.opcode()) {
        //     return Err(VmError::invalid_operation_msg(format!(
        //         "Gas limit exceeded: {}",
        //         gas_error
        //     )));
        // }

        // Record instruction execution in metrics (disabled - no C# counterpart)
        // if let Ok(metrics) = std::env::var("NEO_VM_METRICS") {
        //     if metrics == "1" {
        //         crate::metrics::global_metrics().record_instruction();
        //     }
        // }
        if let Some(host_ptr) = self.interop_host {
            if let Some(context) = self.current_context().cloned() {
                unsafe { (&mut *host_ptr).pre_execute_instruction(self, &context, instruction)? };
            }
        }
        Ok(())
    }

    /// Called after executing an instruction.
    fn post_execute_instruction(&mut self, instruction: &Instruction) -> VmResult<()> {
        if self.reference_counter.count() < self.limits.max_stack_size as usize {
            if let Some(host_ptr) = self.interop_host {
                if let Some(context) = self.current_context().cloned() {
                    unsafe {
                        (&mut *host_ptr).post_execute_instruction(self, &context, instruction)?
                    };
                }
            }
            return Ok(());
        }

        let current = self.reference_counter.check_zero_referred();
        if current > self.limits.max_stack_size as usize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxStackSize exceed: {}/{}",
                current, self.limits.max_stack_size
            )));
        }

        if let Some(host_ptr) = self.interop_host {
            if let Some(context) = self.current_context().cloned() {
                unsafe { (&mut *host_ptr).post_execute_instruction(self, &context, instruction)? };
            }
        }

        Ok(())
    }

    /// Loads a context into the invocation stack.
    pub fn load_context(&mut self, context: ExecutionContext) -> VmResult<()> {
        if self.invocation_stack.len() >= self.limits.max_invocation_stack_size as usize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxInvocationStackSize exceed: {}",
                self.invocation_stack.len()
            )));
        }

        // Push the context onto the invocation stack
        self.invocation_stack.push(context);

        if let Some(host_ptr) = self.interop_host {
            if let Some(new_context) = self.current_context().cloned() {
                unsafe { (&mut *host_ptr).on_context_loaded(self, &new_context)? };
            }
        }

        Ok(())
    }

    /// Unloads a context from the invocation stack.
    pub fn unload_context(&mut self, context: &mut ExecutionContext) -> VmResult<()> {
        // Update current context
        if self.invocation_stack.is_empty() {
            // No more contexts
        } else {
            // Get the new current context
        }

        if let Some(static_fields) = context.static_fields_mut() {
            let current_static_fields = self
                .current_context()
                .and_then(|ctx| ctx.static_fields())
                .map(|sf| sf as *const _);

            if current_static_fields.is_none()
                || !std::ptr::eq(
                    current_static_fields.expect("Operation failed"),
                    static_fields,
                )
            {
                static_fields.clear_references();
            }
        }

        if let Some(local_variables) = context.local_variables_mut() {
            local_variables.clear_references();
        }

        if let Some(arguments) = context.arguments_mut() {
            arguments.clear_references();
        }

        if let Some(host_ptr) = self.interop_host {
            // SAFETY: The host pointer is managed by the caller and guaranteed to remain valid
            // while the execution engine lives. We only borrow it mutably here for the duration
            // of this callback.
            let host = unsafe { &mut *host_ptr };
            host.on_context_unloaded(self, context)?;
        }

        Ok(())
    }

    /// Removes a context from the invocation stack.
    pub fn remove_context(&mut self, context_index: usize) -> VmResult<ExecutionContext> {
        // Get the context
        if context_index >= self.invocation_stack.len() {
            return Err(VmError::invalid_operation_msg("Context index out of range"));
        }

        // Remove the context
        let mut context = self.invocation_stack.remove(context_index);

        if self.invocation_stack.is_empty() {
            self.set_state(VMState::HALT);
        }

        // Unload the context
        self.unload_context(&mut context)?;

        self.reference_counter.check_zero_referred();

        Ok(context)
    }

    /// Creates a new context with the specified script.
    pub fn create_context(
        &self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
    ) -> ExecutionContext {
        let mut context = ExecutionContext::new(script, rvcount, &self.reference_counter);
        context.set_instruction_pointer(initial_position);
        context
    }

    /// Loads a script and creates a new context.
    pub fn load_script(
        &mut self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
    ) -> VmResult<&ExecutionContext> {
        let context = self.create_context(script, rvcount, initial_position);
        self.load_context(context)?;

        self.current_context()
            .ok_or_else(|| VmError::InvalidOperation {
                operation: "load_script".to_string(),
                reason: "No current execution context after loading".to_string(),
            })
    }

    /// Returns the item at the specified index from the top of the current stack without removing it.
    pub fn peek(&self, index: usize) -> VmResult<&StackItem> {
        let context = self
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.evaluation_stack().peek(index)
    }

    /// Removes and returns the item at the top of the current stack.
    pub fn pop(&mut self) -> VmResult<StackItem> {
        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.evaluation_stack_mut().pop()
    }

    /// Pushes an item onto the top of the current stack.
    pub fn push(&mut self, item: StackItem) -> VmResult<()> {
        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.evaluation_stack_mut().push(item);
        Ok(())
    }

    /// Adds gas consumed (integrated with gas calculator)
    /// ApplicationEngine overrides this with additional gas tracking
    pub fn add_gas_consumed(&mut self, _gas: i64) -> VmResult<()> {
        // Gas tracking disabled - no C# counterpart
        // if let Err(gas_error) = self.gas_calculator.add_gas(gas) {
        //     return Err(VmError::invalid_operation_msg(format!(
        //         "Gas limit exceeded: {}",
        //         gas_error
        //     )));
        // }
        Ok(())
    }

    pub fn execute_jump(&mut self, position: i32) -> VmResult<()> {
        let script_len = self
            .current_context()
            .map(|ctx| ctx.script().len())
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        if position < 0 || (position as usize) >= script_len {
            return Err(VmError::InvalidJump(position));
        }

        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.set_instruction_pointer(position as usize);
        self.is_jumping = true;
        Ok(())
    }

    pub fn execute_jump_offset(&mut self, offset: i32) -> VmResult<()> {
        let current_ip = self
            .current_context()
            .map(|ctx| ctx.instruction_pointer())
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        let new_position = (current_ip as i64)
            .checked_add(offset as i64)
            .ok_or_else(|| VmError::InvalidJump(offset))?;

        if new_position < 0 || new_position > i32::MAX as i64 {
            return Err(VmError::InvalidJump(offset));
        }

        self.execute_jump(new_position as i32)
    }

    pub fn execute_call(&mut self, position: usize) -> VmResult<()> {
        let context = self
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        if position >= context.script().len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Call target out of range: {position}"
            )));
        }

        let new_context = context.clone_with_position(position);
        self.load_context(new_context)?;
        self.is_jumping = true;

        Ok(())
    }

    /// Handles system calls. Delegates to the configured interop service when available.
    pub fn on_syscall(&mut self, descriptor: u32) -> VmResult<()> {
        if self.interop_service.is_none() {
            return Err(VmError::invalid_operation_msg(format!(
                "Syscall {descriptor} not supported"
            )));
        }

        let mut service = self
            .interop_service
            .take()
            .expect("interop service should exist");
        let result = service.invoke_by_hash(self, descriptor);
        self.interop_service = Some(service);
        result
    }
    /// Executes a try block
    pub fn execute_try(&mut self, catch_offset: i32, finally_offset: i32) -> VmResult<()> {
        use crate::exception_handling_context::ExceptionHandlingContext;

        if catch_offset == 0 && finally_offset == 0 {
            return Err(VmError::invalid_operation_msg(
                "Both catch and finally offsets cannot be 0",
            ));
        }

        let max_try_nesting = self.limits.max_try_nesting_depth as usize;

        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        if context.try_stack_len() >= max_try_nesting {
            return Err(VmError::MaxTryNestingDepthExceeded);
        }

        let base_ip = i32::try_from(context.instruction_pointer()).map_err(|_| {
            VmError::invalid_operation_msg("Instruction pointer exceeds 32-bit range")
        })?;

        let catch_pointer = if catch_offset == 0 {
            -1
        } else {
            base_ip
                .checked_add(catch_offset)
                .ok_or_else(|| VmError::InvalidJump(catch_offset))?
        };

        let finally_pointer = if finally_offset == 0 {
            -1
        } else {
            base_ip
                .checked_add(finally_offset)
                .ok_or_else(|| VmError::InvalidJump(finally_offset))?
        };

        context.push_try_context(ExceptionHandlingContext::new(
            catch_pointer,
            finally_pointer,
        ));

        Ok(())
    }

    /// Executes an end try operation
    pub fn execute_end_try(&mut self, end_offset: i32) -> VmResult<()> {
        use crate::exception_handling_state::ExceptionHandlingState;

        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        if !context.has_try_context() {
            return Err(VmError::invalid_operation_msg("No try context"));
        }

        let current_try_snapshot = context
            .try_stack_last()
            .cloned()
            .expect("try stack should not be empty");

        let base_ip = i32::try_from(context.instruction_pointer()).map_err(|_| {
            VmError::invalid_operation_msg("Instruction pointer exceeds 32-bit range")
        })?;

        if current_try_snapshot.state() == ExceptionHandlingState::Finally {
            context.pop_try_context();
            let end_pointer = base_ip
                .checked_add(end_offset)
                .ok_or_else(|| VmError::InvalidJump(end_offset))?;
            let end_position =
                usize::try_from(end_pointer).map_err(|_| VmError::InvalidJump(end_pointer))?;
            context.set_instruction_pointer(end_position);
        } else if current_try_snapshot.has_finally() {
            let try_entry = context
                .try_stack_last_mut()
                .expect("try stack should not be empty");
            try_entry.set_state(ExceptionHandlingState::Finally);

            let end_pointer = base_ip
                .checked_add(end_offset)
                .ok_or_else(|| VmError::InvalidJump(end_offset))?;
            try_entry.set_end_pointer(end_pointer);

            let finally_pointer = try_entry.finally_pointer();
            let finally_position = usize::try_from(finally_pointer)
                .map_err(|_| VmError::InvalidJump(finally_pointer))?;
            context.set_instruction_pointer(finally_position);
        } else {
            context.pop_try_context();
            let end_pointer = base_ip
                .checked_add(end_offset)
                .ok_or_else(|| VmError::InvalidJump(end_offset))?;
            let end_position =
                usize::try_from(end_pointer).map_err(|_| VmError::InvalidJump(end_pointer))?;
            context.set_instruction_pointer(end_position);
        }

        self.is_jumping = true;

        Ok(())
    }

    /// Executes an end finally operation
    pub fn execute_end_finally(&mut self) -> VmResult<()> {
        use crate::exception_handling_state::ExceptionHandlingState;

        let end_pointer = {
            let context = self
                .current_context_mut()
                .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

            if !context.has_try_context() {
                return Err(VmError::invalid_operation_msg("No try stack"));
            }

            let current_try_snapshot = context
                .try_stack_last()
                .expect("try stack should not be empty");

            if current_try_snapshot.state() != ExceptionHandlingState::Finally {
                return Err(VmError::invalid_operation_msg(
                    "Invalid exception handling state",
                ));
            }

            let end_pointer = current_try_snapshot.end_pointer();
            context.pop_try_context();
            end_pointer
        };

        if self.uncaught_exception.is_some() {
            self.execute_throw(self.uncaught_exception.clone())?;
        } else {
            let end_position =
                usize::try_from(end_pointer).map_err(|_| VmError::InvalidJump(end_pointer))?;
            let context = self
                .current_context_mut()
                .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
            context.set_instruction_pointer(end_position);
            self.is_jumping = true;
        }

        Ok(())
    }

    /// Executes a throw operation
    pub fn execute_throw(&mut self, ex: Option<StackItem>) -> VmResult<()> {
        use crate::exception_handling_state::ExceptionHandlingState;

        self.uncaught_exception = ex;

        let mut idx = self.invocation_stack.len();
        while idx > 0 {
            idx -= 1;

            while self.invocation_stack.len() > idx + 1 {
                if let Some(mut ctx) = self.invocation_stack.pop() {
                    self.unload_context(&mut ctx)?;
                }
            }

            if self.invocation_stack.is_empty() {
                break;
            }

            if !self
                .invocation_stack
                .last()
                .expect("context should exist")
                .has_try_context()
            {
                if let Some(mut ctx) = self.invocation_stack.pop() {
                    self.unload_context(&mut ctx)?;
                }
                continue;
            }

            loop {
                let (state, has_finally, catch_pointer, finally_pointer) = {
                    let context = self.invocation_stack.last().expect("context should exist");

                    if let Some(try_context) = context.try_stack_last() {
                        (
                            try_context.state(),
                            try_context.has_finally(),
                            try_context.catch_pointer(),
                            try_context.finally_pointer(),
                        )
                    } else {
                        break;
                    }
                };

                if state == ExceptionHandlingState::Finally
                    || (state == ExceptionHandlingState::Catch && !has_finally)
                {
                    if let Some(context) = self.invocation_stack.last_mut() {
                        context.pop_try_context();
                    }
                    continue;
                }

                if state == ExceptionHandlingState::Try && catch_pointer >= 0 {
                    {
                        let context = self
                            .invocation_stack
                            .last_mut()
                            .expect("context should exist");
                        let try_context = context
                            .try_stack_last_mut()
                            .expect("try context should exist");
                        try_context.set_state(ExceptionHandlingState::Catch);
                        if let Some(exception) = self.uncaught_exception.clone() {
                            context.push(exception)?;
                        }
                        let catch_position = usize::try_from(catch_pointer)
                            .map_err(|_| VmError::InvalidJump(catch_pointer))?;
                        context.set_instruction_pointer(catch_position);
                    }
                    self.uncaught_exception = None;
                    self.is_jumping = true;
                    return Ok(());
                }

                {
                    let context = self
                        .invocation_stack
                        .last_mut()
                        .expect("context should exist");
                    let try_context = context
                        .try_stack_last_mut()
                        .expect("try context should exist");
                    try_context.set_state(ExceptionHandlingState::Finally);
                    let finally_position = usize::try_from(finally_pointer)
                        .map_err(|_| VmError::InvalidJump(finally_pointer))?;
                    context.set_instruction_pointer(finally_position);
                }
                self.is_jumping = true;
                return Ok(());
            }

            if let Some(mut ctx) = self.invocation_stack.pop() {
                self.unload_context(&mut ctx)?;
            }
        }

        if let Some(exception) = &self.uncaught_exception {
            Err(VmError::UnhandledException(exception.clone()))
        } else {
            Ok(())
        }
    }

    /// Gets gas consumed (disabled - no C# counterpart)
    /// ApplicationEngine overrides this with additional gas tracking
    pub fn gas_consumed(&self) -> i64 {
        // self.gas_calculator.gas_consumed() // Disabled - no C# counterpart
        0
    }

    /// Gets gas limit (disabled - no C# counterpart)
    /// ApplicationEngine overrides this with actual gas limit
    pub fn gas_limit(&self) -> i64 {
        // self.gas_calculator.gas_limit() // Disabled - no C# counterpart
        1_000_000_000 // Default gas limit
    }

    /// Gets current script hash (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual script hash tracking
    pub fn current_script_hash(&self) -> Option<&[u8]> {
        // Base implementation returns None
        None
    }

    /// Gets script container (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual script container
    pub fn get_script_container(&self) -> Option<&dyn std::any::Any> {
        // Base implementation returns None
        None
    }

    /// Gets the script container hash for signature verification.
    /// Returns the hash of the current transaction or block being executed.
    pub fn get_script_container_hash(&self) -> Vec<u8> {
        // Base implementation returns empty hash
        // ApplicationEngine overrides this with actual container hash
        vec![0u8; HASH_SIZE]
    }

    /// Gets the trigger type for this execution (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual trigger type
    pub fn get_trigger_type(&self) -> u8 {
        0x40
    }

    /// Emits a runtime log event (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual event emission
    pub fn emit_runtime_log_event(&mut self, _message: &str) -> VmResult<()> {
        // Base implementation does nothing
        Ok(())
    }

    /// Adds an execution log (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual log tracking
    pub fn add_execution_log(&mut self, _message: String) -> VmResult<()> {
        // Base implementation does nothing
        Ok(())
    }

    /// Gets transaction hash (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual transaction access
    pub fn get_transaction_hash(&self) -> Option<Vec<u8>> {
        // Base implementation returns None
        None
    }

    /// Gets current block hash (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual blockchain access
    pub fn get_current_block_hash(&self) -> Option<Vec<u8>> {
        // Base implementation returns None
        None
    }

    /// Gets storage item (stub implementation for base ExecutionEngine)
    /// ApplicationEngine overrides this with actual storage access
    pub fn get_storage_item(&self, _key: &[u8]) -> Option<Vec<u8>> {
        // Base implementation returns None
        None
    }
}

impl Drop for ExecutionEngine {
    fn drop(&mut self) {
        // Clear host references to avoid dangling pointers
        self.interop_host = None;
        // Clear the invocation stack
        self.invocation_stack.clear();
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;
    use crate::op_code::OpCode;

    #[test]
    fn test_execution_engine_creation() {
        let engine = ExecutionEngine::new(None);
        assert_eq!(engine.state(), VMState::BREAK);
        assert!(engine.invocation_stack().is_empty());
        assert!(engine.result_stack().is_empty());
        assert!(engine.uncaught_exception().is_none());
    }

    #[test]
    fn test_load_script() {
        let mut engine = ExecutionEngine::new(None);

        let script_bytes = vec![
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
        ];
        let script = Script::new_relaxed(script_bytes);

        {
            let context = engine
                .load_script(script, -1, 0)
                .expect("VM operation should succeed");

            assert_eq!(context.instruction_pointer(), 0);
            assert_eq!(context.rvcount(), -1);
        }

        assert_eq!(engine.invocation_stack().len(), 1);
    }

    #[test]
    fn test_set_state() {
        let mut engine = ExecutionEngine::new(None);
        assert_eq!(engine.state(), VMState::BREAK);

        engine.set_state(VMState::NONE);
        assert_eq!(engine.state(), VMState::NONE);

        engine.set_state(VMState::HALT);
        assert_eq!(engine.state(), VMState::HALT);

        engine.set_state(VMState::FAULT);
        assert_eq!(engine.state(), VMState::FAULT);
    }

    #[test]
    fn test_jump_table_methods() {
        let mut engine = ExecutionEngine::new(None);

        // Test jump_table getter
        let _jump_table = engine.jump_table();

        // Test jump_table_mut getter
        let _jump_table_mut = engine.jump_table_mut();

        // Test set_jump_table
        let new_jump_table = JumpTable::new();
        engine.set_jump_table(new_jump_table);
    }

    #[test]
    fn test_stack_operations() {
        let mut engine = ExecutionEngine::new(None);

        // Create a script with a few instructions
        let script_bytes = vec![
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
        ];
        let script = Script::new_relaxed(script_bytes);

        // Load the script
        engine
            .load_script(script, -1, 0)
            .expect("VM operation should succeed");

        // Push some items onto the stack
        engine
            .push(StackItem::from_int(1))
            .expect("VM operation should succeed");
        engine
            .push(StackItem::from_int(2))
            .expect("VM operation should succeed");
        engine
            .push(StackItem::from_int(3))
            .expect("VM operation should succeed");

        // Peek at the items
        assert_eq!(
            engine
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()
                .expect("VM operation should succeed"),
            num_bigint::BigInt::from(3)
        );
        assert_eq!(
            engine
                .peek(1)
                .expect("intermediate value should exist")
                .as_int()
                .expect("VM operation should succeed"),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            engine
                .peek(2)
                .expect("intermediate value should exist")
                .as_int()
                .expect("VM operation should succeed"),
            num_bigint::BigInt::from(1)
        );

        // Pop an item
        let item = engine.pop().unwrap();
        assert_eq!(
            item.as_int().expect("Operation failed"),
            num_bigint::BigInt::from(3)
        );

        // Peek again
        assert_eq!(
            engine
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()
                .expect("Operation failed"),
            num_bigint::BigInt::from(2)
        );
        assert_eq!(
            engine
                .peek(1)
                .expect("intermediate value should exist")
                .as_int()
                .expect("Operation failed"),
            num_bigint::BigInt::from(1)
        );
    }

    #[test]
    fn test_unload_context() {
        let mut engine = ExecutionEngine::new(None);

        // Create a script with a few instructions
        let script_bytes = vec![
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
        ];
        let script = Script::new_relaxed(script_bytes);

        // Load the script
        let context = engine
            .load_script(script, -1, 0)
            .expect("VM operation should succeed");

        // Push some items onto the stack
        engine
            .push(StackItem::from_int(1))
            .expect("VM operation should succeed");
        engine
            .push(StackItem::from_int(2))
            .expect("VM operation should succeed");

        // Remove the context
        let mut context = engine
            .remove_context(0)
            .expect("VM operation should succeed");

        // Check that the invocation stack is empty
        assert!(engine.invocation_stack().is_empty());

        // Check that the VM state is HALT
        assert_eq!(engine.state(), VMState::HALT);
    }
}
