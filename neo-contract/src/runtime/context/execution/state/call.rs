use alloc::vec::Vec;

use neo_base::{
    hash::Hash160,
    Bytes,
};
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

        let script_bytes = Bytes::from(contract.nef.script.clone());
        let _guard = ContractCallGuard::enter(self, &contract, requested, script_bytes);

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

struct ContractCallGuard<'a> {
    ctx: &'a mut ExecutionContext<'a>,
    prev_script: Bytes,
    prev_calling_hash: Option<Hash160>,
    prev_entry_hash: Option<Hash160>,
    prev_flags: CallFlags,
}

impl<'a> ContractCallGuard<'a> {
    fn enter(
        ctx: &'a mut ExecutionContext<'a>,
        contract: &ContractState,
        requested: CallFlags,
        script: Bytes,
    ) -> Self {
        let guard = Self {
            prev_script: ctx.script().clone(),
            prev_calling_hash: ctx.calling_script_hash(),
            prev_entry_hash: ctx.entry_script_hash(),
            prev_flags: ctx.call_flags(),
            ctx,
        };

        let previous_current = guard.ctx.current_script_hash();
        guard.ctx.set_calling_script_hash(previous_current);
        guard.ctx.set_script(script);
        guard.ctx.set_current_script_hash(contract.hash);
        guard.ctx.set_call_flags(requested);

        guard
    }
}

impl<'a> Drop for ContractCallGuard<'a> {
    fn drop(&mut self) {
        self.ctx.set_script(self.prev_script.clone());
        self.ctx.set_calling_script_hash(self.prev_calling_hash);
        self.ctx.entry_script_hash = self.prev_entry_hash;
        self.ctx.set_call_flags(self.prev_flags);
    }
}
