use super::*;

struct ExternalVmExecution {
    script: Vec<u8>,
    initial_stack: Vec<VmStackValue>,
    initial_ip: usize,
    rvcount: i32,
    max_instructions: u64,
    instructions_executed: u64,
}

struct ExternalVmHost<'a> {
    engine: &'a mut ApplicationEngine,
    instructions_executed: u64,
    max_instructions: u64,
}

impl<'a> ExternalVmHost<'a> {
    fn new(engine: &'a mut ApplicationEngine, execution: &ExternalVmExecution) -> Self {
        Self {
            engine,
            instructions_executed: execution.instructions_executed,
            max_instructions: execution.max_instructions,
        }
    }
}

impl SyscallProvider for ExternalVmHost<'_> {
    fn on_instruction(&mut self, opcode: u8) -> std::result::Result<(), String> {
        if self.instructions_executed >= self.max_instructions {
            return Err(format!(
                "MaxInstructions exceed: {}",
                self.instructions_executed
            ));
        }
        self.instructions_executed = self.instructions_executed.saturating_add(1);

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
                let time = self
                    .engine
                    .get_current_block_time()
                    .map_err(|e| e.to_string())?;
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
                let counter = self.engine.get_or_init_invocation_counter(&hash);
                stack.push(VmStackValue::Integer(i64::from(counter)));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GasLeft) => {
                stack.push(VmStackValue::Integer(self.engine.gas_left()));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::BurnGas) => {
                let amount = pop_vm_i64(stack, "System.Runtime.BurnGas")?;
                self.engine
                    .runtime_burn_gas(amount)
                    .map_err(|e| e.to_string())
            }
            Some(ExternalRuntimeSyscall::CheckWitness) => {
                let hash_or_pubkey = pop_vm_bytes(stack, "System.Runtime.CheckWitness")?;
                let result = match hash_or_pubkey.len() {
                    20 => {
                        let hash = UInt160::from_bytes(&hash_or_pubkey)
                            .map_err(|error| error.to_string())?;
                        self.engine
                            .check_witness_hash(&hash)
                            .map_err(|e| e.to_string())?
                    }
                    33 => {
                        let hash = self
                            .engine
                            .pubkey_to_hash(&hash_or_pubkey)
                            .map_err(|e| e.to_string())?;
                        self.engine
                            .check_witness_hash(&hash)
                            .map_err(|e| e.to_string())?
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
                    Murmur3::murmur128(self.engine.nonce_bytes(), seed)
                } else {
                    let bytes = Murmur3::murmur128(self.engine.nonce_bytes(), network);
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
                let Some(transaction) = container
                    .as_ref()
                    .as_any()
                    .downcast_ref::<neo_payloads::Transaction>()
                else {
                    stack.push(VmStackValue::Null);
                    return Ok(());
                };

                stack.push(VmStackValue::Array(
                    0,
                    transaction
                        .signers()
                        .iter()
                        .map(|signer| signer.to_stack_value())
                        .collect(),
                ));
                Ok(())
            }
            Some(ExternalRuntimeSyscall::GetScriptContainer) => {
                let container = self
                    .engine
                    .get_script_container()
                    .ok_or_else(|| "No script container".to_string())?;

                let Some(transaction) = container
                    .as_ref()
                    .as_any()
                    .downcast_ref::<neo_payloads::Transaction>()
                else {
                    return Err("Script container does not implement Interoperable".to_string());
                };

                stack.push(
                    transaction
                        .to_stack_value()
                        .map_err(|error| error.to_string())?,
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
                let script_hash = self
                    .engine
                    .current_script_hash()
                    .unwrap_or_else(UInt160::zero);
                let event = LogEventArgs::new(
                    self.engine.script_container().cloned(),
                    script_hash,
                    message,
                );
                self.engine.emit_log_event(event);
                Ok(())
            }
            None => Err(format!("unsupported syscall 0x{api:08x}")),
        }
    }
}

