//
// context.rs - Context management (load, unload, remove, create)
//

use super::{ExecutionContext, ExecutionEngine, Script, VMState, VmError, VmResult};

impl ExecutionEngine {
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

        if let Some(host) = self.interop_host
            && let Some(new_context) = self.current_context().cloned()
        {
            host.on_context_loaded(self, &new_context)?;
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

        if context.has_static_fields() {
            let current_shares_static = self
                .current_context()
                .is_some_and(|current| context.shares_static_fields_with(current));
            if !current_shares_static {
                context.clear_static_fields_references();
            }
        }

        if let Some(local_variables) = context.local_variables_mut() {
            local_variables.clear_references();
        }

        if let Some(arguments) = context.arguments_mut() {
            arguments.clear_references();
        }

        if let Some(host) = self.interop_host {
            host.on_context_unloaded(self, context)?;
        }

        Ok(())
    }

    /// Removes a context from the invocation stack.
    ///
    /// The removed context is returned to the caller and keeps its evaluation
    /// stack contents, so its backing allocation is not recycled here. Contexts
    /// that are disposed of instead (`RET`, `THROW`) do hand their allocation
    /// back to the engine's allocator.
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

        Ok(context)
    }

    /// Creates a new context with the specified script.
    ///
    /// The new context starts on a recycled evaluation-stack allocation when
    /// the engine's [`StackAllocator`](crate::memory::StackAllocator) has one,
    /// and allocates otherwise.
    #[must_use]
    pub fn create_context(
        &self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
    ) -> ExecutionContext {
        let stack_buffer = self.stack_allocator.acquire();
        let mut context = ExecutionContext::new_with_stack_buffer(
            script,
            rvcount,
            &self.reference_counter,
            stack_buffer,
        );
        context.set_instruction_pointer(initial_position);
        context
    }

    /// Hands a dead context's evaluation-stack backing allocation back to the
    /// allocator when the context is its exclusive owner.
    ///
    /// Returns `true` when the allocation was retained for reuse. The context
    /// must be leaving the VM for good: its evaluation stack is left without
    /// storage, and the entries it held are released when the returned buffer
    /// is cleared, exactly as dropping the context would have released them.
    ///
    /// A context whose stack is still aliased - which is the case for every
    /// context created by `CALL`, because it shares the caller's stack - is left
    /// untouched, so a surviving alias can never observe a recycled allocation.
    pub(crate) fn recycle_context_stack(&self, context: &mut ExecutionContext) -> bool {
        let Some(buffer) = context.take_exclusive_evaluation_buffer() else {
            return false;
        };
        self.stack_allocator.recycle(buffer).is_ok()
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
                operation: "load_script".into(),
                reason: "No current execution context after loading".into(),
            })
    }
}
