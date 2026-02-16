//! ApplicationLogs service for capturing execution logs and serving RPC queries.

use crate::UInt256;
use crate::i_event_handlers::{ICommittedHandler, ICommittingHandler};
use crate::ledger::block::Block as LedgerBlock;
use crate::ledger::blockchain_application_executed::ApplicationExecuted;
use crate::neo_system::NeoSystem;
use crate::persistence::{DataCache, IStore, IStoreSnapshot};
use crate::smart_contract::{NotifyEventArgs, TriggerType};
use crate::unhandled_exception_policy::UnhandledExceptionPolicy;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_vm::stack_item::StackItemType;
use neo_vm::{StackItem, VMState};
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::any::Any;
use std::collections::HashSet;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{error, warn};

use super::ApplicationLogsSettings;

/// ApplicationLogs storage and commit handler.
pub struct ApplicationLogsService {
    settings: ApplicationLogsSettings,
    store: Arc<dyn IStore>,
    snapshot: Mutex<Option<Arc<dyn IStoreSnapshot>>>,
    disabled: AtomicBool,
}

impl ApplicationLogsService {
    const PREFIX_BLOCK: u8 = 0x40;
    const PREFIX_TX: u8 = 0x41;

    /// Creates a new ApplicationLogs service.
    pub fn new(settings: ApplicationLogsSettings, store: Arc<dyn IStore>) -> Self {
        Self {
            settings,
            store,
            snapshot: Mutex::new(None),
            disabled: AtomicBool::new(false),
        }
    }

    /// Returns the settings in use.
    pub fn settings(&self) -> &ApplicationLogsSettings {
        &self.settings
    }

    /// Returns the stored block log JSON, if present.
    pub fn get_block_log(&self, hash: &UInt256) -> Option<Value> {
        self.read_log(Self::PREFIX_BLOCK, hash)
    }

    /// Returns the stored transaction log JSON, if present.
    pub fn get_transaction_log(&self, hash: &UInt256) -> Option<Value> {
        self.read_log(Self::PREFIX_TX, hash)
    }

    fn start_batch(&self) {
        let mut guard = self.snapshot.lock();
        *guard = Some(self.store.get_snapshot());
    }

    fn commit_batch(&self) {
        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return;
        };
        if let Some(snapshot) = Arc::get_mut(snapshot_arc) {
            if let Err(err) = snapshot.try_commit() {
                warn!(target: "neo::application_logs", error = %err, "application logs commit failed");
            }
        }
    }

    fn handle_panic(&self, payload: Box<dyn Any + Send>, phase: &'static str) {
        error!(
            target: "neo::application_logs",
            phase,
            error = panic_message(&payload),
            "application logs handler panicked"
        );
        match self.settings.exception_policy {
            UnhandledExceptionPolicy::StopPlugin => {
                self.disabled.store(true, Ordering::SeqCst);
            }
            UnhandledExceptionPolicy::StopNode => std::process::exit(1),
            UnhandledExceptionPolicy::Terminate => std::process::abort(),
            UnhandledExceptionPolicy::Ignore | UnhandledExceptionPolicy::Continue => {}
        }
    }

    fn write_log(&self, prefix: u8, hash: &UInt256, value: Value) {
        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return;
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return;
        };
        let mut key = Vec::with_capacity(1 + 32);
        key.push(prefix);
        key.extend_from_slice(&hash.to_bytes());
        match serde_json::to_vec(&value) {
            Ok(bytes) => snapshot.put(key, bytes),
            Err(err) => {
                warn!(target: "neo::application_logs", error = %err, "failed to serialize application log")
            }
        }
    }

    fn read_log(&self, prefix: u8, hash: &UInt256) -> Option<Value> {
        let mut key = Vec::with_capacity(1 + 32);
        key.push(prefix);
        key.extend_from_slice(&hash.to_bytes());
        let snapshot = self.store.get_snapshot();
        let raw = snapshot.try_get(&key)?;
        serde_json::from_slice(&raw).ok()
    }

    fn block_log_json(&self, block_hash: &UInt256, executions: &[ApplicationExecuted]) -> Value {
        let block_executions = executions
            .iter()
            .filter(|exec| exec.transaction.is_none())
            .map(|exec| self.execution_to_json(exec, false))
            .collect::<Vec<_>>();
        let mut obj = Map::new();
        obj.insert(
            "blockhash".to_string(),
            Value::String(block_hash.to_string()),
        );
        obj.insert("executions".to_string(), Value::Array(block_executions));
        Value::Object(obj)
    }

    fn transaction_log_json(&self, tx_hash: &UInt256, exec: &ApplicationExecuted) -> Value {
        let mut obj = Map::new();
        obj.insert("txid".to_string(), Value::String(tx_hash.to_string()));
        obj.insert(
            "executions".to_string(),
            Value::Array(vec![self.execution_to_json(exec, true)]),
        );
        Value::Object(obj)
    }

    fn execution_to_json(&self, exec: &ApplicationExecuted, include_exception: bool) -> Value {
        let mut trigger = Map::new();
        trigger.insert(
            "trigger".to_string(),
            Value::String(trigger_to_string(exec.trigger).to_string()),
        );
        trigger.insert(
            "vmstate".to_string(),
            Value::String(vm_state_to_string(exec.vm_state).to_string()),
        );
        trigger.insert(
            "gasconsumed".to_string(),
            Value::String(exec.gas_consumed.to_string()),
        );

        let mut exception = include_exception.then(|| exec.exception.clone()).flatten();
        match stack_items_to_json(&exec.stack, self.settings.max_stack_size) {
            Ok(stack) => {
                trigger.insert("stack".to_string(), Value::Array(stack));
            }
            Err(err) => {
                exception = Some(err);
            }
        }

        if include_exception || exception.is_some() {
            trigger.insert(
                "exception".to_string(),
                exception.map(Value::String).unwrap_or(Value::Null),
            );
        }

        let notifications = exec
            .notifications
            .iter()
            .map(notification_to_json)
            .collect::<Vec<_>>();
        trigger.insert("notifications".to_string(), Value::Array(notifications));

        if self.settings.debug {
            let logs = exec
                .logs
                .iter()
                .map(|log| {
                    let mut obj = Map::new();
                    obj.insert(
                        "contract".to_string(),
                        Value::String(log.script_hash.to_string()),
                    );
                    obj.insert("message".to_string(), Value::String(log.message.clone()));
                    Value::Object(obj)
                })
                .collect();
            trigger.insert("logs".to_string(), Value::Array(logs));
        }

        Value::Object(trigger)
    }
}