impl ApplicationEngine {
    pub(super) fn try_execute_with_external_vm(&mut self) -> Option<VMState> {
        let execution = self.prepare_external_vm_execution()?;
        let (result, instructions_executed) = {
            let mut host = ExternalVmHost::new(self, &execution);
            let result = if execution.rvcount >= 0 {
                interpret_with_stack_and_syscalls_at_with_result_limit(
                    &execution.script,
                    execution.initial_stack,
                    execution.initial_ip,
                    execution.rvcount as usize,
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
            (result, host.instructions_executed)
        };

        match result {
            Ok(result) => Some(self.apply_external_vm_result(result, instructions_executed)),
            Err(message) => Some(self.apply_external_vm_fault(message, instructions_executed)),
        }
    }

    fn prepare_external_vm_execution(&self) -> Option<ExternalVmExecution> {
        if self.diagnostic.is_some() || !self.pending_native_calls.is_empty() {
            return None;
        }

        let engine = self.vm_engine.engine();
        if matches!(engine.state(), VMState::HALT | VMState::FAULT) {
            return None;
        }
        if engine.invocation_stack().len() != 1 {
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
        if !context.evaluation_stack().is_empty() {
            return None;
        }

        let script = context.script().as_bytes();
        if script_uses_application_engine_host(script) {
            return None;
        }

        // The eval stack is guaranteed empty by the guard above; convert
        // fallibly anyway and decline the external VM (fall back to the local
        // engine) rather than panic if a value cannot be represented.
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
            rvcount: context.rvcount(),
            max_instructions: engine.limits().max_instructions,
            instructions_executed: engine.instructions_executed,
        })
    }

    fn apply_external_vm_result(
        &mut self,
        result: neo_vm_rs::ExecutionResult,
        instructions_executed: u64,
    ) -> VMState {
        self.vm_engine.engine_mut().instructions_executed = instructions_executed;

        match result.state {
            VMState::HALT => self.apply_external_vm_halt(result.stack),
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

    fn apply_external_vm_halt(&mut self, stack: Vec<VmStackValue>) -> VMState {
        let mut stack_items: Vec<StackItem> = Vec::with_capacity(stack.len());
        for v in stack {
            match StackItem::try_from(v) {
                Ok(item) => stack_items.push(item),
                Err(error) => {
                    return self.apply_external_vm_fault(
                        format!("Failed to convert external VM stack item: {error}"),
                        0,
                    );
                }
            }
        }

        let context_index = match self
            .vm_engine
            .engine()
            .invocation_stack()
            .len()
            .checked_sub(1)
        {
            Some(index) => index,
            None => {
                return self.apply_external_vm_fault(
                    "No execution context after external VM halt".to_string(),
                    0,
                );
            }
        };

        if let Err(error) = self.vm_engine.engine_mut().remove_context(context_index) {
            return self.apply_external_vm_fault(error.to_string(), 0);
        }

        let result_stack = self.vm_engine.engine_mut().result_stack_mut();
        for item in stack_items {
            if let Err(error) = result_stack.push(item) {
                return self.apply_external_vm_fault(error.to_string(), 0);
            }
        }

        VMState::HALT
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
    let instructions = match neo_vm_rs::parse_script_instructions(script) {
        Ok(instructions) => instructions,
        Err(_) => return true,
    };

    for instruction in instructions {
        match instruction.opcode() {
            OpCode::SYSCALL if !is_external_vm_supported_syscall(instruction.token_u32()) => {
                return true;
            }
            OpCode::CALLT => return true,
            _ => {}
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
    if api == neo_vm_rs::interop_hash("System.Runtime.Platform") {
        Some(ExternalRuntimeSyscall::Platform)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetTrigger") {
        Some(ExternalRuntimeSyscall::GetTrigger)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetNetwork") {
        Some(ExternalRuntimeSyscall::GetNetwork)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetAddressVersion") {
        Some(ExternalRuntimeSyscall::GetAddressVersion)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetTime") {
        Some(ExternalRuntimeSyscall::GetTime)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetExecutingScriptHash") {
        Some(ExternalRuntimeSyscall::GetExecutingScriptHash)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetCallingScriptHash") {
        Some(ExternalRuntimeSyscall::GetCallingScriptHash)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetEntryScriptHash") {
        Some(ExternalRuntimeSyscall::GetEntryScriptHash)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetInvocationCounter") {
        Some(ExternalRuntimeSyscall::GetInvocationCounter)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GasLeft") {
        Some(ExternalRuntimeSyscall::GasLeft)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.BurnGas") {
        Some(ExternalRuntimeSyscall::BurnGas)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.CheckWitness") {
        Some(ExternalRuntimeSyscall::CheckWitness)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetRandom") {
        Some(ExternalRuntimeSyscall::GetRandom)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.CurrentSigners") {
        Some(ExternalRuntimeSyscall::CurrentSigners)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.GetScriptContainer") {
        Some(ExternalRuntimeSyscall::GetScriptContainer)
    } else if api == neo_vm_rs::interop_hash("System.Runtime.Log") {
        Some(ExternalRuntimeSyscall::Log)
    } else {
        None
    }
}

fn is_external_vm_supported_syscall(api: u32) -> bool {
    external_runtime_syscall(api).is_some()
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
    neo_vm_rs::stack_value_as_i64(&value).ok_or_else(|| {
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

fn external_vm_fault_message(message: Option<String>, ip: Option<u32>) -> String {
    let message = message.unwrap_or_else(|| "VM execution faulted".to_string());
    match ip {
        Some(ip) => format!("{message} [ip={ip}]"),
        None => message,
    }
}
