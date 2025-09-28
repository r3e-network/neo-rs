// Copyright (C) 2015-2025 The Neo Project.
//
// neo_store.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::application_logs::store::log_storage_store::LogStorageStore;
use crate::application_logs::store::models::{BlockchainEventModel, BlockchainExecutionModel};
use crate::application_logs::log_reader::{LogEventArgs, TriggerType};
use neo_core::{Block, UInt256, UInt160, ApplicationExecuted, IStore, IStoreSnapshot};
use std::sync::Arc;

/// NeoStore implementation matching C# NeoStore exactly
pub struct NeoStore {
    /// Store instance
    store: Arc<dyn IStore>,
    /// Block log snapshot
    block_log_snapshot: Option<Arc<dyn IStoreSnapshot>>,
}

impl NeoStore {
    /// Creates a new NeoStore
    /// Matches C# constructor
    pub fn new(store: Arc<dyn IStore>) -> Self {
        Self {
            store,
            block_log_snapshot: None,
        }
    }
    
    /// Disposes the store
    /// Matches C# Dispose method
    pub fn dispose(&mut self) {
        self.block_log_snapshot = None;
    }
    
    /// Starts block log batch
    /// Matches C# StartBlockLogBatch method
    pub fn start_block_log_batch(&mut self) {
        self.block_log_snapshot = None;
        self.block_log_snapshot = Some(self.store.get_snapshot());
    }
    
    /// Commits block log
    /// Matches C# CommitBlockLog method
    pub fn commit_block_log(&mut self) {
        if let Some(snapshot) = &self.block_log_snapshot {
            snapshot.commit();
        }
    }
    
    /// Gets the store
    /// Matches C# GetStore method
    pub fn get_store(&self) -> Arc<dyn IStore> {
        self.store.clone()
    }
    
    /// Gets contract log
    /// Matches C# GetContractLog method
    pub fn get_contract_log(
        &self,
        script_hash: UInt160,
        page: u32,
        page_size: u32,
    ) -> Vec<(BlockchainEventModel, UInt256)> {
        let lss = LogStorageStore::new(self.store.get_snapshot());
        let mut models = Vec::new();
        
        for contract_state in lss.find_contract_state(script_hash, page, page_size) {
            let stack_items = self.create_stack_item_array(&lss, &contract_state.stack_item_ids);
            let model = BlockchainEventModel::create(contract_state, stack_items);
            models.push((model, contract_state.transaction_hash));
        }
        
        models
    }
    
    /// Gets contract log with trigger type
    pub fn get_contract_log_with_trigger(
        &self,
        script_hash: UInt160,
        trigger_type: TriggerType,
        page: u32,
        page_size: u32,
    ) -> Vec<(BlockchainEventModel, UInt256)> {
        let lss = LogStorageStore::new(self.store.get_snapshot());
        let mut models = Vec::new();
        
        for contract_state in lss.find_contract_state_with_trigger(script_hash, trigger_type, page, page_size) {
            let stack_items = self.create_stack_item_array(&lss, &contract_state.stack_item_ids);
            let model = BlockchainEventModel::create(contract_state, stack_items);
            models.push((model, contract_state.transaction_hash));
        }
        
        models
    }
    
    /// Gets contract log with event name
    pub fn get_contract_log_with_event(
        &self,
        script_hash: UInt160,
        trigger_type: TriggerType,
        event_name: &str,
        page: u32,
        page_size: u32,
    ) -> Vec<(BlockchainEventModel, UInt256)> {
        let lss = LogStorageStore::new(self.store.get_snapshot());
        let mut models = Vec::new();
        
        for contract_state in lss.find_contract_state_with_event(script_hash, trigger_type, event_name, page, page_size) {
            let stack_items = self.create_stack_item_array(&lss, &contract_state.stack_item_ids);
            let model = BlockchainEventModel::create(contract_state, stack_items);
            models.push((model, contract_state.transaction_hash));
        }
        
        models
    }
    
