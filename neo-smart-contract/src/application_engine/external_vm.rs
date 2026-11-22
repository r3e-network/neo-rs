use super::*;
use crate::protocol_projection::{signer_to_stack_value, transaction_to_stack_value};
use neo_vm::stack_item::vm_integer::vm_integer_from_bigint;

mod engine;
mod host;

#[cfg(test)]
mod tests;

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
                            .map_err(|error| error.clone())?;
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
                        "Notification size {} exceeds maximum allowed size of {MAX_NOTIFICATION_SIZE} bytes",
                        message_bytes.len()
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

/// Moves the last `len` values out, so a compound can take its children.
fn take_tail<T>(built: &mut Vec<T>, len: usize) -> Vec<T> {
    built.split_off(built.len() - len)
}

impl ApplicationEngine {
    pub(super) fn try_execute_with_external_vm(&mut self) -> Option<VMState> {
        let execution = self.prepare_external_vm_execution()?;
        let _depth = ExternalVmDepthGuard::enter();
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
}
/// Whether `item` nests more than `limit` compounds deep. The walk stops
/// after `limit + 1` levels, so it also ends on a value that contains itself.
///
/// Children are held on an explicit work list rather than the native stack:
/// `limit` is the canonical engine's own nesting bound, which a legal value can
/// reach, and one native frame per level would not fit on a thread stack.
fn canonical_nesting_exceeds(item: &StackItem, limit: usize) -> bool {
    let mut pending: Vec<(StackItem, usize)> = vec![(item.clone(), limit)];
    while let Some((item, remaining)) = pending.pop() {
        let Some(room) = remaining.checked_sub(1) else {
            return matches!(
                item,
                StackItem::Array(_) | StackItem::Struct(_) | StackItem::Map(_)
            );
        };
        match item {
            StackItem::Array(array) => {
                pending.extend(array.items().into_iter().map(|child| (child, room)));
            }
            StackItem::Struct(structure) => {
                pending.extend(structure.items().into_iter().map(|child| (child, room)));
            }
            StackItem::Map(map) => {
                pending.extend(map.iter().map(|(_, value)| (value, room)));
            }
            _ => {}
        }
    }
    false
}
/// A contract called from the interpreter runs inside the caller's host call,
/// so every interpreter execution in a call chain stays on the native stack
/// until the chain unwinds. Contexts past this many run on the canonical
/// engine, whose loop keeps the native stack flat up to its own invocation
/// limit. Four of them, the innermost receiving a value nested to the
/// interpreter's limit, fit in 512 KiB without optimization.
const MAX_EXTERNAL_VM_DEPTH: usize = 4;
thread_local! {
    static EXTERNAL_VM_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn external_vm_depth_exhausted() -> bool {
    EXTERNAL_VM_DEPTH.with(|depth| depth.get() >= MAX_EXTERNAL_VM_DEPTH)
}
struct ExternalVmDepthGuard;
impl ExternalVmDepthGuard {
    fn enter() -> Self {
        EXTERNAL_VM_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}
impl Drop for ExternalVmDepthGuard {
    fn drop(&mut self) {
        EXTERNAL_VM_DEPTH.with(|depth| depth.set(depth.get() - 1));
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
