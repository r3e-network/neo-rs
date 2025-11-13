use alloc::vec::Vec;

use neo_base::hash::Hash160;
use neo_vm::{VmError, VmValue};

use crate::{nef::CallFlags, runtime::contract_store, state::ContractState};

use super::ExecutionContext;

impl<'a> ExecutionContext<'a> {
    pub(crate) fn handle_contract_call(
        &mut self,
        hash: &Hash160,
        method: &str,
        call_flags: u8,
        args: Vec<VmValue>,
    ) -> Result<VmValue, VmError> {
        let requested = CallFlags::from_bits(call_flags).ok_or(VmError::InvalidType)?;
        if !self.current_call_flags.contains(requested) {
            return Err(VmError::NativeFailure("insufficient call flags"));
        }

        let contract = self.load_contract_state(hash)?;
        contract
            .manifest
            .abi
            .find_method(method, args.len())
            .ok_or(VmError::NativeFailure("method not found"))?;

        Err(VmError::NativeFailure(
            "contract calls are not supported yet",
        ))
    }

    fn load_contract_state(&self, hash: &Hash160) -> Result<ContractState, VmError> {
        match contract_store::load_contract_state(self.store(), hash) {
            Ok(Some(state)) => Ok(state),
            Ok(None) => Err(VmError::NativeFailure("contract not found")),
            Err(_) => Err(VmError::NativeFailure("contract lookup failed")),
        }
    }
}
