use crate::{error::VmError, value::VmValue};

use super::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn exec_add(&mut self) -> Result<(), VmError> {
        let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
        self.stack.push(VmValue::Int(lhs + rhs));
        Ok(())
    }

    pub(super) fn exec_sub(&mut self) -> Result<(), VmError> {
        let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
        self.stack.push(VmValue::Int(lhs - rhs));
        Ok(())
    }

    pub(super) fn exec_mul(&mut self) -> Result<(), VmError> {
        let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
        self.stack.push(VmValue::Int(lhs * rhs));
        Ok(())
    }

    pub(super) fn exec_div(&mut self) -> Result<(), VmError> {
        let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
        if rhs == 0 {
            return Err(VmError::DivisionByZero);
        }
        self.stack.push(VmValue::Int(lhs / rhs));
        Ok(())
    }

    pub(super) fn exec_mod(&mut self) -> Result<(), VmError> {
        let (rhs, lhs) = (self.pop_int()?, self.pop_int()?);
        if rhs == 0 {
            return Err(VmError::DivisionByZero);
        }
        self.stack.push(VmValue::Int(lhs % rhs));
        Ok(())
    }

    pub(super) fn exec_negate(&mut self) -> Result<(), VmError> {
        let value = self.pop_int()?;
        self.stack.push(VmValue::Int(-value));
        Ok(())
    }

    pub(super) fn exec_inc(&mut self) -> Result<(), VmError> {
        let value = self.pop_int()?;
        self.stack.push(VmValue::Int(value + 1));
        Ok(())
    }

    pub(super) fn exec_dec(&mut self) -> Result<(), VmError> {
        let value = self.pop_int()?;
        self.stack.push(VmValue::Int(value - 1));
        Ok(())
    }

    pub(super) fn exec_sign(&mut self) -> Result<(), VmError> {
        let value = self.pop_int()?;
        let sign = if value > 0 {
            1
        } else if value < 0 {
            -1
        } else {
            0
        };
        self.stack.push(VmValue::Int(sign));
        Ok(())
    }

    pub(super) fn exec_abs(&mut self) -> Result<(), VmError> {
        let value = self.pop_int()?;
        let abs = value.checked_abs().ok_or(VmError::Fault)?;
        self.stack.push(VmValue::Int(abs));
        Ok(())
    }
}
