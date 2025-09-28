// Copyright (C) 2015-2025 The Neo Project.
//
// state_service_settings.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Unhandled exception policy enumeration.
/// Matches C# UnhandledExceptionPolicy enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnhandledExceptionPolicy {
    /// Stop the plugin when an unhandled exception occurs
    StopPlugin,
    /// Continue running the plugin when an unhandled exception occurs
    Continue,
}

/// State service settings implementation.
/// Matches C# StateServiceSettings class exactly
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateServiceSettings {
    /// The path for storing state data.
    /// Matches C# Path property
    pub path: String,
    
    /// Whether to store full state.
    /// Matches C# FullState property
    pub full_state: bool,
    
    /// The network identifier.
    /// Matches C# Network property
    pub network: u32,
    
    /// Whether to auto-verify state.
    /// Matches C# AutoVerify property
    pub auto_verify: bool,
    
    /// Maximum number of find result items.
    /// Matches C# MaxFindResultItems property
    pub max_find_result_items: i32,
    
    /// Exception policy for unhandled exceptions.
    /// Matches C# ExceptionPolicy property
    pub exception_policy: UnhandledExceptionPolicy,
}

impl StateServiceSettings {
    /// The default state service settings.
    /// Matches C# Default property
    pub fn default() -> Self {
        Self {
            path: "Data_MPT_{0}".to_string(),
            full_state: false,
            network: 860833102,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: UnhandledExceptionPolicy::StopPlugin,
        }
    }
    
    /// Loads settings from configuration.
    /// Matches C# Load method
    pub fn load(config: &serde_json::Value) -> Self {
        let plugin_config = &config["PluginConfiguration"];
        
        Self {
            path: plugin_config["Path"]
                .as_str()
                .unwrap_or("Data_MPT_{0}")
                .to_string(),
            full_state: plugin_config["FullState"]
                .as_bool()
                .unwrap_or(false),
            network: plugin_config["Network"]
                .as_u64()
                .unwrap_or(860833102) as u32,
            auto_verify: plugin_config["AutoVerify"]
                .as_bool()
                .unwrap_or(false),
            max_find_result_items: plugin_config["MaxFindResultItems"]
                .as_i64()
                .unwrap_or(100) as i32,
            exception_policy: plugin_config["UnhandledExceptionPolicy"]
                .as_str()
                .unwrap_or("StopPlugin")
                .parse()
                .unwrap_or(UnhandledExceptionPolicy::StopPlugin),
        }
    }
    
    /// Gets the path.
    pub fn path(&self) -> &str {
        &self.path
    }
    
    /// Gets the full state setting.
    pub fn full_state(&self) -> bool {
        self.full_state
    }
    
    /// Gets the network identifier.
    pub fn network(&self) -> u32 {
        self.network
    }
    
    /// Gets the auto-verify setting.
    pub fn auto_verify(&self) -> bool {
        self.auto_verify
    }
    
    /// Gets the maximum find result items.
    pub fn max_find_result_items(&self) -> i32 {
        self.max_find_result_items
    }
    
    /// Gets the exception policy.
    pub fn exception_policy(&self) -> &str {
        match self.exception_policy {
            UnhandledExceptionPolicy::StopPlugin => "StopPlugin",
            UnhandledExceptionPolicy::Continue => "Continue",
        }
    }
}

impl Default for StateServiceSettings {
    fn default() -> Self {
        Self::default()
    }
}

impl std::str::FromStr for UnhandledExceptionPolicy {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "StopPlugin" => Ok(UnhandledExceptionPolicy::StopPlugin),
            "Continue" => Ok(UnhandledExceptionPolicy::Continue),
            _ => Err(format!("Invalid UnhandledExceptionPolicy: {}", s)),
        }
    }
}