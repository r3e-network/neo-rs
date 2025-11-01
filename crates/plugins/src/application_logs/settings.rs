// Copyright (C) 2015-2025 The Neo Project.
//
// settings.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

static DEFAULT_SETTINGS: Lazy<RwLock<ApplicationLogsSettings>> =
    Lazy::new(|| RwLock::new(ApplicationLogsSettings::default()));

/// Application Logs settings matching C# ApplicationLogsSettings exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationLogsSettings {
    /// Path template for log storage
    pub path: String,
    /// Network ID
    pub network: u32,
    /// Maximum stack size
    pub max_stack_size: i32,
    /// Debug mode
    pub debug: bool,
    /// Exception policy
    pub exception_policy: UnhandledExceptionPolicy,
}

/// Unhandled exception policy matching C# enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UnhandledExceptionPolicy {
    /// Stop the node
    StopNode,
    /// Ignore the exception
    Ignore,
}

impl ApplicationLogsSettings {
    /// Loads settings from configuration
    /// Matches C# Load method
    pub fn load(config: &serde_json::Value) {
        let settings = ApplicationLogsSettings::from_config(config);
        if let Ok(mut guard) = DEFAULT_SETTINGS.write() {
            *guard = settings;
        }
    }

    /// Gets the default settings
    /// Matches C# Default property
    pub fn default() -> ApplicationLogsSettings {
        DEFAULT_SETTINGS
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Creates settings from configuration
    /// Matches C# constructor with IConfigurationSection
    pub fn from_config(config: &serde_json::Value) -> Self {
        Self {
            path: config
                .get("Path")
                .and_then(|v| v.as_str())
                .unwrap_or("ApplicationLogs_{0}")
                .to_string(),
            network: config
                .get("Network")
                .and_then(|v| v.as_u64())
                .unwrap_or(5195086) as u32,
            max_stack_size: config
                .get("MaxStackSize")
                .and_then(|v| v.as_i64())
                .unwrap_or(65535) as i32,
            debug: config
                .get("Debug")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            exception_policy: config
                .get("UnhandledExceptionPolicy")
                .and_then(|v| v.as_str())
                .and_then(|s| match s {
                    "StopNode" => Some(UnhandledExceptionPolicy::StopNode),
                    "Ignore" => Some(UnhandledExceptionPolicy::Ignore),
                    _ => None,
                })
                .unwrap_or(UnhandledExceptionPolicy::Ignore),
        }
    }
}

impl Default for ApplicationLogsSettings {
    fn default() -> Self {
        Self {
            path: "ApplicationLogs_{0}".to_string(),
            network: 5_195_086,
            max_stack_size: 65_535,
            debug: false,
            exception_policy: UnhandledExceptionPolicy::Ignore,
        }
    }
}
