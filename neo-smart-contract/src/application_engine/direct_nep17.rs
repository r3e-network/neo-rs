//! Direct dispatch for strictly parsed native NEP-17 transfers.
//!
//! The entry stack-building instructions are not interpreted, but their fees
//! are charged in order. Native ABI entry, argument serialization, native
//! return conversion, context unload, ASSERT, and snapshot commit reuse the
//! canonical handlers so direct dispatch does not become a second state model.

use super::*;
use neo_vm::fast_path::TransferParams;
use neo_vm::jump_table::control;

const SYSTEM_CONTRACT_CALL_PRICE: i64 = 1 << 15;

impl ApplicationEngine {
    /// Returns whether direct native dispatch is applicable without changing
    /// storage or execution state. Any uncertainty selects the canonical VM.
    pub fn can_execute_direct_nep17_transfer(&mut self, params: &TransferParams) -> bool {
        if self.state() != VMState::BREAK || self.vm_engine.engine().invocation_stack().len() != 1 {
            return false;
        }
        let Ok(contract_hash) = UInt160::from_bytes(&params.contract_hash) else {
            return false;
        };
        let Ok(recipient) = UInt160::from_bytes(&params.to) else {
            return false;
        };
        let block_height = self.current_block_index();
        let Some(native) = self.native_registry.get(&contract_hash) else {
            return false;
        };
        if !matches!(native.name(), "NeoToken" | "GasToken")
            || !native.is_active(&self.protocol_settings, block_height)
        {
            return false;
        }
        // A contract recipient can queue onNEP17Payment. Keep that execution in
        // the canonical VM instead of attempting an in-place fallback.
        if self.contracts.contains_key(&recipient)
            || self.native_registry.get(&recipient).is_some()
            || self
                .native_state_reader
                .contract(self.snapshot_cache.as_ref(), &recipient)
                .ok()
                .flatten()
                .is_some()
        {
            return false;
        }
        // Faun fixed-fee whitelisting changes both wrapper and native fees.
        // The first direct-dispatch version deliberately declines these calls.
        if self
            .protocol_settings
            .is_hardfork_enabled(Hardfork::HfFaun, block_height)
            && !matches!(
                self.native_state_reader.whitelisted_fee(
                    self.snapshot_cache.as_ref(),
                    &contract_hash,
                    "transfer",
                    4,
                ),
                Ok(None)
            )
        {
            return false;
        }
        true
    }

    /// Executes a previously parsed and eligibility-checked transfer without
    /// interpreting the entry stack-building instructions.
    ///
    /// This method never retries on the canonical engine. Once direct dispatch
    /// starts, errors become canonical VM faults and the ledger applies its
    /// normal transaction-overlay rollback rules.
    pub fn execute_direct_nep17_transfer(&mut self, params: &TransferParams) -> VMState {
        self.attach_host();
        let result = self.execute_direct_nep17_transfer_inner(params);
        if let Err(error) = result {
            self.vm_engine.engine_mut().fault(error);
        }
        let state = self.state();
        if state == VMState::FAULT {
            self.capture_fault_exception_from_vm();
        }
        state
    }

    fn execute_direct_nep17_transfer_inner(&mut self, params: &TransferParams) -> VmResult<()> {
        for instruction in &params.entry_instructions {
            self.set_direct_instruction_pointer(instruction.offset)?;
            self.add_cpu_fee(Self::get_opcode_price(instruction.opcode.byte()))
                .map_err(map_core_error_to_vm_error)?;
            match instruction.opcode {
                OpCode::SYSCALL => self.direct_native_contract_call(params)?,
                OpCode::ASSERT => {
                    let assertion = self.vm_engine.engine_mut().pop()?;
                    if !assertion.as_boolean()? {
                        return Err(VmError::AssertFailed);
                    }
                }
                _ => {}
            }
        }
        self.set_direct_instruction_pointer(params.entry_end_offset)?;
        // The entry script ends with an implicit RET: no RET opcode fee, but
        // canonical result-stack transfer and snapshot commit are required.
        control::ret(self.vm_engine.engine_mut(), &Instruction::ret())
    }

    fn direct_native_contract_call(&mut self, params: &TransferParams) -> VmResult<()> {
        self.add_cpu_fee(SYSTEM_CONTRACT_CALL_PRICE)
            .map_err(map_core_error_to_vm_error)?;
        let contract_hash = UInt160::from_bytes(&params.contract_hash)
            .map_err(|error| contract_call_vm_error(error.to_string()))?;
        let args = vec![
            StackItem::from_byte_string(params.from.clone()),
            StackItem::from_byte_string(params.to.clone()),
            params.amount_stack_item(),
            StackItem::null(),
        ];
        self.call_contract_dynamic(&contract_hash, "transfer", CallFlags::ALL, args)
            .map_err(|error| contract_call_vm_error(error.to_string()))?;

        // Native ABI methods are tiny PUSH0; CallNative; RET scripts. Reuse the
        // real host syscall to preserve Any serialization, null masks, native
        // fees, witness checks, return conversion, notifications, and logs.
        let native_offset = self
            .vm_engine
            .engine()
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No native execution context"))?
            .instruction_pointer();
        self.add_cpu_fee(Self::get_opcode_price(OpCode::PUSH0.byte()))
            .map_err(map_core_error_to_vm_error)?;
        self.vm_engine.engine_mut().push(StackItem::from_int(0))?;
        self.set_direct_instruction_pointer(native_offset + 1)?;
        self.add_cpu_fee(Self::get_opcode_price(OpCode::SYSCALL.byte()))
            .map_err(map_core_error_to_vm_error)?;
        self.attach_host();
        self.vm_engine
            .engine_mut()
            .on_syscall(neo_vm::interop_hash("System.Contract.CallNative"))?;
        self.set_direct_instruction_pointer(native_offset + 6)?;
        self.add_cpu_fee(Self::get_opcode_price(OpCode::RET.byte()))
            .map_err(map_core_error_to_vm_error)?;
        control::ret(self.vm_engine.engine_mut(), &Instruction::ret())
    }

    fn set_direct_instruction_pointer(&mut self, offset: usize) -> VmResult<()> {
        let context = self
            .vm_engine
            .engine_mut()
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current execution context"))?;
        context.set_instruction_pointer(offset);
        Ok(())
    }
}

fn contract_call_vm_error(error: String) -> VmError {
    VmError::InteropService {
        service: "System.Contract.Call".to_string(),
        error,
    }
}
