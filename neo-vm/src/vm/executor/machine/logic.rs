use crate::{error::VmError, value::VmValue};

use super::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn exec_and(&mut self) -> Result<(), VmError> {
        let (rhs, lhs) = (self.pop_bool()?, self.pop_bool()?);
        self.stack.push(VmValue::Bool(lhs && rhs));
        Ok(())
    }

    pub(super) fn exec_or(&mut self) -> Result<(), VmError> {
        let (rhs, lhs) = (self.pop_bool()?, self.pop_bool()?);
        self.stack.push(VmValue::Bool(lhs || rhs));
        Ok(())
    }

    pub(super) fn exec_not(&mut self) -> Result<(), VmError> {
        let value = self.pop_value()?;
        match value {
            VmValue::Bool(v) => self.stack.push(VmValue::Bool(!v)),
            VmValue::Int(v) => self.stack.push(VmValue::Int(!v)),
            _ => return Err(VmError::InvalidType),
        }
        Ok(())
    }

    pub(super) fn exec_equal(&mut self) -> Result<(), VmError> {
        let rhs = self.pop_value()?;
        let lhs = self.pop_value()?;
        let result = Self::equals(lhs, rhs)?;
        self.stack.push(VmValue::Bool(result));
        Ok(())
    }

    pub(super) fn exec_not_equal(&mut self) -> Result<(), VmError> {
        let rhs = self.pop_value()?;
        let lhs = self.pop_value()?;
        let result = Self::equals(lhs, rhs)?;
        self.stack.push(VmValue::Bool(!result));
        Ok(())
    }

    pub(super) fn exec_greater(&mut self) -> Result<(), VmError> {
        let rhs = self.pop_int()?;
        let lhs = self.pop_int()?;
        self.stack.push(VmValue::Bool(lhs > rhs));
        Ok(())
    }

    pub(super) fn exec_less(&mut self) -> Result<(), VmError> {
        let rhs = self.pop_int()?;
        let lhs = self.pop_int()?;
        self.stack.push(VmValue::Bool(lhs < rhs));
        Ok(())
    }

    pub(super) fn exec_greater_or_equal(&mut self) -> Result<(), VmError> {
        let rhs = self.pop_int()?;
        let lhs = self.pop_int()?;
        self.stack.push(VmValue::Bool(lhs >= rhs));
        Ok(())
    }

    pub(super) fn exec_less_or_equal(&mut self) -> Result<(), VmError> {
        let rhs = self.pop_int()?;
        let lhs = self.pop_int()?;
        self.stack.push(VmValue::Bool(lhs <= rhs));
        Ok(())
    }

    pub(super) fn exec_xor(&mut self) -> Result<(), VmError> {
        let rhs = self.pop_value()?;
        let lhs = self.pop_value()?;
        match (lhs, rhs) {
            (VmValue::Bool(a), VmValue::Bool(b)) => self.stack.push(VmValue::Bool(a ^ b)),
            (VmValue::Int(a), VmValue::Int(b)) => self.stack.push(VmValue::Int(a ^ b)),
            _ => return Err(VmError::InvalidType),
        }
        Ok(())
    }

    pub(super) fn exec_shl(&mut self) -> Result<(), VmError> {
        let shift = self.pop_int()?;
        let value = self.pop_int()?;
        if shift < 0 {
            return Err(VmError::InvalidType);
        }
        self.stack.push(VmValue::Int(
            value.checked_shl(shift as u32).ok_or(VmError::Fault)?,
        ));
        Ok(())
    }

    pub(super) fn exec_shr(&mut self) -> Result<(), VmError> {
        let shift = self.pop_int()?;
        let value = self.pop_int()?;
        if shift < 0 {
            return Err(VmError::InvalidType);
        }
        self.stack.push(VmValue::Int(
            value.checked_shr(shift as u32).ok_or(VmError::Fault)?,
        ));
        Ok(())
    }
}
