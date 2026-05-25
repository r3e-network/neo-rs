//! ApplicationLogs service for capturing execution logs and serving RPC queries.

use crate::i_event_handlers::{CommittedHandler, CommittingHandler};
use crate::ledger::block::Block as LedgerBlock;
use crate::ledger::blockchain_application_executed::ApplicationExecuted;
use crate::neo_system::NeoSystem;
use crate::persistence::{DataCache, IStore, StoreSnapshot};
use crate::smart_contract::{NotifyEventArgs, TriggerType};
use crate::vm_runtime::rpc_json::{stack_item_rpc_json, stack_items_rpc_json_per_item};
use crate::vm_runtime::StackItem;
use crate::UInt256;
use neo_vm_rs::VmState as VMState;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, warn};

use super::ApplicationLogsSettings;

/// ApplicationLogs storage and commit handler.
pub struct ApplicationLogsService {
    settings: ApplicationLogsSettings,
    store: Arc<dyn IStore>,
    snapshot: Mutex<Option<Arc<dyn StoreSnapshot>>>,
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
        self.settings
            .exception_policy
            .apply(|| self.disabled.store(true, Ordering::SeqCst));
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
            Ok(bytes) => {
                if let Err(err) = snapshot.put(key, bytes) {
                    warn!(target: "neo::application_logs", error = %err, "failed to write application log to storage");
                }
            }
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
        let stack_items: &[StackItem] = &exec.stack;
        match stack_items_rpc_json_per_item(stack_items, self.settings.max_stack_size) {
            Ok(stack) => {
                trigger.insert("stack".to_string(), Value::Array(stack));
            }
            Err(err) => {
                exception = Some(err.to_string());
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

impl CommittingHandler for ApplicationLogsService {
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

impl CommittedHandler for ApplicationLogsService {
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
        .map(|item| stack_item_rpc_json(item, None))
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
