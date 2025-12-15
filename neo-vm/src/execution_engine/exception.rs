//
// exception.rs - Exception handling (try, catch, finally, throw)
//

use super::*;

impl ExecutionEngine {
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

        if let Some(exception) = self.uncaught_exception.clone() {
            self.set_state(VMState::FAULT);
            Err(VmError::UnhandledException(exception))
        } else {
            Ok(())
        }
    }
}
