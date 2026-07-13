//! ApplicationEngine.Helper - matches C# Neo.SmartContract.ApplicationEngine helper methods exactly

use neo_config::hardfork::Hardfork;
use neo_crypto::{Crypto, ECCurve, ECPoint};
// Old wrapper types removed - StackValue compounds are flat Vecs now
use crate::NotifyEventArgs;
use crate::application_engine::{ApplicationEngine, MAX_NOTIFICATION_COUNT, MAX_NOTIFICATION_SIZE};
use neo_error::{CoreError, CoreResult};
use neo_primitives::TriggerType;
use neo_primitives::UInt160;
use neo_serialization::BinarySerializer;
use neo_vm::StackItem;
use neo_vm::stack_item::{Array as ArrayItem, Map as MapItem, Struct as StructItem};
use neo_vm_rs::StackValue;
use neo_vm_rs::VmOrderedDictionary;
use neo_vm_rs::VmState as VMState;
use num_traits::ToPrimitive;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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
    pub fn push_boolean(&mut self, value: bool) -> CoreResult<()> {
        self.push(StackItem::from_bool(value))
    }

    /// Helper to push an integer to the stack
    pub fn push_integer(&mut self, value: i64) -> CoreResult<()> {
        self.push(StackItem::from_int(value))
    }

    /// Helper to push bytes to the stack
    pub fn push_bytes(&mut self, value: Vec<u8>) -> CoreResult<()> {
        self.push(StackItem::from_byte_string(value))
    }

    /// Helper to push a string to the stack
    pub fn push_string(&mut self, value: String) -> CoreResult<()> {
        self.push(StackItem::from_byte_string(value.into_bytes()))
    }

    /// Helper to push an array to the stack
    pub fn push_array(&mut self, value: Vec<StackItem>) -> CoreResult<()> {
        self.push(StackItem::from_array(value))
    }

    /// Helper to push null to the stack
    pub fn push_null(&mut self) -> CoreResult<()> {
        self.push(StackItem::null())
    }

    /// Helper to pop a boolean from the stack
    pub fn pop_boolean(&mut self) -> CoreResult<bool> {
        let item = self.pop()?;
        item.as_bool().map_err(|e| CoreError::other(e.to_string()))
    }

    /// Helper to pop an integer from the stack
    pub fn pop_integer(&mut self) -> CoreResult<i64> {
        let item = self.pop()?;
        let integer = item
            .into_int()
            .map_err(|e| CoreError::other(e.to_string()))?;
        integer
            .to_i64()
            .ok_or_else(|| CoreError::other("Integer too large"))
    }

    /// Helper to pop bytes from the stack
    pub fn pop_bytes(&mut self) -> CoreResult<Vec<u8>> {
        let item = self.pop()?;
        item.as_bytes().map_err(|e| CoreError::other(e.to_string()))
    }

    /// Helper to pop a string from the stack
    pub fn pop_string(&mut self) -> CoreResult<String> {
        let item = self.pop()?;
        let bytes = item
            .as_bytes()
            .map_err(|e| CoreError::other(e.to_string()))?;
        String::from_utf8(bytes).map_err(|_| CoreError::other("Invalid UTF-8"))
    }

    /// Helper to pop an array from the stack
    pub fn pop_array(&mut self) -> CoreResult<Vec<StackItem>> {
        let item = self.pop()?;
        match item {
            StackItem::Array(array) => Ok(array.items()),
            StackItem::Struct(struct_item) => Ok(struct_item.items()),
            _ => Err(CoreError::other("Expected array")),
        }
    }

    /// Helper to check if top of stack is null
    pub fn peek_is_null(&self, index: usize) -> CoreResult<bool> {
        let item = self.peek(index)?;
        Ok(item.is_null())
    }

    /// Helper to convert public key to script hash
    pub fn pubkey_to_hash(&self, pubkey: &[u8]) -> CoreResult<UInt160> {
        if pubkey.len() != 33 || !matches!(pubkey.first(), Some(0x02 | 0x03)) {
            return Err(CoreError::other("Invalid public key"));
        }
        let point = ECPoint::decode(pubkey, ECCurve::secp256r1())
            .map_err(|e| CoreError::other(format!("Invalid public key: {e}")))?;
        let script = crate::helper::Helper::signature_redeem_script(&point.to_bytes());
        Ok(UInt160::from_array(Crypto::hash160(&script)))
    }

    /// Helper to get current block time
    pub fn get_current_block_time(&self) -> CoreResult<u64> {
        self.current_block_timestamp()
    }

    /// Helper to emit log event
    pub fn emit_log_event(&mut self, event: neo_payloads::LogEventArgs) {
        self.push_log(event);
    }

    /// Helper to emit notify event
    pub fn emit_notify_event(&mut self, event: crate::NotifyEventArgs) {
        self.push_notification(event);
    }

    /// Ensures the notification payload size stays within protocol limits.
    pub fn ensure_notification_size(&self, state: &[StackItem]) -> CoreResult<()> {
        detect_circular_reference(state)?;
        let limits = self.execution_limits();
        let value = notification_state_to_stack_value(state)?;
        let serialized = BinarySerializer::serialize_stack_value_with_limits(
            &value,
            MAX_NOTIFICATION_SIZE,
            limits.max_stack_size as usize,
        )?;
        if serialized.len() > MAX_NOTIFICATION_SIZE {
            return Err(CoreError::other(format!(
                "Notification size {} exceeds maximum allowed size of {} bytes",
                serialized.len(),
                MAX_NOTIFICATION_SIZE
            )));
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
    ) -> CoreResult<()> {
        if self.is_hardfork_enabled(Hardfork::HfEchidna)
            && self.trigger_type() == TriggerType::Application
            && self.notifications().len() >= MAX_NOTIFICATION_COUNT
        {
            return Err(CoreError::other(format!(
                "Maximum number of notifications `{}` is reached.",
                MAX_NOTIFICATION_COUNT
            )));
        }

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
        if let Ok(state_arc) = self.current_execution_state() {
            let mut context_state = state_arc.lock();
            context_state.notification_count = context_state.notification_count.saturating_add(1);
        }
        Ok(())
    }

    /// Helper to get notifications
    pub fn get_notifications(&self, hash: Option<UInt160>) -> CoreResult<Vec<StackItem>> {
        let limits = self.execution_limits();
        let mut result = Vec::new();
        for notification in self.notifications() {
            if hash.is_none_or(|expected| notification.script_hash == expected) {
                result.push(self.notification_to_stack_item(notification)?);
                if result.len() > limits.max_stack_size as usize {
                    return Err(CoreError::other("Too many notifications"));
                }
            }
        }
        Ok(result)
    }

    fn notification_to_stack_item(&self, notification: &NotifyEventArgs) -> CoreResult<StackItem> {
        let state = if self.is_hardfork_enabled(Hardfork::HfDomovoi) {
            readonly_array_stack_item(clone_notification_state(&notification.state)?)
        } else {
            notification.state_array()
        };
        notification
            .try_to_stack_item_with_state_array(state)
            .map_err(|error| CoreError::other(error.to_string()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CompoundKey {
    Array(usize),
    Struct(usize),
    Map(usize),
}

fn detect_circular_reference(state: &[StackItem]) -> CoreResult<()> {
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for item in state {
        detect_stack_item_cycle(item, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn notification_state_to_stack_value(state: &[StackItem]) -> CoreResult<StackValue> {
    state
        .iter()
        .cloned()
        .map(StackValue::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map(|items| StackValue::Array(neo_vm_rs::next_stack_item_id(), items))
        .map_err(|error| CoreError::other(error.to_string()))
}

fn detect_stack_item_cycle(
    item: &StackItem,
    visiting: &mut HashSet<CompoundKey>,
    visited: &mut HashSet<CompoundKey>,
) -> CoreResult<()> {
    match item {
        StackItem::Array(array) => {
            let key = CompoundKey::Array(array.id());
            detect_compound_cycle(
                key,
                visiting,
                visited,
                array.items(),
                "Circular reference detected while serializing compound",
            )
        }
        StackItem::Struct(struct_item) => {
            let key = CompoundKey::Struct(struct_item.id());
            detect_compound_cycle(
                key,
                visiting,
                visited,
                struct_item.items(),
                "Circular reference detected while serializing compound",
            )
        }
        StackItem::Map(entries) => {
            let key = CompoundKey::Map(entries.id());
            if visited.contains(&key) {
                return Ok(());
            }
            if !visiting.insert(key) {
                return Err(CoreError::other(
                    "Circular reference detected while serializing map",
                ));
            }
            for (entry_key, entry_value) in entries.iter() {
                detect_stack_item_cycle(&entry_key, visiting, visited)?;
                detect_stack_item_cycle(&entry_value, visiting, visited)?;
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
) -> CoreResult<()> {
    if visited.contains(&key) {
        return Ok(());
    }
    if !visiting.insert(key) {
        return Err(CoreError::other(cycle_message.to_string()));
    }
    for item in items {
        detect_stack_item_cycle(&item, visiting, visited)?;
    }
    visiting.remove(&key);
    visited.insert(key);
    Ok(())
}

fn clone_notification_state(state: &[StackItem]) -> CoreResult<Vec<StackItem>> {
    let mut seen = HashMap::new();
    let mut copied = Vec::with_capacity(state.len());
    for item in state {
        copied.push(clone_stack_item_as_immutable(item, &mut seen)?);
    }
    Ok(copied)
}

fn readonly_array_stack_item(items: Vec<StackItem>) -> StackItem {
    let array = ArrayItem::new_untracked(items);
    array.set_read_only(true);
    StackItem::Array(array)
}

fn clone_stack_item_as_immutable(
    item: &StackItem,
    seen: &mut HashMap<CompoundKey, StackItem>,
) -> CoreResult<StackItem> {
    match item {
        StackItem::Null => Ok(StackItem::Null),
        StackItem::Boolean(value) => Ok(StackItem::Boolean(*value)),
        StackItem::Integer(value) => Ok(StackItem::Integer(value.clone())),
        StackItem::ByteString(bytes) => Ok(StackItem::ByteString(bytes.clone())),
        // Immutable clone (C# DeepCopy asImmutable=true): Buffer collapses to ByteString.
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
            let cloned = ArrayItem::new_untracked(Vec::new());
            let cloned_item = StackItem::Array(cloned.clone());
            seen.insert(key, cloned_item.clone());
            for element in array.iter() {
                let child = clone_stack_item_as_immutable(&element, seen)?;
                cloned
                    .push(child)
                    .map_err(|e| CoreError::other(e.to_string()))?;
            }
            cloned.set_read_only(true);
            Ok(cloned_item)
        }
        StackItem::Struct(struct_item) => {
            let key = CompoundKey::Struct(struct_item.id());
            if let Some(existing) = seen.get(&key) {
                return Ok(existing.clone());
            }
            let cloned = StructItem::new_untracked(Vec::new());
            let cloned_item = StackItem::Struct(cloned.clone());
            seen.insert(key, cloned_item.clone());
            for element in struct_item.iter() {
                let child = clone_stack_item_as_immutable(&element, seen)?;
                cloned
                    .push(child)
                    .map_err(|e| CoreError::other(e.to_string()))?;
            }
            cloned.set_read_only(true);
            Ok(cloned_item)
        }
        StackItem::Map(entries) => {
            let key = CompoundKey::Map(entries.id());
            if let Some(existing) = seen.get(&key) {
                return Ok(existing.clone());
            }
            let cloned = MapItem::new_untracked(VmOrderedDictionary::new());
            let cloned_item = StackItem::Map(cloned.clone());
            seen.insert(key, cloned_item.clone());
            for (entry_key, entry_value) in entries.iter() {
                let cloned_key = clone_stack_item_as_immutable(&entry_key, seen)?;
                let cloned_value = clone_stack_item_as_immutable(&entry_value, seen)?;
                cloned
                    .set(cloned_key, cloned_value)
                    .map_err(|e| CoreError::other(e.to_string()))?;
            }
            cloned.set_read_only(true);
            Ok(cloned_item)
        }
    }
}
