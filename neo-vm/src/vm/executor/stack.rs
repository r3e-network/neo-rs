use alloc::{string::String, vec::Vec};

use neo_base::Bytes;

use crate::{error::VmError, value::VmValue};

use super::machine::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn pop_value(&mut self) -> Result<VmValue, VmError> {
        self.stack.pop().ok_or(VmError::StackUnderflow)
    }

    pub(super) fn peek_value(&self, depth: usize) -> Result<VmValue, VmError> {
        let len = self.stack.len();
        if depth >= len {
            return Err(VmError::StackUnderflow);
        }
        Ok(self.stack[len - 1 - depth].clone())
    }

    pub(super) fn pop_int(&mut self) -> Result<i64, VmError> {
        match self.pop_value()? {
            VmValue::Int(value) => Ok(value),
            _ => Err(VmError::InvalidType),
        }
    }

    pub(super) fn pop_bool(&mut self) -> Result<bool, VmError> {
        match self.pop_value()? {
            VmValue::Bool(value) => Ok(value),
            _ => Err(VmError::InvalidType),
        }
    }

    pub(super) fn collect_syscall_args(&mut self) -> Result<Vec<VmValue>, VmError> {
        Ok(self.stack.drain(..).collect())
    }

    pub(super) fn jump_to(&mut self, target: usize) -> Result<(), VmError> {
        if target >= self.instructions.len() {
            return Err(VmError::Fault);
        }
        self.ip = target;
        Ok(())
    }

    pub(super) fn equals(lhs: VmValue, rhs: VmValue) -> Result<bool, VmError> {
        Ok(lhs == rhs)
    }

    pub(super) fn to_bool(value: VmValue) -> Result<bool, VmError> {
        match value {
            VmValue::Bool(v) => Ok(v),
            VmValue::Int(v) => Ok(v != 0),
            VmValue::Bytes(bytes) => Ok(bytes.iter().any(|b| *b != 0)),
            VmValue::String(s) => Ok(!s.is_empty()),
            VmValue::Null => Ok(false),
        }
    }

    pub(super) fn to_int(value: VmValue) -> Result<i64, VmError> {
        match value {
            VmValue::Int(v) => Ok(v),
            VmValue::Bool(v) => Ok(if v { 1 } else { 0 }),
            VmValue::Bytes(bytes) => {
                if bytes.len() > 8 {
                    return Err(VmError::InvalidType);
                }
                let mut buf = [0u8; 8];
                buf[..bytes.len()].copy_from_slice(bytes.as_slice());
                Ok(i64::from_le_bytes(buf))
            }
            _ => Err(VmError::InvalidType),
        }
    }

    pub(super) fn to_bytes(value: VmValue) -> Result<Bytes, VmError> {
        match value {
            VmValue::Bytes(bytes) => Ok(bytes),
            VmValue::String(s) => Ok(Bytes::from(s.as_bytes())),
            VmValue::Bool(v) => Ok(Bytes::from(vec![if v { 1 } else { 0 }])),
            VmValue::Int(v) => Ok(Bytes::from(v.to_le_bytes().to_vec())),
            VmValue::Null => Ok(Bytes::default()),
        }
    }

    pub(super) fn to_string(value: VmValue) -> Result<String, VmError> {
        match value {
            VmValue::String(s) => Ok(s),
            VmValue::Bytes(bytes) => {
                String::from_utf8(bytes.into_vec()).map_err(|_| VmError::InvalidType)
            }
            VmValue::Bool(v) => Ok(if v { "true" } else { "false" }.to_string()),
            VmValue::Int(v) => Ok(v.to_string()),
            VmValue::Null => Ok(String::new()),
        }
    }
}
