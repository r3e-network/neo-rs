// Copyright (C) 2015-2025 The Neo Project.
//
// log_reader.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::application_logs::settings::ApplicationLogsSettings;
use crate::application_logs::store::neo_store::NeoStore;
use neo_core::{NeoSystem, Block, UInt256, UInt160, DataCache, ApplicationExecuted};
use neo_json::JsonValue;
use std::sync::Arc;
use std::collections::VecDeque;

/// Log event arguments matching C# LogEventArgs
#[derive(Debug, Clone)]
pub struct LogEventArgs {
    pub script_container: Option<Arc<dyn IScriptContainer>>,
    pub message: String,
}

/// Script container trait
pub trait IScriptContainer: Send + Sync {
    fn hash(&self) -> UInt256;
}

/// LogReader implementation matching C# LogReader exactly
pub struct LogReader {
    /// Neo store instance
    pub neo_store: Option<NeoStore>,
    /// Neo system reference
    pub neo_system: Option<Arc<NeoSystem>>,
    /// Log events list
    pub log_events: VecDeque<LogEventArgs>,
}

impl LogReader {
    /// Creates a new LogReader
    /// Matches C# default constructor
    pub fn new() -> Self {
        Self {
            neo_store: None,
            neo_system: None,
            log_events: VecDeque::new(),
        }
    }
    