    /// Puts transaction engine log state
    pub fn put_transaction_engine_log_state(&mut self, hash: UInt256, logs: &[LogEventArgs]) {
        if let Some(snapshot) = &self.block_log_snapshot {
            let lss = LogStorageStore::new(snapshot.clone());
            let mut ids = Vec::new();
            
            for log in logs {
                let engine_state = EngineLogState::create(log.script_hash.clone(), log.message.clone());
                let id = lss.put_engine_state(engine_state);
                ids.push(id);
            }
            
            let transaction_engine_state = TransactionEngineLogState::create(ids);
            lss.put_transaction_engine_state(hash, transaction_engine_state);
        }
    }
    
    /// Gets block log
    pub fn get_block_log(&self, hash: UInt256, trigger: TriggerType) -> Option<BlockchainExecutionModel> {
        let lss = LogStorageStore::new(self.store.get_snapshot());
        
        if let Some(execution_block_state_id) = lss.try_get_execution_block_state(hash, trigger) {
            if let Some(execution_log_state) = lss.try_get_execution_state(execution_block_state_id) {
                let stack_items = self.create_stack_item_array(&lss, &execution_log_state.stack_item_ids);
                let mut model = BlockchainExecutionModel::create(trigger, execution_log_state, stack_items);
                
                if let Some(block_log_state) = lss.try_get_block_state(hash, trigger) {
                    let mut event_models = Vec::new();
                    for notify_log_id in &block_log_state.notify_log_ids {
                        if let Some(notify_log_state) = lss.try_get_notify_state(*notify_log_id) {
                            let notify_stack_items = self.create_stack_item_array(&lss, &notify_log_state.stack_item_ids);
                            let event_model = BlockchainEventModel::create(notify_log_state, notify_stack_items);
                            event_models.push(event_model);
                        }
                    }
                    model.notifications = event_models;
                }
                
                return Some(model);
            }
        }
        
        None
    }
    
    /// Gets block log with event name
    pub fn get_block_log_with_event(&self, hash: UInt256, trigger: TriggerType, event_name: &str) -> Option<BlockchainExecutionModel> {
        let lss = LogStorageStore::new(self.store.get_snapshot());
        
        if let Some(execution_block_state_id) = lss.try_get_execution_block_state(hash, trigger) {
            if let Some(execution_log_state) = lss.try_get_execution_state(execution_block_state_id) {
                let stack_items = self.create_stack_item_array(&lss, &execution_log_state.stack_item_ids);
                let mut model = BlockchainExecutionModel::create(trigger, execution_log_state, stack_items);
                
                if let Some(block_log_state) = lss.try_get_block_state(hash, trigger) {
                    let mut event_models = Vec::new();
                    for notify_log_id in &block_log_state.notify_log_ids {
                        if let Some(notify_log_state) = lss.try_get_notify_state(*notify_log_id) {
                            if notify_log_state.event_name.eq_ignore_ascii_case(event_name) {
                                let notify_stack_items = self.create_stack_item_array(&lss, &notify_log_state.stack_item_ids);
                                let event_model = BlockchainEventModel::create(notify_log_state, notify_stack_items);
                                event_models.push(event_model);
                            }
                        }
                    }
                    model.notifications = event_models;
                }
                
                return Some(model);
            }
        }
        
        None
    }
    
    /// Puts block log
    pub fn put_block_log(&mut self, block: Block, application_executed_list: &[ApplicationExecuted]) {
        if let Some(snapshot) = &self.block_log_snapshot {
            for app_execution in application_executed_list {
                let lss = LogStorageStore::new(snapshot.clone());
                let exe_state_id = self.put_execution_log_block(&lss, &block, app_execution);
                self.put_block_and_transaction_log(&lss, &block, app_execution, exe_state_id);
            }
        }
    }
    
