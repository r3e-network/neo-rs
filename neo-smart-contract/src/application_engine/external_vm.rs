use super::*;
use crate::protocol_projection::{signer_to_stack_value, transaction_to_stack_value};
struct ExternalVmExecution {
    script: Vec<u8>,
    initial_stack: Vec<VmStackValue>,
    initial_ip: usize,
    result_stack_limit: Option<usize>,
    max_instructions: u64,
    instructions_executed: u64,
}
struct ExternalVmHost<'a> {
    engine: &'a mut ApplicationEngine,
    instructions_executed: u64,
    max_instructions: u64,
    runtime_items: HashMap<u64, StackItem>,
}
impl<'a> ExternalVmHost<'a> {
    fn new(engine: &'a mut ApplicationEngine, execution: &ExternalVmExecution) -> Self {
        Self {
            engine,
            instructions_executed: execution.instructions_executed,
            max_instructions: execution.max_instructions,
            runtime_items: HashMap::new(),
        }
    }
}
impl SyscallProvider for ExternalVmHost<'_> {
    fn semantics(&self) -> InterpreterSemantics {
        let limits = self.engine.vm_engine.engine().limits();
        InterpreterSemantics {
            zero_shift_converts_to_integer: limits.zero_shift_converts_to_integer,
            pre_gorgon_collection_semantics: limits.pre_gorgon_collection_semantics,
        }
    }

    fn on_instruction(&mut self, opcode: u8) -> std::result::Result<(), String> {
        if self.instructions_executed >= self.max_instructions {
            return Err(format!(
                "MaxInstructions exceed: {}",
                self.instructions_executed
            ));
        }
        self.instructions_executed = self.instructions_executed.saturating_add(1);
        if let Some(profiler) = self.engine.profiler.as_ref() {
            profiler.record_opcode(opcode);
            profiler.record_external_instruction();
        }
        let opcode_price = ApplicationEngine::get_opcode_price(opcode);
        if opcode_price > 0 {
            self.engine
                .add_cpu_fee(opcode_price)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    fn syscall(
        &mut self,
        api: u32,
        _ip: usize,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        self.charge_syscall_fee(api)?;
        self.handle_runtime_syscall(api, stack)
    }
    fn needs_full_stack(&self) -> bool {
        true
    }
    fn syscall_with_full_stack(
        &mut self,
        api: u32,
        _ip: usize,
        stack: &mut Vec<VmStackValue>,
        _argument_count: usize,
    ) -> std::result::Result<(), String> {
        if external_runtime_syscall(api).is_some() {
            self.charge_syscall_fee(api)?;
            self.handle_runtime_syscall(api, stack)
        } else {
            self.invoke_canonical_syscall(api, stack)
        }
    }
    fn callt(
        &mut self,
        token: u16,
        _ip: usize,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        self.invoke_canonical_callt(token, stack)
    }
}
impl ExternalVmHost<'_> {
    fn charge_syscall_fee(&mut self, api: u32) -> std::result::Result<(), String> {
        let entry = self
            .engine
            .interop_handlers
            .get(&api)
            .copied()
            .ok_or_else(|| format!("unsupported syscall 0x{api:08x}"))?;
        if !self.engine.has_call_flags(entry.required_call_flags) {
            return Err(format!(
                "Missing required call flags: {:?}",
                entry.required_call_flags
            ));
        }
        if entry.price > 0 {
            self.engine
                .add_cpu_fee(entry.price)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    fn handle_runtime_syscall(
        &mut self,
        api: u32,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        match external_runtime_syscall(api) {
            Some(ExternalRuntimeSyscall::Platform) => {
                stack.push(VmStackValue::ByteString(b"NEO".to_vec()));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetTrigger) => {
                stack.push(VmStackValue::Integer(i64::from(
                    self.engine.trigger_type().bits(),
                )));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetNetwork) => {
                stack.push(VmStackValue::Integer(i64::from(
                    self.engine.protocol_settings().network,
                )));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetAddressVersion) => {
                stack.push(VmStackValue::Integer(i64::from(
                    self.engine.protocol_settings().address_version,
                )));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetTime) => {
                let time = self.engine.get_current_block_time()?;
                stack.push(vm_integer_from_u64(time));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetExecutingScriptHash) => {
                stack.push(vm_hash_from_option(self.engine.current_script_hash()));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetCallingScriptHash) => {
                stack.push(vm_hash_from_option(self.engine.get_calling_script_hash()));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetEntryScriptHash) => {
                stack.push(vm_hash_from_option(self.engine.entry_script_hash()));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetInvocationCounter) => {
                let hash = self
                    .engine
                    .current_script_hash()
                    .ok_or_else(|| "No current script".to_string())?;
                let counter = self.engine.get_invocation_counter(&hash);
                stack.push(VmStackValue::Integer(i64::from(counter)));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GasLeft) => {
                stack.push(VmStackValue::Integer(self.engine.gas_left()));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::BurnGas) => {
                let amount = pop_vm_i64(stack, "System.Runtime.BurnGas")?;
                self.engine.runtime_burn_gas(amount)
            }
            Some(ExternalRuntimeSyscall::CheckWitness) => {
                let hash_or_pubkey = pop_vm_bytes(stack, "System.Runtime.CheckWitness")?;
                let result = match hash_or_pubkey.len() {
                    20 => {
                        let hash = UInt160::from_bytes(&hash_or_pubkey)
                            .map_err(|error| error.to_string())?;
                        self.engine
                            .check_witness_hash(&hash)
                            .map_err(|error| error.to_string())?
                    }
                    33 => {
                        let hash = self
                            .engine
                            .pubkey_to_hash(&hash_or_pubkey)
                            .map_err(|error| error.to_string())?;
                        self.engine
                            .check_witness_hash(&hash)
                            .map_err(|error| error.to_string())?
                    }
                    _ => return Err("Invalid hashOrPubkey length".to_string()),
                };
                stack.push(VmStackValue::Boolean(result));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetRandom) => {
                let network = self.engine.protocol_settings().network;
                let aspid_enabled = self.engine.is_hardfork_enabled(Hardfork::HfAspidochelone);
                let buffer = if aspid_enabled {
                    let seed = network.wrapping_add(self.engine.random_counter());
                    self.engine.increment_random_counter();
                    murmur128(self.engine.nonce_bytes(), seed)
                } else {
                    let bytes = murmur128(self.engine.nonce_bytes(), network);
                    self.engine.set_nonce_bytes(bytes);
                    bytes
                };
                let price = if aspid_enabled { 1 << 13 } else { 1 << 4 };
                self.engine
                    .add_cpu_fee(price)
                    .map_err(|error| error.to_string())?;
                let integer = num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &buffer);
                stack.push(vm_integer_from_bigint(integer));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::CurrentSigners) => {
                let Some(container) = self.engine.get_script_container() else {
                    stack.push(VmStackValue::Null);
                    return Ok(());
                };
                let Some(transaction) = container.as_ref().as_any().downcast_ref::<Transaction>()
                else {
                    stack.push(VmStackValue::Null);
                    return Ok(());
                };
                stack.push(VmStackValue::Array(
                    transaction
                        .signers()
                        .iter()
                        .map(signer_to_stack_value)
                        .collect(),
                ));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetScriptContainer) => {
                let container = self
                    .engine
                    .get_script_container()
                    .ok_or_else(|| "No script container".to_string())?;
                let Some(transaction) = container.as_ref().as_any().downcast_ref::<Transaction>()
                else {
                    return Err("Script container does not implement Interoperable".to_string());
                };
                stack.push(
                    transaction_to_stack_value(transaction).map_err(|error| error.to_string())?,
                );
                Ok(())
            }
            Some(ExternalRuntimeSyscall::Log) => {
                let message_bytes = pop_vm_bytes(stack, "System.Runtime.Log")?;
                if message_bytes.len() > MAX_NOTIFICATION_SIZE {
                    return Err(format!(
                        "Notification size {} exceeds maximum allowed size of {} bytes",
                        message_bytes.len(),
                        MAX_NOTIFICATION_SIZE
                    ));
                }
                let message = String::from_utf8(message_bytes).map_err(|_| {
                    "Failed to convert byte array to string: Invalid UTF-8 sequence".to_string()
                })?;
                let container = self
                    .engine
                    .script_container()
                    .ok_or_else(|| "No script container".to_string())?;
                let script_hash = self
                    .engine
                    .current_script_hash()
                    .unwrap_or_else(UInt160::zero);
                let event = LogEventArgs::new(Arc::clone(container), script_hash, message);
                self.engine.emit_log_event(event);
                Ok(())
            }
            None => Err(format!("unsupported syscall 0x{api:08x}")),
        }
    }
}

impl ExternalVmHost<'_> {
    fn invoke_canonical_syscall(
        &mut self,
        api: u32,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        self.replace_canonical_stack(stack)?;
        let target_depth = self.engine.vm_engine.engine().invocation_stack().len();
        let syscall_result = self.engine.vm_engine.engine_mut().on_syscall(api);
        if external_vm_trace_enabled() {
            eprintln!(
                "external syscall=0x{api:08x} target_depth={target_depth} depth={} state={:?} result={syscall_result:?}",
                self.engine.vm_engine.engine().invocation_stack().len(),
                self.engine.vm_engine.engine().state(),
            );
        }
        syscall_result.map_err(|error| error.to_string())?;
        let nested_result = self.execute_nested_to_depth(target_depth);
        if external_vm_trace_enabled() {
            eprintln!(
                "external nested target_depth={target_depth} depth={} state={:?} result={nested_result:?}",
                self.engine.vm_engine.engine().invocation_stack().len(),
                self.engine.vm_engine.engine().state(),
            );
        }
        nested_result?;
        self.export_canonical_stack(stack)
    }

    fn invoke_canonical_callt(
        &mut self,
        token: u16,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        self.replace_canonical_stack(stack)?;
        let target_depth = self.engine.vm_engine.engine().invocation_stack().len();
        self.engine
            .vm_engine
            .engine_mut()
            .invoke_callt(token)
            .map_err(|error| error.to_string())?;
        self.execute_nested_to_depth(target_depth)?;
        self.export_canonical_stack(stack)
    }

    fn execute_nested_to_depth(&mut self, target_depth: usize) -> std::result::Result<(), String> {
        if self.engine.vm_engine.engine().invocation_stack().len() <= target_depth {
            return Ok(());
        }
        match self
            .engine
            .execute_until_invocation_stack_depth(target_depth)
        {
            VMState::FAULT => Err(self
                .engine
                .fault_exception()
                .unwrap_or("nested contract execution faulted")
                .to_string()),
            VMState::HALT
                if self.engine.vm_engine.engine().invocation_stack().len() > target_depth =>
            {
                Err("nested contract execution halted before returning to its caller".to_string())
            }
            _ => Ok(()),
        }
    }

    fn replace_canonical_stack(
        &mut self,
        stack: &[VmStackValue],
    ) -> std::result::Result<(), String> {
        let items = stack
            .iter()
            .cloned()
            .map(|value| self.external_to_canonical(value))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let engine = self.engine.vm_engine.engine_mut();
        let context = engine
            .current_context_mut()
            .ok_or_else(|| "No current execution context for external VM bridge".to_string())?;
        let mut evaluation_stack = context.evaluation_stack_mut();
        evaluation_stack.clear();
        for item in items {
            evaluation_stack
                .push(item)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn export_canonical_stack(
        &mut self,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        let items = {
            let engine = self.engine.vm_engine.engine();
            let context = engine
                .current_context()
                .ok_or_else(|| "No current execution context after host call".to_string())?;
            context.evaluation_stack().to_vec()
        };
        let mut exported = Vec::with_capacity(items.len());
        for item in items {
            exported.push(self.canonical_to_external(item)?);
        }
        *stack = exported;
        Ok(())
    }

    fn external_to_canonical(&self, value: VmStackValue) -> std::result::Result<StackItem, String> {
        match value {
            VmStackValue::Null => Ok(StackItem::Null),
            VmStackValue::Boolean(value) => Ok(StackItem::from_bool(value)),
            VmStackValue::Integer(value) => Ok(StackItem::from_i64(value)),
            VmStackValue::BigInteger(bytes) => Ok(StackItem::from_int(
                num_bigint::BigInt::from_signed_bytes_le(&bytes),
            )),
            VmStackValue::ByteString(bytes) => Ok(StackItem::from_byte_string(bytes)),
            VmStackValue::Buffer(bytes) => Ok(StackItem::from_buffer(bytes)),
            VmStackValue::Array(items) => Ok(StackItem::from_array(
                items
                    .into_iter()
                    .map(|item| self.external_to_canonical(item))
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            )),
            VmStackValue::Struct(items) => Ok(StackItem::from_struct(
                items
                    .into_iter()
                    .map(|item| self.external_to_canonical(item))
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            )),
            VmStackValue::Map(entries) => {
                let mut map = neo_vm::VmOrderedDictionary::with_capacity(entries.len());
                for (key, value) in entries {
                    map.insert(
                        self.external_to_canonical(key)?,
                        self.external_to_canonical(value)?,
                    );
                }
                Ok(StackItem::from_map(map))
            }
            VmStackValue::Pointer(position) => {
                let position = usize::try_from(position)
                    .map_err(|_| format!("negative external VM pointer: {position}"))?;
                let script = self
                    .engine
                    .vm_engine
                    .engine()
                    .current_context()
                    .ok_or_else(|| "No script for external VM pointer".to_string())?
                    .script_arc();
                Ok(StackItem::from_pointer(script, position))
            }
            VmStackValue::Interop(handle) | VmStackValue::Iterator(handle) => self
                .runtime_items
                .get(&handle)
                .cloned()
                .ok_or_else(|| format!("unknown external VM host handle: {handle}")),
        }
    }

    fn canonical_to_external(
        &mut self,
        item: StackItem,
    ) -> std::result::Result<VmStackValue, String> {
        match item {
            StackItem::Null => Ok(VmStackValue::Null),
            StackItem::Boolean(value) => Ok(VmStackValue::Boolean(value)),
            StackItem::Integer(value) => Ok(match value.to_i64() {
                Some(value) => VmStackValue::Integer(value),
                None => VmStackValue::BigInteger(value.to_signed_bytes_le()),
            }),
            StackItem::ByteString(bytes) => Ok(VmStackValue::ByteString(bytes)),
            StackItem::Buffer(buffer) => Ok(VmStackValue::Buffer(buffer.data())),
            StackItem::Array(array) => Ok(VmStackValue::Array(
                array
                    .items()
                    .into_iter()
                    .map(|item| self.canonical_to_external(item))
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            )),
            StackItem::Struct(structure) => Ok(VmStackValue::Struct(
                structure
                    .items()
                    .into_iter()
                    .map(|item| self.canonical_to_external(item))
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            )),
            StackItem::Map(map) => {
                let mut entries = Vec::with_capacity(map.len());
                for (key, value) in map.iter() {
                    entries.push((
                        self.canonical_to_external(key)?,
                        self.canonical_to_external(value)?,
                    ));
                }
                Ok(VmStackValue::Map(entries))
            }
            StackItem::Pointer(pointer) => Ok(VmStackValue::Pointer(
                i64::try_from(pointer.position())
                    .map_err(|_| "canonical pointer does not fit external VM i64".to_string())?,
            )),
            StackItem::InteropInterface(interface) => {
                let handle = Arc::as_ptr(&interface).cast::<()>() as usize as u64;
                self.runtime_items
                    .insert(handle, StackItem::InteropInterface(interface));
                Ok(VmStackValue::Interop(handle))
            }
        }
    }
}

impl ApplicationEngine {
    pub(super) fn try_execute_with_external_vm(&mut self) -> Option<VMState> {
        let execution = self.prepare_external_vm_execution()?;
        let mut host = ExternalVmHost::new(self, &execution);
        let result = if let Some(result_stack_limit) = execution.result_stack_limit {
            interpret_with_stack_and_syscalls_at_with_result_limit(
                &execution.script,
                execution.initial_stack,
                execution.initial_ip,
                result_stack_limit,
                &mut host,
            )
        } else {
            interpret_with_stack_and_syscalls_at(
                &execution.script,
                execution.initial_stack,
                execution.initial_ip,
                &mut host,
            )
        };
        let instructions_executed = host.instructions_executed;
        match result {
            Ok(result) => {
                if result.state == VMState::HALT
                    && let Err(message) = host.replace_canonical_stack(&result.stack)
                {
                    return Some(
                        host.engine
                            .apply_external_vm_fault(message, instructions_executed),
                    );
                }
                Some(
                    host.engine
                        .apply_external_vm_result(result, instructions_executed),
                )
            }
            Err(message) => Some(
                host.engine
                    .apply_external_vm_fault(message, instructions_executed),
            ),
        }
    }
    fn prepare_external_vm_execution(&self) -> Option<ExternalVmExecution> {
        // The bridge remains an explicit experiment until it supports method slots
        // and beats canonical execution in no-profiler A/B replay. Keep the disable
        // switch as well so one experimental binary can run alternating comparisons.
        static ENABLE_EXTERNAL_VM: OnceLock<bool> = OnceLock::new();
        static DISABLE_EXTERNAL_VM: OnceLock<bool> = OnceLock::new();
        let enabled = *ENABLE_EXTERNAL_VM
            .get_or_init(|| crate::env_flags::env_flag_enabled("NEO_RS_ENABLE_EXTERNAL_VM", false));
        if !enabled
            || *DISABLE_EXTERNAL_VM
                .get_or_init(|| std::env::var_os("NEO_RS_DISABLE_EXTERNAL_VM").is_some())
        {
            return None;
        }
        // Diagnostics must not change consensus semantics by forcing the jump-table path.
        if !self.pending_native_calls.is_empty() {
            return None;
        }
        let engine = self.vm_engine.engine();
        if matches!(engine.state(), VMState::HALT | VMState::FAULT) {
            return None;
        }
        let context = engine.current_context()?;
        if context.local_variables().is_some()
            || context.arguments().is_some()
            || context.try_stack().is_some()
            || context.has_static_fields()
        {
            return None;
        }
        let invocation_stack = engine.invocation_stack();
        let shares_caller_stack = invocation_stack
            .len()
            .checked_sub(2)
            .and_then(|caller_index| invocation_stack.get(caller_index))
            .is_some_and(|caller| context.shares_evaluation_stack_with(caller));
        let script = context.script().as_bytes();
        if script_uses_application_engine_host(script) {
            return None;
        }
        // Host-only objects cannot be manufactured without their runtime
        // identity. Such a context remains on the canonical engine; objects
        // returned later by syscalls use the bridge's handle table instead.
        let initial_stack = context
            .evaluation_stack()
            .iter()
            .cloned()
            .map(VmStackValue::try_from)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        Some(ExternalVmExecution {
            script: script.to_vec(),
            initial_stack,
            initial_ip: context.instruction_pointer(),
            result_stack_limit: external_result_stack_limit(context.rvcount(), shares_caller_stack),
            max_instructions: engine.limits().max_instructions,
            instructions_executed: engine.instructions_executed,
        })
    }
    fn apply_external_vm_result(
        &mut self,
        result: neo_vm::ExecutionResult,
        instructions_executed: u64,
    ) -> VMState {
        self.vm_engine.engine_mut().instructions_executed = instructions_executed;
        match result.state {
            VMState::HALT => self.apply_external_vm_halt(instructions_executed),
            VMState::FAULT => {
                let message = external_vm_fault_message(result.fault_message, result.fault_ip);
                self.apply_external_vm_fault(message, instructions_executed)
            }
            other => {
                self.vm_engine.engine_mut().set_state(other);
                other
            }
        }
    }
    fn apply_external_vm_halt(&mut self, instructions_executed: u64) -> VMState {
        // Reuse the canonical RET path after synchronizing the evaluation stack.
        // This preserves rvcount checks, return propagation, dynamic-call null
        // normalization, child snapshot commits, and notification rollback.
        let result = neo_vm::jump_table::control::ret(
            self.vm_engine.engine_mut(),
            &neo_vm::Instruction::ret(),
        );
        match result {
            Ok(()) => self.vm_engine.engine().state(),
            Err(error) => self.apply_external_vm_fault(error.to_string(), instructions_executed),
        }
    }
    fn apply_external_vm_fault(&mut self, message: String, instructions_executed: u64) -> VMState {
        if instructions_executed > 0 {
            self.vm_engine.engine_mut().instructions_executed = instructions_executed;
        }
        self.vm_engine
            .engine_mut()
            .set_uncaught_exception(Some(StackItem::from_byte_string(
                message.clone().into_bytes(),
            )));
        self.vm_engine.engine_mut().set_state(VMState::FAULT);
        self.fault_exception = Some(message);
        VMState::FAULT
    }
}
fn script_uses_application_engine_host(script: &[u8]) -> bool {
    let Ok(instructions) = neo_vm::parse_script_instructions(script) else {
        return true;
    };
    let invocation_stack = neo_vm::interop_hash("System.Runtime.GetInvocationStack");
    for instruction in instructions {
        // Static fields are canonical contract state. Until their object identity
        // can be preserved across the ABI bridge, both initialization and every
        // initialized static slot must stay on the canonical engine.
        if instruction.opcode() == OpCode::INITSSLOT {
            return true;
        }
        // The fast interpreter deliberately owns caller slots and TRY frames.
        // Projecting the complete invocation stack would expose the incomplete
        // canonical mirror, so this one introspection syscall stays canonical.
        if instruction.opcode() == OpCode::SYSCALL && instruction.token_u32() == invocation_stack {
            return true;
        }
    }
    false
}
#[derive(Clone, Copy)]
enum ExternalRuntimeSyscall {
    Platform,
    GetTrigger,
    GetNetwork,
    GetAddressVersion,
    GetTime,
    GetExecutingScriptHash,
    GetCallingScriptHash,
    GetEntryScriptHash,
    GetInvocationCounter,
    GasLeft,
    BurnGas,
    CheckWitness,
    GetRandom,
    CurrentSigners,
    GetScriptContainer,
    Log,
}
fn external_runtime_syscall(api: u32) -> Option<ExternalRuntimeSyscall> {
    if api == neo_vm::interop_hash("System.Runtime.Platform") {
        Some(ExternalRuntimeSyscall::Platform)
    } else if api == neo_vm::interop_hash("System.Runtime.GetTrigger") {
        Some(ExternalRuntimeSyscall::GetTrigger)
    } else if api == neo_vm::interop_hash("System.Runtime.GetNetwork") {
        Some(ExternalRuntimeSyscall::GetNetwork)
    } else if api == neo_vm::interop_hash("System.Runtime.GetAddressVersion") {
        Some(ExternalRuntimeSyscall::GetAddressVersion)
    } else if api == neo_vm::interop_hash("System.Runtime.GetTime") {
        Some(ExternalRuntimeSyscall::GetTime)
    } else if api == neo_vm::interop_hash("System.Runtime.GetExecutingScriptHash") {
        Some(ExternalRuntimeSyscall::GetExecutingScriptHash)
    } else if api == neo_vm::interop_hash("System.Runtime.GetCallingScriptHash") {
        Some(ExternalRuntimeSyscall::GetCallingScriptHash)
    } else if api == neo_vm::interop_hash("System.Runtime.GetEntryScriptHash") {
        Some(ExternalRuntimeSyscall::GetEntryScriptHash)
    } else if api == neo_vm::interop_hash("System.Runtime.GetInvocationCounter") {
        Some(ExternalRuntimeSyscall::GetInvocationCounter)
    } else if api == neo_vm::interop_hash("System.Runtime.GasLeft") {
        Some(ExternalRuntimeSyscall::GasLeft)
    } else if api == neo_vm::interop_hash("System.Runtime.BurnGas") {
        Some(ExternalRuntimeSyscall::BurnGas)
    } else if api == neo_vm::interop_hash("System.Runtime.CheckWitness") {
        Some(ExternalRuntimeSyscall::CheckWitness)
    } else if api == neo_vm::interop_hash("System.Runtime.GetRandom") {
        Some(ExternalRuntimeSyscall::GetRandom)
    } else if api == neo_vm::interop_hash("System.Runtime.CurrentSigners") {
        Some(ExternalRuntimeSyscall::CurrentSigners)
    } else if api == neo_vm::interop_hash("System.Runtime.GetScriptContainer") {
        Some(ExternalRuntimeSyscall::GetScriptContainer)
    } else if api == neo_vm::interop_hash("System.Runtime.Log") {
        Some(ExternalRuntimeSyscall::Log)
    } else {
        None
    }
}
fn vm_integer_from_u64(value: u64) -> VmStackValue {
    if value <= i64::MAX as u64 {
        VmStackValue::Integer(value as i64)
    } else {
        VmStackValue::BigInteger(num_bigint::BigInt::from(value).to_signed_bytes_le())
    }
}
fn vm_integer_from_bigint(value: num_bigint::BigInt) -> VmStackValue {
    match value.to_i64() {
        Some(value) => VmStackValue::Integer(value),
        None => VmStackValue::BigInteger(value.to_signed_bytes_le()),
    }
}
fn vm_hash_from_option(hash: Option<UInt160>) -> VmStackValue {
    match hash {
        Some(hash) => VmStackValue::ByteString(hash.to_bytes()),
        None => VmStackValue::Null,
    }
}
fn pop_vm_i64(stack: &mut Vec<VmStackValue>, syscall: &str) -> std::result::Result<i64, String> {
    let value = stack
        .pop()
        .ok_or_else(|| format!("{syscall} expects an argument"))?;
    neo_vm::stack_value_as_i64(&value).ok_or_else(|| {
        format!("{syscall} expects an Integer argument that fits into i64, got {value:?}")
    })
}
fn pop_vm_bytes(
    stack: &mut Vec<VmStackValue>,
    syscall: &str,
) -> std::result::Result<Vec<u8>, String> {
    let value = stack
        .pop()
        .ok_or_else(|| format!("{syscall} expects an argument"))?;
    value
        .to_byte_string_bytes()
        .ok_or_else(|| format!("{syscall} expects a byte-convertible argument, got {value:?}"))
}
fn external_result_stack_limit(rvcount: i32, shares_caller_stack: bool) -> Option<usize> {
    // Canonical RET ignores rvcount when a cloned context shares its caller's
    // evaluation stack. Applying the limit inside the fast interpreter would
    // discard method arguments before the shared-stack RET can run.
    (rvcount >= 0 && !shares_caller_stack).then_some(rvcount as usize)
}

fn external_vm_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| crate::env_flags::env_flag_enabled("NEO_TRACE_EXTERNAL_VM", false))
}