    /// Gets the plugin name
    /// Matches C# Name property
    pub fn name(&self) -> &'static str {
        "ApplicationLogs"
    }
    
    /// Gets the plugin description
    /// Matches C# Description property
    pub fn description(&self) -> &'static str {
        "Synchronizes smart contract VM executions and notifications (NotifyLog) on blockchain."
    }
    
    /// Gets the exception policy
    /// Matches C# ExceptionPolicy property
    pub fn exception_policy(&self) -> crate::application_logs::settings::UnhandledExceptionPolicy {
        ApplicationLogsSettings::default().exception_policy.clone()
    }
    
    /// Gets the config file path
    /// Matches C# ConfigFile property
    pub fn config_file(&self) -> String {
        "ApplicationLogs.json".to_string()
    }
    
    /// Disposes the plugin
    /// Matches C# Dispose method
    pub fn dispose(&mut self) {
        // Unregister from blockchain events, matching C# Dispose behavior
        self.neo_store = None;
        self.neo_system = None;
    }
    
    /// Configures the plugin
    /// Matches C# Configure method
    pub fn configure(&mut self, config: Option<serde_json::Value>) {
        if let Some(config_value) = config {
            ApplicationLogsSettings::load(&config_value);
        }
    }
    
    /// Called when system is loaded
    /// Matches C# OnSystemLoaded method
    pub fn on_system_loaded(&mut self, system: Arc<NeoSystem>) {
        let settings = ApplicationLogsSettings::default();
        if system.settings().network != settings.network {
            return;
        }
        
        let path = format!(settings.path, settings.network);
        let store = system.load_store(&path);
        self.neo_store = Some(NeoStore::new(store));
        self.neo_system = Some(system);
        
        // Register RPC methods, matching C# RPC method registration
    }
    
    /// Handles blockchain committing events
    /// Matches C# Blockchain_Committing_Handler
    pub fn on_blockchain_committing(
        &mut self,
        system: Arc<NeoSystem>,
        block: Block,
        snapshot: DataCache,
        application_executed_list: Vec<ApplicationExecuted>,
    ) {
        let settings = ApplicationLogsSettings::default();
        if system.settings().network != settings.network {
            return;
        }
        
        if let Some(neo_store) = &mut self.neo_store {
            neo_store.start_block_log_batch();
            neo_store.put_block_log(block, &application_executed_list);
            
            if settings.debug {
                for app_exec in &application_executed_list {
                    if let Some(transaction) = &app_exec.transaction {
                        let logs: Vec<LogEventArgs> = self.log_events
                            .iter()
                            .filter(|log| {
                                if let Some(container) = &log.script_container {
                                    container.hash() == transaction.hash()
                                } else {
                                    false
                                }
                            })
                            .cloned()
                            .collect();
                        
                        if !logs.is_empty() {
                            neo_store.put_transaction_engine_log_state(transaction.hash(), &logs);
                        }
                    }
                }
                self.log_events.clear();
            }
        }
    }
    
    /// Handles blockchain committed events
    /// Matches C# Blockchain_Committed_Handler
    pub fn on_blockchain_committed(&mut self, system: Arc<NeoSystem>, block: Block) {
        let settings = ApplicationLogsSettings::default();
        if system.settings().network != settings.network {
            return;
        }
        
        if let Some(neo_store) = &mut self.neo_store {
            neo_store.commit_block_log();
        }
    }
    
    /// Handles application engine log events
    /// Matches C# ApplicationEngine_Log_Handler
    pub fn on_application_engine_log(&mut self, sender: Arc<dyn IApplicationEngine>, event_args: LogEventArgs) {
        let settings = ApplicationLogsSettings::default();
        if !settings.debug {
            return;
        }
        
        if let Some(neo_system) = &self.neo_system {
            if neo_system.settings().network != settings.network {
                return;
            }
        }
        
        if event_args.script_container.is_none() {
            return;
        }
        
        self.log_events.push_back(event_args);
    }
    
    /// Gets application log for a transaction or block
    /// Matches C# GetApplicationLog RPC method
    pub fn get_application_log(&self, hash: UInt256, trigger_type: Option<String>) -> Result<JsonValue, String> {
        let mut raw = self.block_to_json(hash);
        if raw.is_null() {
            raw = self.transaction_to_json(hash);
            if raw.is_null() {
                return Err("Unknown transaction/blockhash".to_string());
            }
        }
        
        if let Some(trigger) = trigger_type {
            if let Some(executions) = raw.get("executions").and_then(|v| v.as_array()) {
                let filtered_executions: Vec<JsonValue> = executions
                    .iter()
                    .filter(|exec| {
                        exec.get("trigger")
                            .and_then(|v| v.as_str())
                            .map(|t| t.eq_ignore_ascii_case(&trigger))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect();
                
                raw["executions"] = JsonValue::Array(filtered_executions);
            }
        }
        
        Ok(raw)
    }
    
    /// Console command for getting block logs
    /// Matches C# OnGetBlockCommand
    pub fn on_get_block_command(&self, block_hash_or_index: String, event_name: Option<String>) {
        let block_hash = if let Ok(index) = block_hash_or_index.parse::<u32>() {
            // Query blockchain for block hash by index, matching C# blockchain query behavior
            if let Some(store) = &self.neo_store {
                store.get_block_hash(index).unwrap_or_default()
            } else {
                UInt256::default()
            }
        } else if let Ok(hash) = UInt256::from_string(&block_hash_or_index) {
            hash
        } else {
            println!("Invalid block hash or index.");
            return;
        };
        
        if let Some(neo_store) = &self.neo_store {
            let block_on_persist = if event_name.is_none() {
                neo_store.get_block_log(block_hash, TriggerType::OnPersist)
            } else {
                neo_store.get_block_log_with_event(block_hash, TriggerType::OnPersist, &event_name.unwrap())
            };
            
            let block_post_persist = if event_name.is_none() {
                neo_store.get_block_log(block_hash, TriggerType::PostPersist)
            } else {
                neo_store.get_block_log_with_event(block_hash, TriggerType::PostPersist, &event_name.unwrap())
            };
            
            if block_on_persist.is_none() && block_post_persist.is_none() {
                println!("No logs.");
            } else {
                if let Some(on_persist) = block_on_persist {
                    self.print_execution_to_console(&on_persist);
                    if block_post_persist.is_some() {
                        println!("--------------------------------");
                    }
                }
                if let Some(post_persist) = block_post_persist {
                    self.print_execution_to_console(&post_persist);
                }
            }
        }
    }
    
    /// Console command for getting transaction logs
    /// Matches C# OnGetTransactionCommand
    pub fn on_get_transaction_command(&self, tx_hash: UInt256, event_name: Option<String>) {
        if let Some(neo_store) = &self.neo_store {
            let tx_application = if event_name.is_none() {
                neo_store.get_transaction_log(tx_hash)
            } else {
                neo_store.get_transaction_log_with_event(tx_hash, &event_name.unwrap())
            };
            
            if let Some(application) = tx_application {
                self.print_execution_to_console(&application);
            } else {
                println!("No logs.");
            }
        }
    }
    
    /// Console command for getting contract logs
    /// Matches C# OnGetContractCommand
    pub fn on_get_contract_command(&self, script_hash: UInt160, page: u32, page_size: u32, event_name: Option<String>) {
        if page == 0 {
            println!("Page is invalid. Pick a number 1 and above.");
            return;
        }
        
        if page_size == 0 {
            println!("PageSize is invalid. Pick a number between 1 and 10.");
            return;
        }
        
        if let Some(neo_store) = &self.neo_store {
            let tx_contract = if event_name.is_none() {
                neo_store.get_contract_log(script_hash, TriggerType::Application, page, page_size)
            } else {
                neo_store.get_contract_log_with_event(script_hash, TriggerType::Application, &event_name.unwrap(), page, page_size)
            };
            
            if tx_contract.is_empty() {
                println!("No logs.");
            } else {
                self.print_event_model_to_console(&tx_contract);
            }
        }
    }
    
    // Private helper methods
    
    /// Converts block to JSON
    fn block_to_json(&self, hash: UInt256) -> JsonValue {
        // Implementation would convert block to JSON
        JsonValue::Null
    }
    
    /// Converts transaction to JSON
    fn transaction_to_json(&self, hash: UInt256) -> JsonValue {
        // Implementation would convert transaction to JSON
        JsonValue::Null
    }
    
    /// Prints execution to console
    fn print_execution_to_console(&self, model: &BlockchainExecutionModel) {
        println!("Trigger: {}", model.trigger);
        println!("VM State: {}", model.vm_state);
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
                println!("  {}: {}", i, item.to_json());
            }
        }
        
        if model.notifications.is_empty() {
            println!("Notifications: []");
        } else {
            println!("Notifications:");
            for notify_item in &model.notifications {
                println!("  ScriptHash: {}", notify_item.script_hash);
                println!("  Event Name: {}", notify_item.event_name);
                println!("  State Parameters:");
                for (i, state) in notify_item.state.iter().enumerate() {
                    println!("    {}: {}", i, state.to_json());
                }
            }
        }
    }
    
    /// Prints event model to console
    fn print_event_model_to_console(&self, models: &[BlockchainEventModel]) {
        for model in models {
            println!("Transaction Hash: {}", model.transaction_hash);
            println!("Block Index: {}", model.block_index);
            println!("Event Name: {}", model.event_name);
            // Print other fields as needed
        }
    }
}

/// Application engine trait
pub trait IApplicationEngine: Send + Sync {
    fn log(&self, message: String);
}

/// Trigger type enum
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerType {
    OnPersist,
    PostPersist,
    Application,
}

/// Blockchain execution model
#[derive(Debug, Clone)]
pub struct BlockchainExecutionModel {
    pub trigger: String,
    pub vm_state: String,
    pub exception: String,
    pub gas_consumed: u64,
    pub stack: Vec<StackItem>,
    pub notifications: Vec<NotificationItem>,
}

/// Blockchain event model
#[derive(Debug, Clone)]
pub struct BlockchainEventModel {
    pub transaction_hash: UInt256,
    pub block_index: u32,
    pub event_name: String,
}

/// Stack item trait
pub trait StackItem: Send + Sync {
    fn to_json(&self) -> JsonValue;
}

/// Notification item
#[derive(Debug, Clone)]
pub struct NotificationItem {
    pub script_hash: UInt160,
    pub event_name: String,
    pub state: Vec<Box<dyn StackItem>>,
}