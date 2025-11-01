// Copyright (C) 2015-2025 The Neo Project.
//
// Lightweight Application Logs reader that mirrors the C# plugin surface area.

use crate::application_logs::settings::ApplicationLogsSettings;
use crate::application_logs::store::{
    ApplicationEngineLogModel, BlockchainEventModel, BlockchainExecutionModel, ContractLogEntry,
    NeoStore,
};
use neo_core::neo_ledger::{ApplicationExecuted, Block};
use neo_core::persistence::DataCache;
use neo_core::smart_contract::TriggerType;
use neo_core::{NeoSystem, UInt160, UInt256};
use neo_vm::StackItem;
use serde_json::{json, Value as JsonValue};
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::warn;

/// Log event arguments matching the C# `LogEventArgs` contract.
#[derive(Clone)]
pub struct LogEventArgs {
    pub script_container: Option<Arc<dyn IScriptContainer>>,
    pub script_hash: Option<UInt160>,
    pub message: String,
}

/// Script container abstraction used by the log reader.
pub trait IScriptContainer: Send + Sync {
    fn hash(&self) -> UInt256;
}

/// Minimal log-reader used by the Application Logs plugin.
pub struct LogReader {
    settings: ApplicationLogsSettings,
    neo_system: Option<Arc<NeoSystem>>,
    store: Option<NeoStore>,
    log_events: VecDeque<LogEventArgs>,
}

impl LogReader {
    /// Creates a new reader with the given settings.
    pub fn new(settings: ApplicationLogsSettings) -> Self {
        Self {
            settings,
            neo_system: None,
            store: None,
            log_events: VecDeque::new(),
        }
    }

    /// Applies configuration loaded from disk.
    pub fn configure(&mut self, config: Option<JsonValue>) {
        if let Some(value) = config {
            ApplicationLogsSettings::load(&value);
            self.settings = ApplicationLogsSettings::from_config(&value);
        }
    }

    /// Resets any internal state.
    pub fn dispose(&mut self) {
        self.neo_system = None;
        self.store = None;
        self.log_events.clear();
    }

    /// Called when the node runtime is ready.
    pub fn on_system_loaded(&mut self, system: Arc<NeoSystem>) {
        if system.settings().network == self.settings.network {
            let mut store = NeoStore::new(system.store());
            store.start_block_log_batch();
            self.store = Some(store);
            self.neo_system = Some(system);
        }
    }

    /// Captures the VM executions that occur during block persistence.
    pub fn on_blockchain_committing(
        &mut self,
        system: Arc<NeoSystem>,
        block: Block,
        _snapshot: DataCache,
        application_executed_list: Vec<ApplicationExecuted>,
    ) {
        if system.settings().network != self.settings.network {
            return;
        }

        if let Some(store) = self.store.as_mut() {
            store.start_block_log_batch();
            if let Err(err) = store.put_block_log(&block, &application_executed_list) {
                warn!(target: "neo", "failed to persist block logs: {err}");
            }

            if self.settings.debug && !self.log_events.is_empty() {
                for executed in &application_executed_list {
                    if let Some(tx) = &executed.transaction {
                        let tx_hash = tx.hash();
                        let matching_logs: Vec<LogEventArgs> = self
                            .log_events
                            .iter()
                            .filter(|log| {
                                log.script_container
                                    .as_ref()
                                    .map(|container| container.hash() == tx_hash)
                                    .unwrap_or(false)
                            })
                            .cloned()
                            .collect();
                        if !matching_logs.is_empty() {
                            if let Err(err) =
                                store.put_transaction_engine_log_state(&tx_hash, &matching_logs)
                            {
                                warn!(target: "neo", "failed to persist engine logs: {err}");
                            }
                        }
                    }
                }
                self.log_events.clear();
            }
        }
    }

    /// Flushes any pending data now that the block has been committed.
    pub fn on_blockchain_committed(&mut self, system: Arc<NeoSystem>, _block: Block) {
        if system.settings().network != self.settings.network {
            return;
        }
        if let Some(store) = self.store.as_mut() {
            store.commit_block_log();
        }
    }

    /// Captures VM log events emitted by the runtime.
    pub fn on_application_engine_log(
        &mut self,
        _sender: Arc<dyn IApplicationEngine>,
        event_args: LogEventArgs,
    ) {
        if !self.settings.debug {
            return;
        }

        if let Some(system) = &self.neo_system {
            if system.settings().network != self.settings.network {
                return;
            }
        }

        self.log_events.push_back(event_args);
    }