fn external_vm_fault_message(message: Option<String>, ip: Option<u32>) -> String {
    let message = message.unwrap_or_else(|| "VM execution faulted".to_string());
    match ip {
        Some(ip) => format!("{message} [ip={ip}]"),
        None => message,
    }
}

#[cfg(test)]
mod tests {
    use super::{external_result_stack_limit, script_uses_application_engine_host};
    use neo_vm::OpCode;

    #[test]
    fn static_slot_initialization_stays_on_canonical_engine() {
        assert!(script_uses_application_engine_host(&[
            OpCode::INITSSLOT as u8,
            1,
            OpCode::RET as u8,
        ]));
        assert!(!script_uses_application_engine_host(&[OpCode::RET as u8]));
    }

    #[test]
    fn shared_caller_stack_never_truncates_initializer_arguments() {
        assert_eq!(external_result_stack_limit(0, true), None);
        assert_eq!(external_result_stack_limit(1, true), None);
        assert_eq!(external_result_stack_limit(-1, true), None);
    }

    #[test]
    fn independent_stack_retains_declared_return_limit() {
        assert_eq!(external_result_stack_limit(0, false), Some(0));
        assert_eq!(external_result_stack_limit(1, false), Some(1));
        assert_eq!(external_result_stack_limit(-1, false), None);
    }
}