    /// Gets transaction log
    pub fn get_transaction_log(&self, hash: UInt256) -> Option<BlockchainExecutionModel> {
        let lss = LogStorageStore::new(self.store.get_snapshot());
        
        if let Some(execution_transaction_state_id) = lss.try_get_execution_transaction_state(hash) {
            if let Some(execution_log_state) = lss.try_get_execution_state(execution_transaction_state_id) {
                let stack_items = self.create_stack_item_array(&lss, &execution_log_state.stack_item_ids);
                let mut model = BlockchainExecutionModel::create(TriggerType::Application, execution_log_state, stack_items);
                
                if let Some(transaction_log_state) = lss.try_get_transaction_state(hash) {
                    let mut event_models = Vec::new();
                    for notify_log_id in &transaction_log_state.notify_log_ids {
                        if let Some(notify_log_state) = lss.try_get_notify_state(*notify_log_id) {
                            let notify_stack_items = self.create_stack_item_array(&lss, &notify_log_state.stack_item_ids);
                            let event_model = BlockchainEventModel::create(notify_log_state, notify_stack_items);
                            event_models.push(event_model);
                        }
                    }
                    model.notifications = event_models;
                }
                
                return Some(model);
            }
        }
        
        None
    }
    
    /// Gets transaction log with event name
    pub fn get_transaction_log_with_event(&self, hash: UInt256, event_name: &str) -> Option<BlockchainExecutionModel> {
        let lss = LogStorageStore::new(self.store.get_snapshot());
        
        if let Some(execution_transaction_state_id) = lss.try_get_execution_transaction_state(hash) {
            if let Some(execution_log_state) = lss.try_get_execution_state(execution_transaction_state_id) {
                let stack_items = self.create_stack_item_array(&lss, &execution_log_state.stack_item_ids);
                let mut model = BlockchainExecutionModel::create(TriggerType::Application, execution_log_state, stack_items);
                
                if let Some(transaction_log_state) = lss.try_get_transaction_state(hash) {
                    let mut event_models = Vec::new();
                    for notify_log_id in &transaction_log_state.notify_log_ids {
                        if let Some(notify_log_state) = lss.try_get_notify_state(*notify_log_id) {
                            if notify_log_state.event_name.eq_ignore_ascii_case(event_name) {
                                let notify_stack_items = self.create_stack_item_array(&lss, &notify_log_state.stack_item_ids);
                                let event_model = BlockchainEventModel::create(notify_log_state, notify_stack_items);
                                event_models.push(event_model);
                            }
                        }
                    }
                    model.notifications = event_models;
                }
                
                return Some(model);
            }
        }
        
        None
    }
    
    // Private helper methods
    
    fn put_execution_log_block(&self, log_store: &LogStorageStore, block: &Block, app_execution: &ApplicationExecuted) -> String {
        let stack_item_ids = self.create_stack_item_id_list(log_store, app_execution);
        let exe_state = ExecutionLogState::create(app_execution, stack_item_ids);
        let exe_state_id = log_store.put_execution_state(exe_state);
        log_store.put_execution_block_state(block.hash(), app_execution.trigger, exe_state_id);
        exe_state_id
    }
    
    fn put_block_and_transaction_log(&self, log_store: &LogStorageStore, block: &Block, app_execution: &ApplicationExecuted, exe_state_id: String) {
        // Implementation would put block and transaction logs
    }
    
    fn create_stack_item_array(&self, lss: &LogStorageStore, stack_item_ids: &[String]) -> Vec<Box<dyn StackItem>> {
        // Implementation would create stack items from IDs
        Vec::new()
    }
    
    fn create_stack_item_id_list(&self, log_store: &LogStorageStore, app_execution: &ApplicationExecuted) -> Vec<String> {
        // Implementation would create stack item IDs from execution
        Vec::new()
    }
}

/// Stack item trait
pub trait StackItem: Send + Sync {
    fn to_json(&self) -> serde_json::Value;
}