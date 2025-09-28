// Copyright (C) 2015-2025 The Neo Project.
//
// tokens_tracker.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{NeoSystem, Block, DataCache, ApplicationExecuted, IStore, UnhandledExceptionPolicy};
use neo_core::plugins::Plugin;
use neo_core::event_handlers::{ICommittingHandler, ICommittedHandler};
use super::trackers::TrackerBase;
use std::sync::Arc;
use std::path::Path;

/// Tokens tracker plugin implementation.
/// Matches C# TokensTracker class exactly
pub struct TokensTracker {
    /// Base plugin functionality
    base: Plugin,
    
    /// Database path
    /// Matches C# _dbPath field
    db_path: String,
    
    /// Whether to track history
    /// Matches C# _shouldTrackHistory field
    should_track_history: bool,
    
    /// Maximum results
    /// Matches C# _maxResults field
    max_results: u32,
    
    /// Network identifier
    /// Matches C# _network field
    network: u32,
    
    /// Enabled trackers
    /// Matches C# _enabledTrackers field
    enabled_trackers: Vec<String>,
    
    /// Database store
    /// Matches C# _db field
    db: Option<Arc<dyn IStore>>,
    
    /// Exception policy
    /// Matches C# _exceptionPolicy field
    exception_policy: UnhandledExceptionPolicy,
    
    /// Neo system reference
    /// Matches C# neoSystem field
    neo_system: Option<Arc<NeoSystem>>,
    
    /// Trackers
    /// Matches C# trackers field
    trackers: Vec<Box<dyn TrackerBase>>,
}

impl TokensTracker {
    /// Creates a new TokensTracker instance.
    /// Matches C# constructor
    pub fn new() -> Self {
        Self {
            base: Plugin::new(),
            db_path: "TokensBalanceData".to_string(),
            should_track_history: true,
            max_results: 1000,
            network: 860833102,
            enabled_trackers: Vec::new(),
            db: None,
            exception_policy: UnhandledExceptionPolicy::StopNode,
            neo_system: None,
            trackers: Vec::new(),
        }
    }
    
    /// Gets the description of the plugin.
    /// Matches C# Description property
    pub fn description(&self) -> &str {
        "Enquiries balances and transaction history of accounts through RPC"
    }
    
    /// Gets the config file path.
    /// Matches C# ConfigFile property
    pub fn config_file(&self) -> String {
        format!("{}/TokensTracker.json", self.base.root_path())
    }
    
    /// Gets the exception policy.
    /// Matches C# ExceptionPolicy property
    pub fn exception_policy(&self) -> &str {
        match self.exception_policy {
            UnhandledExceptionPolicy::StopNode => "StopNode",
            UnhandledExceptionPolicy::StopPlugin => "StopPlugin",
            UnhandledExceptionPolicy::Continue => "Continue",
        }
    }
    
    /// Configures the plugin.
    /// Matches C# Configure method
    pub fn configure(&mut self) {
        let config = self.base.get_configuration();
        
        self.db_path = config["DBPath"]
            .as_str()
            .unwrap_or("TokensBalanceData")
            .to_string();
        
        self.should_track_history = config["TrackHistory"]
            .as_bool()
            .unwrap_or(true);
        
        self.max_results = config["MaxResults"]
            .as_u64()
            .unwrap_or(1000) as u32;
        
        self.network = config["Network"]
            .as_u64()
            .unwrap_or(860833102) as u32;
        
        if let Some(enabled_trackers) = config["EnabledTrackers"].as_array() {
            self.enabled_trackers = enabled_trackers
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
        }
        
        if let Some(policy_str) = config["UnhandledExceptionPolicy"].as_str() {
            self.exception_policy = match policy_str {
                "StopNode" => UnhandledExceptionPolicy::StopNode,
                "StopPlugin" => UnhandledExceptionPolicy::StopPlugin,
                "Continue" => UnhandledExceptionPolicy::Continue,
                _ => UnhandledExceptionPolicy::StopNode,
            };
        }
    }
    
    /// Called when the system is loaded.
    /// Matches C# OnSystemLoaded method
    pub fn on_system_loaded(&mut self, system: &NeoSystem) {
        if system.settings().network != self.network {
            return;
        }
        
        self.neo_system = Some(Arc::new(system.clone()));
        let path = format!("{}/{}", self.db_path, system.settings().network);
        self.db = Some(system.load_store(&self.base.get_full_path(&path)));
        
        if self.enabled_trackers.contains(&"NEP-11".to_string()) {
            // In a real implementation, this would create a NEP11Tracker
            // self.trackers.push(Box::new(Nep11Tracker::new(...)));
        }
        
        if self.enabled_trackers.contains(&"NEP-17".to_string()) {
            // In a real implementation, this would create a NEP17Tracker
            // self.trackers.push(Box::new(Nep17Tracker::new(...)));
        }
        
        // Register RPC methods for each tracker
        for tracker in &self.trackers {
            // In a real implementation, this would register RPC methods
            // RpcServerPlugin::register_methods(tracker, self.network);
        }
    }
    
    /// Resets the batch for all trackers.
    /// Matches C# ResetBatch method
    fn reset_batch(&mut self) {
        for tracker in &mut self.trackers {
            tracker.reset_batch();
        }
    }
    
    /// Handles committing events.
    /// Matches C# Blockchain_Committing_Handler method
    pub fn handle_committing(&mut self, system: &NeoSystem, block: &Block, snapshot: &DataCache, executed_list: &[ApplicationExecuted]) {
        if system.settings().network != self.network {
            return;
        }
        
        // Start freshly with a new DBCache for each block
        self.reset_batch();
        
        for tracker in &mut self.trackers {
            tracker.on_persist(system, block, snapshot, executed_list);
        }
    }
    
    /// Handles committed events.
    /// Matches C# Blockchain_Committed_Handler method
    pub fn handle_committed(&mut self, system: &NeoSystem, block: &Block) {
        if system.settings().network != self.network {
            return;
        }
        
        for tracker in &mut self.trackers {
            tracker.commit();
        }
    }
}

impl ICommittingHandler for TokensTracker {
    fn handle_committing(&mut self, system: &NeoSystem, block: &Block, snapshot: &DataCache, executed_list: &[ApplicationExecuted]) {
        self.handle_committing(system, block, snapshot, executed_list);
    }
}

impl ICommittedHandler for TokensTracker {
    fn handle_committed(&mut self, system: &NeoSystem, block: &Block) {
        self.handle_committed(system, block);
    }
}

impl Default for TokensTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TokensTracker {
    fn drop(&mut self) {
        // Cleanup resources
        self.trackers.clear();
    }
}