    /// Returns a JSON representation of the requested application log.
    pub fn get_application_log(
        &self,
        hash: UInt256,
        trigger_type: Option<String>,
    ) -> Result<JsonValue, String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "Application log store not initialised".to_string())?;

        let mut executions = Vec::new();
        if let Some(model) = store
            .get_transaction_log(&hash)
            .map_err(|err| err.to_string())?
        {
            executions.push(model);
        }

        if let Some(model) = store
            .get_block_log(&hash, TriggerType::OnPersist)
            .map_err(|err| err.to_string())?
        {
            executions.push(model);
        }

        if let Some(model) = store
            .get_block_log(&hash, TriggerType::PostPersist)
            .map_err(|err| err.to_string())?
        {
            executions.push(model);
        }

        if executions.is_empty() {
            return Err("Unknown transaction/blockhash".to_string());
        }

        if let Some(trigger) = trigger_type {
            executions.retain(|execution| {
                trigger_to_str(execution.trigger).eq_ignore_ascii_case(&trigger)
            });
        }

        let json_executions: Vec<JsonValue> = executions
            .into_iter()
            .map(|execution| execution_to_json(&execution))
            .collect();

        Ok(json!({ "executions": json_executions }))
    }

    /// Console command for retrieving block logs.
    pub fn on_get_block_command(&self, block_hash_or_index: String, event_name: Option<String>) {
        let block_hash = if let Ok(index) = block_hash_or_index.parse::<u32>() {
            let Some(system) = self.neo_system.as_ref() else {
                println!("Neo system not initialised.");
                return;
            };
            match system.block_hash_at(index) {
                Some(hash) => hash,
                None => {
                    println!("Block not found.");
                    return;
                }
            }
        } else if let Ok(hash) = block_hash_or_index.parse::<UInt256>() {
            hash
        } else {
            println!("Invalid block hash.");
            return;
        };

        let Some(store) = self.store.as_ref() else {
            println!("Application log store not initialised.");
            return;
        };

        let mut outputs = Vec::new();

        if event_name.is_none() {
            if let Ok(Some(model)) = store.get_block_log(&block_hash, TriggerType::OnPersist) {
                outputs.push(model);
            }
            if let Ok(Some(model)) = store.get_block_log(&block_hash, TriggerType::PostPersist) {
                outputs.push(model);
            }
        } else if let Some(event_name) = &event_name {
            if let Ok(Some(model)) =
                store.get_block_log_with_event(&block_hash, TriggerType::OnPersist, event_name)
            {
                outputs.push(model);
            }
            if let Ok(Some(model)) =
                store.get_block_log_with_event(&block_hash, TriggerType::PostPersist, event_name)
            {
                outputs.push(model);
            }
        }

        if outputs.is_empty() {
            println!("No logs.");
            return;
        }

        for model in outputs {
            self.print_execution_to_console(&model);
            println!("--------------------------------");
        }
    }

    /// Console command for retrieving transaction logs.
    pub fn on_get_transaction_command(&self, tx_hash: UInt256, event_name: Option<String>) {
        let Some(store) = self.store.as_ref() else {
            println!("Application log store not initialised.");
            return;
        };

        let result = if let Some(event) = event_name {
            store
                .get_transaction_log_with_event(&tx_hash, &event)
                .map_err(|err| err.to_string())
        } else {
            store
                .get_transaction_log(&tx_hash)
                .map_err(|err| err.to_string())
        };

        match result {
            Ok(Some(model)) => self.print_execution_to_console(&model),
            Ok(None) => println!("No logs."),
            Err(err) => println!("Failed to fetch transaction logs: {err}"),
        }
    }

    /// Console command for retrieving contract logs.
    pub fn on_get_contract_command(
        &self,
        script_hash: UInt160,
        page: u32,
        page_size: u32,
        event_name: Option<String>,
    ) {
        if page == 0 || page_size == 0 {
            println!("Invalid paging arguments.");
            return;
        }

        let Some(store) = self.store.as_ref() else {
            println!("Application log store not initialised.");
            return;
        };

        let result = if let Some(event) = &event_name {
            store.get_contract_log_with_trigger_and_event(
                &script_hash,
                TriggerType::Application,
                event,
                page,
                page_size,
            )
        } else {
            store.get_contract_log_with_trigger(
                &script_hash,
                TriggerType::Application,
                page,
                page_size,
            )
        };

        match result {
            Ok(logs) if logs.is_empty() => println!("No logs."),
            Ok(logs) => self.print_event_model_to_console(&logs),
            Err(err) => println!("Failed to fetch contract logs: {err}"),
        }
    }

    fn print_execution_to_console(&self, model: &BlockchainExecutionModel) {
        println!("Trigger: {:?}", model.trigger);
        println!("VM State: {:?}", model.vm_state);
        if model.exception.is_empty() {
            println!("Exception: null");
        } else {
            println!("Exception: {}", model.exception);
        }
        println!("Gas Consumed: {}", model.gas_consumed);

        if model.stack.is_empty() {
            println!("Stack: []");
        } else {
            println!("Stack:");
            for (i, item) in model.stack.iter().enumerate() {
                println!("  {}: {}", i, stack_item_to_json(item));
            }
        }

        if model.notifications.is_empty() {
            println!("Notifications: []");
        } else {
            println!("Notifications:");
            for notify in &model.notifications {
                println!("  ScriptHash: {}", notify.script_hash);
                println!("  Event Name: {}", notify.event_name);
                if notify.state.is_empty() {
                    println!("  State: []");
                } else {
                    println!("  State:");
                    for (i, value) in notify.state.iter().enumerate() {
                        println!("    {}: {}", i, stack_item_to_json(value));
                    }
                }
            }
        }
        if self.settings.debug {
            if model.logs.is_empty() {
                println!("Logs: []");
            } else {
                println!("Logs:");
                for log in &model.logs {
                    println!("  ScriptHash: {}", log.script_hash);
                    println!("  Message: {}", log.message);
                }
            }
        }
    }
    fn print_event_model_to_console(&self, models: &[ContractLogEntry]) {
        for entry in models {
            println!("Transaction Hash: {}", entry.transaction_hash);
            println!("Trigger: {}", trigger_to_str(entry.trigger));
            println!("Timestamp: {}", entry.timestamp);
            println!("Notification Index: {}", entry.notification_index);
            println!("Script Hash: {}", entry.event.script_hash);
            println!("Event Name: {}", entry.event.event_name);
            if entry.event.state.is_empty() {
                println!("State: []");
            } else {
                println!("State:");
                for (i, value) in entry.event.state.iter().enumerate() {
                    println!("  {}: {}", i, stack_item_to_json(value));
                }
            }
            println!("--------------------------------");
        }
    }
}

