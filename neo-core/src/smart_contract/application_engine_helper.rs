//! ApplicationEngine.Helper - matches C# Neo.SmartContract.ApplicationEngine helper methods exactly

use crate::cryptography::crypto_utils::NeoHash;
use crate::hardfork::Hardfork;
use crate::smart_contract::application_engine::{
    ApplicationEngine, MAX_NOTIFICATION_COUNT, MAX_NOTIFICATION_SIZE,
};
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::notify_event_args::NotifyEventArgs;
use crate::smart_contract::trigger_type::TriggerType;
use crate::UInt160;
use neo_vm::stack_item::{Array, Map, Struct};
use neo_vm::{OrderedDictionary, StackItem, VMState};
use num_traits::ToPrimitive;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

impl ApplicationEngine {
    /// Provides detailed stack information when the engine faults.
    pub fn get_engine_stack_info_on_fault(
        &self,
        exception_stack_trace: bool,
        exception_message: bool,
    ) -> String {
        if self.state() != VMState::FAULT {
            return String::new();
        }

        if self.fault_exception_string().is_none() {
            return String::new();
        }

        let mut output = String::new();

        if let Some(calling_hash) = self.get_calling_script_hash() {
            let _ = writeln!(
                output,
                "CallingScriptHash={}{}",
                calling_hash,
                self.contract_display_name(&calling_hash)
                    .map(|name| format!("[{name}]"))
                    .unwrap_or_default()
            );
        }

        if let Some(current_hash) = self.current_script_hash() {
            let _ = writeln!(
                output,
                "CurrentScriptHash={}{}",
                current_hash,
                self.contract_display_name(&current_hash)
                    .map(|name| format!("[{name}]"))
                    .unwrap_or_default()
            );
        }

        if let Some(entry_hash) = self.entry_script_hash() {
            let _ = writeln!(output, "EntryScriptHash={entry_hash}");
        }

        for context in self.invocation_stack().iter().rev() {
            let script_len = context.script().len();
            let ip = context.instruction_pointer();
            let opcode = context
                .current_instruction()
                .map(|instruction| format!("{:?}", instruction.opcode))
                .unwrap_or_else(|_| "<none>".to_string());

            let script_hash =
                UInt160::from_bytes(&context.script_hash()).unwrap_or_else(|_| UInt160::zero());
            let contract_suffix = self
                .contract_display_name(&script_hash)
                .map(|name| format!("[{name}]"))
                .unwrap_or_default();

            let _ = writeln!(
                output,
                "\tInstructionPointer={ip}, OpCode {opcode}, Script Length={script_len} {script_hash}{contract_suffix}"
            );
        }

        output.push_str(&self.get_engine_exception_info(exception_stack_trace, exception_message));
        output
    }

    /// Provides formatted exception details matching the C# helper.
    pub fn get_engine_exception_info(
        &self,
        _exception_stack_trace: bool,
        exception_message: bool,
    ) -> String {
        if self.state() != VMState::FAULT {
            return String::new();
        }

        let Some(message) = self.fault_exception_string() else {
            return String::new();
        };

        let mut output = String::new();

        if exception_message && !message.is_empty() {
            let _ = writeln!(output, "{message}");
        }

        output
    }

    /// Helper to push a boolean to the stack
    pub fn push_boolean(&mut self, value: bool) -> Result<(), String> {
        self.push(StackItem::from_bool(value))
    }

    /// Helper to push an integer to the stack
    pub fn push_integer(&mut self, value: i64) -> Result<(), String> {
        self.push(StackItem::from_int(value))
    }

    /// Helper to push bytes to the stack
    pub fn push_bytes(&mut self, value: Vec<u8>) -> Result<(), String> {
        self.push(StackItem::from_byte_string(value))
    }

    /// Helper to push a string to the stack
    pub fn push_string(&mut self, value: String) -> Result<(), String> {
        self.push(StackItem::from_byte_string(value.into_bytes()))
    }

    /// Helper to push an array to the stack
    pub fn push_array(&mut self, value: Vec<StackItem>) -> Result<(), String> {
        self.push(StackItem::from_array(value))
    }

    /// Helper to push null to the stack
    pub fn push_null(&mut self) -> Result<(), String> {
        self.push(StackItem::null())
    }

    /// Helper to pop a boolean from the stack
    pub fn pop_boolean(&mut self) -> Result<bool, String> {
        let item = self.pop()?;
        item.as_bool().map_err(|e| e.to_string())
    }

    /// Helper to pop an integer from the stack
    pub fn pop_integer(&mut self) -> Result<i64, String> {
        let item = self.pop()?;
        let integer = item.as_int().map_err(|e| e.to_string())?;
        integer
            .to_i64()
            .ok_or_else(|| "Integer too large".to_string())
    }

