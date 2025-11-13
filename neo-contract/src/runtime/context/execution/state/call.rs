use alloc::vec::Vec;

use neo_base::hash::Hash160;
use neo_vm::{VmError, VmValue};

use crate::nef::CallFlags;

use super::ExecutionContext;

impl<'a> ExecutionContext<'a> {
    pub(crate) fn handle_contract_call(
        &mut self,
        _hash: &Hash160,
        _method: &str,
        call_flags: u8,
        _args: Vec<VmValue>,
    ) -> Result<VmValue, VmError> {
        let requested = CallFlags::from_bits(call_flags).ok_or(VmError::InvalidType)?;
        if !self.current_call_flags.contains(requested) {
            return Err(VmError::NativeFailure("insufficient call flags"));
        }
        Err(VmError::NativeFailure(
            "contract calls are not supported yet",
        ))
    }
}
