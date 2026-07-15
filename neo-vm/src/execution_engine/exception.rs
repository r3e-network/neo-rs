//
// exception.rs - Exception handling (try, catch, finally, throw)
//

use super::{ExecutionEngine, StackItem, TryFrom, VmError, VmResult};

impl<S> ExecutionEngine<S> {
    /// Executes a try block
    pub fn execute_try(&mut self, catch_offset: i32, finally_offset: i32) -> VmResult<()> {
        use crate::ExceptionHandlingContext;

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
                .ok_or(VmError::InvalidJump(catch_offset))?
        };

        let finally_pointer = if finally_offset == 0 {
            -1
        } else {
            base_ip
                .checked_add(finally_offset)
                .ok_or(VmError::InvalidJump(finally_offset))?
        };

        context.push_try_context(ExceptionHandlingContext::new(
            catch_pointer,
            finally_pointer,
        ));

        Ok(())
    }

    /// Executes an end try operation
    pub fn execute_end_try(&mut self, end_offset: i32) -> VmResult<()> {
        use crate::ExceptionHandlingState;

        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        if !context.has_try_context() {
            return Err(VmError::invalid_operation_msg("No try context"));
        }

        let current_try_snapshot = context
            .try_stack_last()
            .cloned()
            .ok_or_else(|| VmError::invalid_operation_msg("No try context"))?;

        let base_ip = i32::try_from(context.instruction_pointer()).map_err(|_| {
            VmError::invalid_operation_msg("Instruction pointer exceeds 32-bit range")
        })?;

        // C# ExecuteEndTry faults if ENDTRY is reached while already in the FINALLY
        // state (JumpTable.Control.cs:585-586). Treating it as a normal pop+jump
        // would diverge HALT/FAULT from C# for malformed/adversarial scripts.
        if current_try_snapshot.state() == ExceptionHandlingState::Finally {
            return Err(VmError::invalid_operation_msg(
                "The opcode ENDTRY can't be executed in a FINALLY block",
            ));
        }
        if current_try_snapshot.has_finally() {
            let try_entry = context
                .try_stack_last_mut()
                .ok_or_else(|| VmError::invalid_operation_msg("No try context"))?;
            try_entry.set_state(ExceptionHandlingState::Finally);

            let end_pointer = base_ip
                .checked_add(end_offset)
                .ok_or(VmError::InvalidJump(end_offset))?;
            try_entry.set_end_pointer(end_pointer);

            let finally_pointer = try_entry.finally_pointer();
            let finally_position = usize::try_from(finally_pointer)
                .map_err(|_| VmError::InvalidJump(finally_pointer))?;
            context.set_instruction_pointer(finally_position)?;
        } else {
            context.pop_try_context();
            let end_pointer = base_ip
                .checked_add(end_offset)
                .ok_or(VmError::InvalidJump(end_offset))?;
            let end_position =
                usize::try_from(end_pointer).map_err(|_| VmError::InvalidJump(end_pointer))?;
            context.set_instruction_pointer(end_position)?;
        }

        self.is_jumping = true;

        Ok(())
    }

    /// Executes an end finally operation
    pub fn execute_end_finally(&mut self) -> VmResult<()> {
        use crate::ExceptionHandlingState;

        let end_pointer = {
            let context = self
                .current_context_mut()
                .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

            if !context.has_try_context() {
                return Err(VmError::invalid_operation_msg("No try stack"));
            }

            let current_try_snapshot = context
                .try_stack_last()
                .ok_or_else(|| VmError::invalid_operation_msg("No try stack"))?;

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
            context.set_instruction_pointer(end_position)?;
            self.is_jumping = true;
        }

        Ok(())
    }

    /// Executes a throw operation
    pub fn execute_throw(&mut self, ex: Option<StackItem>) -> VmResult<()> {
        use crate::ExceptionHandlingState;

        self.uncaught_exception = ex;

        // C# scans the invocation stack first and only unloads the frames above
        // a handler after finding one. If no handler exists, every frame stays
        // loaded for fault diagnostics.
        let mut handler_index = None;
        for idx in (0..self.invocation_stack.len()).rev() {
            loop {
                let (state, has_finally) = {
                    let Some(context) = self.invocation_stack.get(idx) else {
                        break;
                    };

                    if let Some(try_context) = context.try_stack_last() {
                        (try_context.state(), try_context.has_finally())
                    } else {
                        break;
                    }
                };

                if state == ExceptionHandlingState::Finally
                    || (state == ExceptionHandlingState::Catch && !has_finally)
                {
                    if let Some(context) = self.invocation_stack.get_mut(idx) {
                        context.pop_try_context();
                    }
                    continue;
                }
                handler_index = Some(idx);
                break;
            }

            if handler_index.is_some() {
                break;
            }
        }

        let Some(handler_index) = handler_index else {
            return Err(VmError::UnhandledException(
                self.uncaught_exception.clone().unwrap_or(StackItem::Null),
            ));
        };

        while self.invocation_stack.len() > handler_index + 1 {
            if let Some(mut context) = self.invocation_stack.pop() {
                self.unload_context(&mut context)?;
            }
        }

        let context = self
            .invocation_stack
            .last_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        let (state, catch_pointer, finally_pointer) = {
            let try_context = context
                .try_stack_last()
                .ok_or_else(|| VmError::invalid_operation_msg("No try context"))?;
            (
                try_context.state(),
                try_context.catch_pointer(),
                try_context.finally_pointer(),
            )
        };

        if state == ExceptionHandlingState::Try && catch_pointer >= 0 {
            context
                .try_stack_last_mut()
                .ok_or_else(|| VmError::invalid_operation_msg("No try context"))?
                .set_state(ExceptionHandlingState::Catch);
            if let Some(exception) = self.uncaught_exception.clone() {
                context.push(exception)?;
            }
            let catch_position =
                usize::try_from(catch_pointer).map_err(|_| VmError::InvalidJump(catch_pointer))?;
            context.set_instruction_pointer(catch_position)?;
            self.uncaught_exception = None;
        } else {
            context
                .try_stack_last_mut()
                .ok_or_else(|| VmError::invalid_operation_msg("No try context"))?
                .set_state(ExceptionHandlingState::Finally);
            let finally_position = usize::try_from(finally_pointer)
                .map_err(|_| VmError::InvalidJump(finally_pointer))?;
            context.set_instruction_pointer(finally_position)?;
        }

        self.is_jumping = true;
        Ok(())
    }
}
