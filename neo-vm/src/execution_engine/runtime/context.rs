//
// context.rs - Context management (load, unload, remove, create)
//

use super::{ExecutionContext, ExecutionEngine, Script, VMState, VmError, VmResult};
use std::sync::Arc;

impl<S> ExecutionEngine<S> {
    /// Loads a context into the invocation stack.
    pub fn load_context(&mut self, context: ExecutionContext<S>) -> VmResult<()> {
        if self.invocation_stack.len() >= self.limits.max_invocation_stack_size as usize {
            return Err(VmError::invalid_operation_msg(format!(
                "MaxInvocationStackSize exceed: {}",
                self.invocation_stack.len()
            )));
        }

        if let Some(profile) = &self.execution_profile {
            context.set_stack_profile(profile.stack_handle());
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
    pub fn unload_context(&mut self, context: &mut ExecutionContext<S>) -> VmResult<()> {
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
    pub fn remove_context(&mut self, context_index: usize) -> VmResult<ExecutionContext<S>> {
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

        // C# v3.10.1 has no zero-referred GC sweep: the recursive stack-reference
        // count is exact and self-maintaining as stack/slot references are
        // released during unload_context.
        Ok(context)
    }

    /// Creates a new context with an explicit typed state value.
    pub fn create_context_with_state(
        &self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
        state: S,
    ) -> VmResult<ExecutionContext<S>> {
        let mut context =
            ExecutionContext::new_with_state(script, rvcount, &self.reference_counter, state);
        context.set_instruction_pointer(initial_position)?;
        Ok(context)
    }

    /// Creates a new context using a typed-state factory.
    pub fn create_context_with_state_factory<F: FnOnce() -> S>(
        &self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
        factory: F,
    ) -> VmResult<ExecutionContext<S>> {
        let mut context = ExecutionContext::new_with_state_factory(
            script,
            rvcount,
            &self.reference_counter,
            factory,
        );
        context.set_instruction_pointer(initial_position)?;
        Ok(context)
    }

    /// Loads a script and creates a new context with an explicit typed state value.
    pub fn load_script_with_state(
        &mut self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
        state: S,
    ) -> VmResult<&ExecutionContext<S>> {
        let context = self.create_context_with_state(script, rvcount, initial_position, state)?;
        self.load_context(context)?;

        self.current_context()
            .ok_or_else(|| VmError::InvalidOperation {
                operation: "load_script_with_state".into(),
                reason: "No current execution context after loading".into(),
            })
    }

    /// Loads a script and creates a new context using a typed-state factory.
    pub fn load_script_with_state_factory<F: FnOnce() -> S>(
        &mut self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
        factory: F,
    ) -> VmResult<&ExecutionContext<S>> {
        let context =
            self.create_context_with_state_factory(script, rvcount, initial_position, factory)?;
        self.load_context(context)?;

        self.current_context()
            .ok_or_else(|| VmError::InvalidOperation {
                operation: "load_script_with_state_factory".into(),
                reason: "No current execution context after loading".into(),
            })
    }
}

impl<S: Default> ExecutionEngine<S> {
    /// Creates a new context with the specified script.
    pub fn create_context(
        &self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
    ) -> VmResult<ExecutionContext<S>> {
        let mut context = ExecutionContext::new(script, rvcount, &self.reference_counter);
        context.set_instruction_pointer(initial_position)?;
        Ok(context)
    }

    /// Creates a context that retains an existing script allocation.
    pub fn create_context_from_script_arc(
        &self,
        script: Arc<Script>,
        rvcount: i32,
        initial_position: usize,
    ) -> VmResult<ExecutionContext<S>> {
        let mut context =
            ExecutionContext::new_from_script_arc(script, rvcount, &self.reference_counter);
        context.set_instruction_pointer(initial_position)?;
        Ok(context)
    }

    /// Loads a script and creates a new context.
    pub fn load_script(
        &mut self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
    ) -> VmResult<&ExecutionContext<S>> {
        let context = self.create_context(script, rvcount, initial_position)?;
        self.load_context(context)?;

        self.current_context()
            .ok_or_else(|| VmError::InvalidOperation {
                operation: "load_script".into(),
                reason: "No current execution context after loading".into(),
            })
    }
}
