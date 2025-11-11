use crate::{error::VmError, value::VmValue};

use super::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn exec_to_bool(&mut self) -> Result<(), VmError> {
        let value = self.pop_value()?;
        self.stack.push(VmValue::Bool(Self::to_bool(value)?));
        Ok(())
    }

    pub(super) fn exec_to_int(&mut self) -> Result<(), VmError> {
        let value = self.pop_value()?;
        self.stack.push(VmValue::Int(Self::to_int(value)?));
        Ok(())
    }

    pub(super) fn exec_to_bytes(&mut self) -> Result<(), VmError> {
        let value = self.pop_value()?;
        self.stack.push(VmValue::Bytes(Self::to_bytes(value)?));
        Ok(())
    }

    pub(super) fn exec_to_string(&mut self) -> Result<(), VmError> {
        let value = self.pop_value()?;
        self.stack.push(VmValue::String(Self::to_string(value)?));
        Ok(())
    }
}