    /// Helper to pop bytes from the stack
    pub fn pop_bytes(&mut self) -> Result<Vec<u8>, String> {
        let item = self.pop()?;
        item.as_bytes().map_err(|e| e.to_string())
    }

    /// Helper to pop a string from the stack
    pub fn pop_string(&mut self) -> Result<String, String> {
        let item = self.pop()?;
        let bytes = item.as_bytes().map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|_| "Invalid UTF-8".to_string())
    }

    /// Helper to pop an array from the stack
    pub fn pop_array(&mut self) -> Result<Vec<StackItem>, String> {
        let item = self.pop()?;
        match item {
            StackItem::Array(array) => Ok(array.items()),
            StackItem::Struct(struct_item) => Ok(struct_item.items()),
            _ => Err("Expected array".to_string()),
        }
    }

    /// Helper to check if top of stack is null
    pub fn peek_is_null(&self, index: usize) -> Result<bool, String> {
        let item = self.peek(index)?;
        Ok(item.is_null())
    }

    /// Helper to convert public key to script hash
    pub fn pubkey_to_hash(&self, pubkey: &[u8]) -> UInt160 {
        // Create signature redeem script
        let script = crate::smart_contract::helper::Helper::signature_redeem_script(pubkey);
        // Hash the script
        let hash_bytes = NeoHash::hash160(&script);
        UInt160::from_bytes(&hash_bytes).expect("hash160 produces 20 bytes")
    }

    /// Helper to get current block time
    pub fn get_current_block_time(&self) -> Result<u64, String> {
        self.current_block_timestamp()
    }

    /// Reserves a notification slot, enforcing hardfork limits.
    pub fn reserve_notification_slot(&mut self) -> Result<(), String> {
        let state_arc = self.current_execution_state().map_err(|e| e.to_string())?;
        let mut state = state_arc.lock();

        if self.is_hardfork_enabled(Hardfork::HfEchidna)
            && self.trigger_type() == TriggerType::Application
            && state.notification_count >= MAX_NOTIFICATION_COUNT
        {
            return Err(format!(
                "Maximum number of notifications `{}` is reached.",
                MAX_NOTIFICATION_COUNT
            ));
        }

        state.notification_count = state.notification_count.saturating_add(1);
        Ok(())
    }

    /// Helper to emit log event
    pub fn emit_log_event(&mut self, event: crate::smart_contract::LogEventArgs) {
        self.push_log(event);
    }

    /// Helper to emit notify event
    pub fn emit_notify_event(&mut self, event: crate::smart_contract::NotifyEventArgs) {
        self.push_notification(event);
    }

    /// Ensures the notification payload size stays within protocol limits.
    pub fn ensure_notification_size(&self, state: &[StackItem]) -> Result<(), String> {
        detect_circular_reference(state)?;
        let limits = self.execution_limits();
        let serialized =
            BinarySerializer::serialize(&StackItem::from_array(state.to_vec()), limits)
                .map_err(|e| e.to_string())?;
        if serialized.len() > MAX_NOTIFICATION_SIZE {
            return Err(format!(
                "Notification size {} exceeds maximum allowed size of {} bytes",
                serialized.len(),
                MAX_NOTIFICATION_SIZE
            ));
        }
        Ok(())
    }

    /// Sends a notification once all validation passes.
    /// The container can be None for system invocations (OnPersist/PostPersist).
    pub fn send_notification(
        &mut self,
        script_hash: UInt160,
        event_name: String,
        state: Vec<StackItem>,
    ) -> Result<(), String> {
        // Get optional container (can be None for OnPersist/PostPersist triggers)
        let container = self.script_container().cloned();

        let copied = clone_notification_state(&state)?;

        let notification = NotifyEventArgs::new_with_optional_container(
            container,
            script_hash,
            event_name,
            copied,
        );
        self.emit_notify_event(notification);
        Ok(())
    }

    /// Helper to get notifications
    pub fn get_notifications(&self, hash: Option<UInt160>) -> Result<Vec<StackItem>, String> {
        let limits = self.execution_limits();
        let mut result = Vec::new();
        for notification in self.notifications() {
            if hash.map_or(true, |expected| notification.script_hash == expected) {
                result.push(notification.to_stack_item());
                if result.len() > limits.max_stack_size as usize {
                    return Err("Too many notifications".to_string());
                }
            }
        }
        Ok(result)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CompoundKey {
    Array(usize),
    Struct(usize),
    Map(usize),
}

fn detect_circular_reference(state: &[StackItem]) -> Result<(), String> {
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for item in state {
        detect_stack_item_cycle(item, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn detect_stack_item_cycle(
    item: &StackItem,
    visiting: &mut HashSet<CompoundKey>,
    visited: &mut HashSet<CompoundKey>,
) -> Result<(), String> {
    match item {
        StackItem::Array(array) => {
            let key = CompoundKey::Array(array.id());
            detect_compound_cycle(
                key,
                visiting,
                visited,
                array.items(),
                "Circular reference detected while serializing array",
            )
        }
        StackItem::Struct(struct_item) => {
            let key = CompoundKey::Struct(struct_item.id());
            detect_compound_cycle(
                key,
                visiting,
                visited,
                struct_item.items(),
                "Circular reference detected while serializing struct",
            )
        }
        StackItem::Map(map) => {
            let key = CompoundKey::Map(map.id());
            if visited.contains(&key) {
                return Ok(());
            }
            if !visiting.insert(key) {
                return Err("Circular reference detected while serializing map".to_string());
            }
            for (entry_key, entry_value) in map.items().iter() {
                detect_stack_item_cycle(entry_key, visiting, visited)?;
                detect_stack_item_cycle(entry_value, visiting, visited)?;
            }
            visiting.remove(&key);
            visited.insert(key);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn detect_compound_cycle(
    key: CompoundKey,
    visiting: &mut HashSet<CompoundKey>,
    visited: &mut HashSet<CompoundKey>,
    items: Vec<StackItem>,
    cycle_message: &str,
) -> Result<(), String> {
    if visited.contains(&key) {
        return Ok(());
    }
    if !visiting.insert(key) {
        return Err(cycle_message.to_string());
    }
    for item in items {
        detect_stack_item_cycle(&item, visiting, visited)?;
    }
    visiting.remove(&key);
    visited.insert(key);
    Ok(())
}

fn clone_notification_state(state: &[StackItem]) -> Result<Vec<StackItem>, String> {
    let mut seen = HashMap::new();
    let mut copied = Vec::with_capacity(state.len());
    for item in state {
        copied.push(clone_stack_item_as_immutable(item, &mut seen)?);
    }
    Ok(copied)
}

fn clone_stack_item_as_immutable(
    item: &StackItem,
    seen: &mut HashMap<CompoundKey, StackItem>,
) -> Result<StackItem, String> {
    match item {
        StackItem::Null => Ok(StackItem::Null),
        StackItem::Boolean(value) => Ok(StackItem::Boolean(*value)),
        StackItem::Integer(value) => Ok(StackItem::Integer(value.clone())),
        StackItem::ByteString(bytes) => Ok(StackItem::ByteString(bytes.clone())),
        StackItem::Buffer(buffer) => Ok(StackItem::ByteString(buffer.data())),
        StackItem::Pointer(pointer) => Ok(StackItem::Pointer(pointer.clone())),
        StackItem::InteropInterface(interface) => {
            Ok(StackItem::InteropInterface(interface.clone()))
        }
        StackItem::Array(array) => {
            let key = CompoundKey::Array(array.id());
            if let Some(existing) = seen.get(&key) {
                return Ok(existing.clone());
            }
            let cloned_array = Array::new_untracked(Vec::new());
            let cloned_item = StackItem::Array(cloned_array.clone());
            seen.insert(key, cloned_item.clone());
            for element in array.items() {
                let cloned = clone_stack_item_as_immutable(&element, seen)?;
                cloned_array
                    .push(cloned)
                    .map_err(|err| err.to_string())?;
            }
            Ok(cloned_item)
        }
        StackItem::Struct(struct_item) => {
            let key = CompoundKey::Struct(struct_item.id());
            if let Some(existing) = seen.get(&key) {
                return Ok(existing.clone());
            }
            let cloned_struct = Struct::new_untracked(Vec::new());
            let cloned_item = StackItem::Struct(cloned_struct.clone());
            seen.insert(key, cloned_item.clone());
            for element in struct_item.items() {
                let cloned = clone_stack_item_as_immutable(&element, seen)?;
                cloned_struct
                    .push(cloned)
                    .map_err(|err| err.to_string())?;
            }
            Ok(cloned_item)
        }
        StackItem::Map(map) => {
            let key = CompoundKey::Map(map.id());
            if let Some(existing) = seen.get(&key) {
                return Ok(existing.clone());
            }
            let cloned_map = Map::new_untracked(OrderedDictionary::new());
            let cloned_item = StackItem::Map(cloned_map.clone());
            seen.insert(key, cloned_item.clone());
            for (entry_key, entry_value) in map.items().iter() {
                let cloned_key = clone_stack_item_as_immutable(entry_key, seen)?;
                let cloned_value = clone_stack_item_as_immutable(entry_value, seen)?;
                cloned_map
                    .set(cloned_key, cloned_value)
                    .map_err(|err| err.to_string())?;
            }
            Ok(cloned_item)
        }
    }
}