fn execution_to_json(model: &BlockchainExecutionModel) -> JsonValue {
    json!({
        "trigger": trigger_to_str(model.trigger),
        "vmstate": format!("{:?}", model.vm_state),
        "exception": if model.exception.is_empty() {
            JsonValue::Null
        } else {
            JsonValue::String(model.exception.clone())
        },
        "gasConsumed": model.gas_consumed,
        "stack": model.stack.iter().map(stack_item_to_json).collect::<Vec<_>>(),
        "notifications": model
            .notifications
            .iter()
            .map(notification_to_json)
            .collect::<Vec<_>>(),
        "logs": model
            .logs
            .iter()
            .map(log_to_json)
            .collect::<Vec<_>>(),
    })
}

fn notification_to_json(event: &BlockchainEventModel) -> JsonValue {
    json!({
        "contract": event.script_hash.to_string(),
        "eventname": event.event_name,
        "state": event.state.iter().map(stack_item_to_json).collect::<Vec<_>>(),
    })
}

fn log_to_json(log: &ApplicationEngineLogModel) -> JsonValue {
    json!({
        "scriptHash": log.script_hash.to_string(),
        "message": log.message,
    })
}

fn stack_item_to_json(item: &StackItem) -> JsonValue {
    match item {
        StackItem::Null => JsonValue::Null,
        StackItem::Boolean(value) => JsonValue::Bool(*value),
        StackItem::Integer(value) => json!(value.to_string()),
        StackItem::ByteString(bytes) => json!(hex::encode(bytes)),
        StackItem::Buffer(buffer) => json!(hex::encode(buffer.data())),
        StackItem::Array(array) => {
            JsonValue::Array(array.items().iter().map(stack_item_to_json).collect())
        }
        StackItem::Struct(strct) => {
            JsonValue::Array(strct.items().iter().map(stack_item_to_json).collect())
        }
        StackItem::Map(map) => {
            let entries = map
                .iter()
                .map(|(key, value)| {
                    json!({
                        "key": stack_item_to_json(key),
                        "value": stack_item_to_json(value),
                    })
                })
                .collect();
            JsonValue::Array(entries)
        }
        StackItem::Pointer(pointer) => json!({
            "type": "Pointer",
            "position": pointer.position(),
        }),
        StackItem::InteropInterface(iface) => json!({
            "type": "InteropInterface",
            "interface": iface.interface_type(),
        }),
    }
}

/// Interface for the ApplicationEngine abstraction used when capturing logs.
pub trait IApplicationEngine: Send + Sync {
    fn log(&self, message: String);
}
fn trigger_to_str(trigger: TriggerType) -> &'static str {
    if trigger == TriggerType::OnPersist {
        "OnPersist"
    } else if trigger == TriggerType::PostPersist {
        "PostPersist"
    } else if trigger == TriggerType::Application {
        "Application"
    } else if trigger == TriggerType::Verification {
        "Verification"
    } else {
        "Unknown"
    }
}