impl ICommittingHandler for ApplicationLogsService {
    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &LedgerBlock,
        _snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            self.start_batch();

            let block_hash = block.hash();
            let block_log = self.block_log_json(&block_hash, application_executed_list);
            self.write_log(Self::PREFIX_BLOCK, &block_hash, block_log);

            for exec in application_executed_list {
                let Some(tx) = exec.transaction.as_ref() else {
                    continue;
                };
                let tx_hash = tx.hash();
                let tx_log = self.transaction_log_json(&tx_hash, exec);
                self.write_log(Self::PREFIX_TX, &tx_hash, tx_log);
            }
        }));
        if let Err(payload) = result {
            self.handle_panic(payload, "committing");
        }
    }
}

impl ICommittedHandler for ApplicationLogsService {
    fn blockchain_committed_handler(&self, system: &dyn Any, _block: &LedgerBlock) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(system) = system.downcast_ref::<NeoSystem>() else {
            return;
        };
        if system.settings().network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            self.commit_batch();
        }));
        if let Err(payload) = result {
            self.handle_panic(payload, "committed");
        }
    }
}

fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

fn trigger_to_string(trigger: TriggerType) -> &'static str {
    if trigger == TriggerType::ON_PERSIST {
        "OnPersist"
    } else if trigger == TriggerType::POST_PERSIST {
        "PostPersist"
    } else if trigger == TriggerType::VERIFICATION {
        "Verification"
    } else if trigger == TriggerType::APPLICATION {
        "Application"
    } else if trigger == TriggerType::SYSTEM {
        "System"
    } else if trigger == TriggerType::ALL {
        "All"
    } else {
        "Unknown"
    }
}

fn vm_state_to_string(state: VMState) -> &'static str {
    match state {
        VMState::NONE => "NONE",
        VMState::HALT => "HALT",
        VMState::FAULT => "FAULT",
        VMState::BREAK => "BREAK",
    }
}

fn stack_items_to_json(items: &[StackItem], max_size: usize) -> Result<Vec<Value>, String> {
    items
        .iter()
        .map(|item| {
            let mut context = HashSet::new();
            let mut remaining = isize::try_from(max_size).unwrap_or(isize::MAX);
            stack_item_to_json(item, &mut context, &mut remaining)
        })
        .collect()
}

