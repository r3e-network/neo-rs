// Copyright (C) 2015-2025 The Neo Project.
//
// dbft_settings.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::plugins::UnhandledExceptionPolicy;
use serde::{Deserialize, Serialize};

/// DBFT Plugin settings matching C# DbftSettings exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbftSettings {
    /// Recovery logs name
    pub recovery_logs: String,

    /// Whether to ignore recovery logs
    pub ignore_recovery_logs: bool,

    /// Whether to auto start consensus
    pub auto_start: bool,

    /// Network ID
    pub network: u32,

    /// Maximum block size
    pub max_block_size: u32,

    /// Maximum block system fee
    pub max_block_system_fee: i64,

    /// Exception policy
    pub exception_policy: UnhandledExceptionPolicy,
}

impl Default for DbftSettings {
    fn default() -> Self {
        Self {
            recovery_logs: "ConsensusState".to_string(),
            ignore_recovery_logs: false,
            auto_start: false,
            network: 5195086u32,
            max_block_system_fee: 150000000000i64,
            max_block_size: 262144u32,
            exception_policy: UnhandledExceptionPolicy::StopNode,
        }
    }
}

impl DbftSettings {
    /// Creates new DbftSettings with default values
    /// Matches C# default constructor
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates DbftSettings from configuration section
    /// Matches C# constructor with IConfigurationSection parameter
    pub fn from_config(config: &serde_json::Value) -> Self {
        Self {
            recovery_logs: config
                .get("RecoveryLogs")
                .and_then(|v| v.as_str())
                .unwrap_or("ConsensusState")
                .to_string(),
            ignore_recovery_logs: config
                .get("IgnoreRecoveryLogs")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            auto_start: config
                .get("AutoStart")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            network: config
                .get("Network")
                .and_then(|v| v.as_u64())
                .unwrap_or(5195086) as u32,
            max_block_size: config
                .get("MaxBlockSize")
                .and_then(|v| v.as_u64())
                .unwrap_or(262144) as u32,
            max_block_system_fee: config
                .get("MaxBlockSystemFee")
                .and_then(|v| v.as_i64())
                .unwrap_or(150000000000),
            exception_policy: config
                .get("UnhandledExceptionPolicy")
                .and_then(|v| v.as_str())
                .and_then(|s| match s {
                    "StopNode" => Some(UnhandledExceptionPolicy::StopNode),
                    "Ignore" => Some(UnhandledExceptionPolicy::Ignore),
                    _ => None,
                })
                .unwrap_or(UnhandledExceptionPolicy::StopNode),
        }
    }
}
