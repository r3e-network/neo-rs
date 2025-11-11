use crate::{error::VmError, value::VmValue};

use super::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn exec_store(&mut self, index: usize) -> Result<(), VmError> {
        let value = self.stack.pop().ok_or(VmError::StackUnderflow)?;
        if self.locals.len() <= index {
            self.locals.resize(index + 1, VmValue::Null);
        }
        self.locals[index] = value;
        Ok(())
    }

    pub(super) fn exec_load(&mut self, index: usize) -> Result<(), VmError> {
        let value = self.locals.get(index).cloned().unwrap_or(VmValue::Null);
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn exec_dup(&mut self, depth: usize) -> Result<(), VmError> {
        let len = self.stack.len();
        if depth >= len {
            return Err(VmError::StackUnderflow);
        }
        let value = self.stack[len - 1 - depth].clone();
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn exec_swap(&mut self, depth: usize) -> Result<(), VmError> {
        let len = self.stack.len();
        if depth >= len {
            return Err(VmError::StackUnderflow);
        }
        let top_index = len - 1;
        let other_index = len - 1 - depth;
        self.stack.swap(top_index, other_index);
        Ok(())
    }

    pub(super) fn exec_drop(&mut self) -> Result<(), VmError> {
        self.pop_value()?;
        Ok(())
    }

    pub(super) fn exec_over(&mut self) -> Result<(), VmError> {
        let value = self.peek_value(1)?;
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn exec_pick(&mut self, depth: usize) -> Result<(), VmError> {
        let value = self.peek_value(depth)?;
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn exec_roll(&mut self, depth: usize) -> Result<(), VmError> {
        let len = self.stack.len();
        if depth >= len {
            return Err(VmError::StackUnderflow);
        }
        let index = len - 1 - depth;
        let value = self.stack.remove(index);
        self.stack.push(value);
        Ok(())
    }
}
