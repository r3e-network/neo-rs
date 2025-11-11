use alloc::vec::Vec;

use crate::value::VmValue;

use super::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn exec_push_int(&mut self, value: i64) {
        self.stack.push(VmValue::Int(value));
    }

    pub(super) fn exec_push_bool(&mut self, value: bool) {
        self.stack.push(VmValue::Bool(value));
    }

    pub(super) fn exec_push_bytes(&mut self, bytes: Vec<u8>) {
        self.stack.push(VmValue::Bytes(bytes.into()));
    }
}
