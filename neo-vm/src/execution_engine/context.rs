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

        if let Some(host) = self.interop_host {
            if let Some(new_context) = self.current_context().cloned() {
                host.on_context_loaded(self, &new_context)?;
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

        if let Some(host) = self.interop_host {
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

        // Check for zero-referenced items and clean them up. The return value is the count
        // of items checked, which is informational and doesn't require handling.
        let _ = self.reference_counter.check_zero_referred();

        Ok(context)
    }

    /// Creates a new context with the specified script.
    #[must_use]
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
}