fn stack_item_to_json(
    item: &StackItem,
    context: &mut HashSet<(usize, StackItemType)>,
    remaining: &mut isize,
) -> Result<Value, String> {
    let type_name = format!("{:?}", item.stack_item_type());
    let mut json = Map::new();
    json.insert("type".to_string(), Value::String(type_name.clone()));
    subtract_size(remaining, 11 + type_name.len() as isize)?;

    let mut json_value = None;
    match item {
        StackItem::Null | StackItem::InteropInterface(_) => {}
        StackItem::Boolean(value) => {
            subtract_size(remaining, if *value { 4 } else { 5 })?;
            json_value = Some(Value::Bool(*value));
        }
        StackItem::Integer(value) => {
            let text = value.to_string();
            subtract_size(remaining, 2 + text.len() as isize)?;
            json_value = Some(Value::String(text));
        }
        StackItem::ByteString(bytes) => {
            let encoded = BASE64_STANDARD.encode(bytes);
            subtract_size(remaining, 2 + encoded.len() as isize)?;
            json_value = Some(Value::String(encoded));
        }
        StackItem::Buffer(buffer) => {
            let data = buffer.data();
            let encoded = BASE64_STANDARD.encode(data);
            subtract_size(remaining, 2 + encoded.len() as isize)?;
            json_value = Some(Value::String(encoded));
        }
        StackItem::Pointer(pointer) => {
            let text = pointer.position().to_string();
            subtract_size(remaining, text.len() as isize)?;
            json_value = Some(Value::Number(serde_json::Number::from(
                pointer.position() as u64
            )));
        }
        StackItem::Array(array) => {
            let id = array.id();
            let key = (id, StackItemType::Array);
            if !context.insert(key) {
                return Err("Circular reference.".to_string());
            }
            let count = array.items().len();
            subtract_size(remaining, 2 + count.saturating_sub(1) as isize)?;
            let entries = array
                .items()
                .iter()
                .map(|entry| stack_item_to_json(entry, context, remaining))
                .collect::<Result<Vec<_>, _>>()?;
            context.remove(&key);
            json_value = Some(Value::Array(entries));
        }
        StackItem::Struct(structure) => {
            let id = structure.id();
            let key = (id, StackItemType::Struct);
            if !context.insert(key) {
                return Err("Circular reference.".to_string());
            }
            let count = structure.items().len();
            subtract_size(remaining, 2 + count.saturating_sub(1) as isize)?;
            let entries = structure
                .items()
                .iter()
                .map(|entry| stack_item_to_json(entry, context, remaining))
                .collect::<Result<Vec<_>, _>>()?;
            context.remove(&key);
            json_value = Some(Value::Array(entries));
        }
        StackItem::Map(map) => {
            let id = map.id();
            let key = (id, StackItemType::Map);
            if !context.insert(key) {
                return Err("Circular reference.".to_string());
            }
            let count = map.items().len();
            subtract_size(remaining, 2 + count.saturating_sub(1) as isize)?;
            let entries = map
                .items()
                .iter()
                .map(|(key, value)| {
                    subtract_size(remaining, 17)?;
                    let key_json = stack_item_to_json(key, context, remaining)?;
                    let value_json = stack_item_to_json(value, context, remaining)?;
                    let mut entry = Map::new();
                    entry.insert("key".to_string(), key_json);
                    entry.insert("value".to_string(), value_json);
                    Ok(Value::Object(entry))
                })
                .collect::<Result<Vec<_>, String>>()?;
            context.remove(&key);
            json_value = Some(Value::Array(entries));
        }
    }

    if let Some(value) = json_value {
        subtract_size(remaining, 9)?;
        json.insert("value".to_string(), value);
    }

    Ok(Value::Object(json))
}

fn notification_to_json(event: &NotifyEventArgs) -> Value {
    let mut notification = Map::new();
    notification.insert(
        "contract".to_string(),
        Value::String(event.script_hash.to_string()),
    );
    notification.insert(
        "eventname".to_string(),
        Value::String(event.event_name.clone()),
    );

    let state_values = event
        .state
        .iter()
        .map(|item| {
            let mut context = HashSet::new();
            let mut remaining = isize::MAX;
            stack_item_to_json(item, &mut context, &mut remaining)
        })
        .collect::<Result<Vec<_>, _>>();

    let state = match state_values {
        Ok(values) => {
            let mut state_obj = Map::new();
            state_obj.insert("type".to_string(), Value::String("Array".to_string()));
            state_obj.insert("value".to_string(), Value::Array(values));
            Value::Object(state_obj)
        }
        Err(_) => Value::String("error: recursive reference".to_string()),
    };
    notification.insert("state".to_string(), state);

    Value::Object(notification)
}

fn subtract_size(remaining: &mut isize, amount: isize) -> Result<(), String> {
    *remaining = remaining.checked_sub(amount).unwrap_or(-1);
    if *remaining < 0 {
        return Err("Max size reached.".to_string());
    }
    Ok(())
}
