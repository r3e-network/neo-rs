use crate::error::VmError;

use super::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn exec_jump(&mut self, target: usize) -> Result<(), VmError> {
        self.jump_to(target)
    }

    pub(super) fn exec_jump_if_false(&mut self, target: usize) -> Result<(), VmError> {
        let cond = self.pop_bool()?;
        if !cond {
            self.jump_to(target)?;
        }
        Ok(())
    }
}
