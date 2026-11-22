//! Direct dispatch for strictly parsed native NEP-17 transfers.
//!
//! The entry stack-building instructions are not interpreted, but their fees
//! are charged in order. Native ABI entry, argument serialization, native
//! return conversion, context unload, ASSERT, and snapshot commit reuse the
//! canonical handlers so direct dispatch does not become a second state model.
//!
//! Instruction accounting is the one resource dimension the interpreter loop
//! would otherwise own exclusively. Because the entry stack is never
//! materialized here, every step below charges the same `max_instructions`
//! budget and the same counter `ExecutionEngine::execute_next_internal` charges.

use super::*;
use crate::error::CoreResult;
use neo_vm::jump_table::control;
use neo_vm::run_stats::RunStats;

const SYSTEM_CONTRACT_CALL_PRICE: i64 = 1 << 15;
/// Instructions the canonical engine executes for a native ABI method entry:
/// `PUSH0`, `System.Contract.CallNative`, `RET`. See `build_native_contract_state`.
const NATIVE_METHOD_ENTRY_INSTRUCTIONS: u64 = 3;
/// The NEP-17 template packs exactly four arguments (`from`, `to`, `amount`,
/// `data`); the parser rejects every other item count, so the canonical `PACK`
/// price can be reproduced from this constant.
const TEMPLATE_PACKED_ITEMS: i32 = 4;

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
        let Ok(sender) = UInt160::from_bytes(&params.from) else {
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
        // Both transfer participants must be plain accounts. A contract
        // recipient can queue onNEP17Payment, and a contract sender can be
        // re-entered by the GAS reward callbacks the native method triggers
        // (`GasToken::mint` from a NEO transfer). Either would move execution
        // back into the canonical interpreter mid-dispatch, so both decline.
        //
        // A failed state read proves nothing: only a confirmed plain-account
        // participant keeps acceleration eligible.
        for participant in [&sender, &recipient] {
            if !matches!(
                self.direct_transfer_contract_participant(participant),
                Ok(false)
            ) {
                return false;
            }
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
        // Canonical execution charges one instruction per entry step plus the
        // native method entry. When the remaining budget cannot cover them the
        // direct path would report a different instruction count, so the
        // canonical interpreter stays responsible for the outcome.
        let required = params.entry_instructions.len() as u64 + NATIVE_METHOD_ENTRY_INSTRUCTIONS;
        if self.direct_transfer_instruction_budget() < required {
            return false;
        }
        true
    }

    /// Returns whether `hash` takes part in a transfer as a contract.
    ///
    /// Deployed, native, and already-loaded contracts all count. The snapshot
    /// read is fallible, and an error cannot be read as "plain account", so
    /// callers treat `Err` as ineligible.
    fn direct_transfer_contract_participant(&self, hash: &UInt160) -> CoreResult<bool> {
        if self.contracts.contains_key(hash) || self.native_registry.get(hash).is_some() {
            return Ok(true);
        }
        Ok(self
            .native_state_reader
            .contract(self.snapshot_cache.as_ref(), hash)?
            .is_some())
    }

    /// Returns how many further instructions this engine may execute.
    fn direct_transfer_instruction_budget(&self) -> u64 {
        let engine = self.vm_engine.engine();
        engine
            .limits()
            .max_instructions
            .saturating_sub(engine.instructions_executed)
    }

    /// Charges `count` instructions exactly like the canonical instruction loop.
    ///
    /// `ExecutionEngine::execute_next_internal` is the only place canonical
    /// execution checks `max_instructions` and advances `instructions_executed`,
    /// and the direct path never reaches it. Charging here keeps an engine with
    /// a service-level instruction budget (RPC admission, probes) observing the
    /// same work under both strategies, including the same fault.
    fn charge_direct_instructions(&mut self, count: u64) -> VmResult<()> {
        let engine = self.vm_engine.engine_mut();
        let max_instructions = engine.limits().max_instructions;
        for _ in 0..count {
            if engine.instructions_executed >= max_instructions {
                return Err(VmError::instruction_limit_exceeded(
                    engine.instructions_executed,
                    max_instructions,
                ));
            }
            engine.instructions_executed += 1;
        }
        Ok(())
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

    /// Charges one instruction exactly like the canonical hooks do.
    ///
    /// Before `HF_Huyao` the v3.10 opcode table applies up front
    /// (`pre_execute_instruction`); since `HF_Huyao` the `OpcodeV1` coefficient
    /// is charged from the `RunStats` the interpreter would have collected
    /// (`post_execute_instruction`, which neo-vm runs even when the
    /// instruction faulted). `stats` must therefore carry the values the
    /// canonical handler records — only `PACK` and `ASSERT` need non-default
    /// stats in the transfer template.
    fn charge_direct_instruction(&mut self, opcode: OpCode, stats: &RunStats) -> VmResult<()> {
        self.capture_whitelisted();
        if self.use_dynamic_pricing {
            let price = self.opcode_v1(i64::from(self.exec_fee_factor), opcode.byte(), stats);
            self.add_femto_gas(price, true)
                .map_err(map_core_error_to_vm_error)
        } else {
            let price = Self::get_opcode_price(opcode.byte());
            if price > 0 {
                self.add_cpu_fee(price).map_err(map_core_error_to_vm_error)
            } else {
                Ok(())
            }
        }
    }

    fn execute_direct_nep17_transfer_inner(&mut self, params: &TransferParams) -> VmResult<()> {
        for instruction in &params.entry_instructions {
            self.set_direct_instruction_pointer(instruction.offset)?;
            self.charge_direct_instructions(1)?;
            match instruction.opcode {
                OpCode::SYSCALL => {
                    self.charge_direct_instruction(OpCode::SYSCALL, &RunStats::new())?;
                    self.direct_native_contract_call(params)?
                }
                OpCode::ASSERT => {
                    // The canonical ASSERT records the pop's reference delta on
                    // the success path only, and the post hook still charges
                    // the instruction when the assert faults (neo-vm runs it
                    // in a `finally`), so the charge precedes the fault here.
                    let before = self.vm_engine.engine().reference_counter().signed_count() as i64;
                    let assertion = self.vm_engine.engine_mut().pop()?;
                    let succeeded = assertion.as_boolean()?;
                    let mut stats = RunStats::new();
                    if succeeded {
                        stats.collect_ref_delta(
                            (before
                                - self.vm_engine.engine().reference_counter().signed_count() as i64)
                                as i32,
                        );
                    }
                    self.charge_direct_instruction(OpCode::ASSERT, &stats)?;
                    if !succeeded {
                        return Err(VmError::AssertFailed);
                    }
                }
                opcode => {
                    let mut stats = RunStats::new();
                    if opcode == OpCode::PACK {
                        stats.collect_op_length(TEMPLATE_PACKED_ITEMS);
                    }
                    self.charge_direct_instruction(opcode, &stats)?;
                }
            }
        }
        self.set_direct_instruction_pointer(params.entry_end_offset)?;
        // The entry script ends with an implicit RET: no RET opcode fee, but
        // canonical result-stack transfer and snapshot commit are required.
        control::ret(self.vm_engine.engine_mut(), &Instruction::ret())
    }

    fn direct_native_contract_call(&mut self, params: &TransferParams) -> VmResult<()> {
        // C# `System.Contract.Call` price: the `InteropPriceAttribute`
        // coefficient since `HF_Huyao`, the static descriptor price before it
        // (mirrors `invoke_syscall`).
        if self.is_hardfork_enabled(Hardfork::HfHuyao) {
            let femto_gas = op_code_prices_v1::CONTRACT_CALL_PRICE
                .checked_mul(i64::from(self.exec_fee_factor))
                .ok_or_else(|| VmError::invalid_operation_msg("Syscall price overflow"))?;
            self.add_femto_gas(femto_gas, true)
                .map_err(map_core_error_to_vm_error)?;
        } else {
            self.add_cpu_fee(SYSTEM_CONTRACT_CALL_PRICE)
                .map_err(map_core_error_to_vm_error)?;
        }
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
        self.charge_direct_instructions(1)?;
        self.charge_direct_instruction(OpCode::PUSH0, &RunStats::new())?;
        self.vm_engine.engine_mut().push(StackItem::from_int(0))?;
        self.set_direct_instruction_pointer(native_offset + 1)?;
        self.charge_direct_instructions(1)?;
        self.charge_direct_instruction(OpCode::SYSCALL, &RunStats::new())?;
        self.attach_host();
        self.vm_engine
            .engine_mut()
            .on_syscall(neo_vm::interop_hash("System.Contract.CallNative"))?;
        self.set_direct_instruction_pointer(native_offset + 6)?;
        self.charge_direct_instructions(1)?;
        self.charge_direct_instruction(OpCode::RET, &RunStats::new())?;
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
