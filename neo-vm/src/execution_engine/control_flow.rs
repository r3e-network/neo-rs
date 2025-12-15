//
// control_flow.rs - Jump, call, and syscall operations
//

use super::*;

impl ExecutionEngine {
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
}
