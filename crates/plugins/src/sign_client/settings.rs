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

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Sign settings matching C# SignSettings exactly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignSettings {
    /// The name of the sign client
    /// Matches C# Name property
    pub name: String,
    
    /// The endpoint of the sign client
    /// Matches C# Endpoint property
    pub endpoint: String,
    
    /// Exception policy
    /// Matches C# ExceptionPolicy property
    pub exception_policy: UnhandledExceptionPolicy,
}

/// Unhandled exception policy matching C# UnhandledExceptionPolicy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnhandledExceptionPolicy {
    Ignore,
    Log,
    Throw,
}

impl Default for UnhandledExceptionPolicy {
    fn default() -> Self {
        UnhandledExceptionPolicy::Ignore
    }
}

impl SignSettings {
    /// Section name constant
    /// Matches C# SectionName constant
    pub const SECTION_NAME: &'static str = "PluginConfiguration";
    
    /// Default endpoint constant
    /// Matches C# DefaultEndpoint constant
    pub const DEFAULT_ENDPOINT: &'static str = "http://127.0.0.1:9991";
    
    /// Creates a new SignSettings from configuration
    /// Matches C# constructor with IConfigurationSection
    pub fn from_config(config: &serde_json::Value) -> Self {
        let name = config.get("Name")
            .and_then(|v| v.as_str())
            .unwrap_or("SignClient")
            .to_string();
        
        let endpoint = config.get("Endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or(Self::DEFAULT_ENDPOINT)
            .to_string();
        
        let exception_policy = config.get("UnhandledExceptionPolicy")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "Ignore" => Some(UnhandledExceptionPolicy::Ignore),
                "Log" => Some(UnhandledExceptionPolicy::Log),
                "Throw" => Some(UnhandledExceptionPolicy::Throw),
                _ => None,
            })
            .unwrap_or(UnhandledExceptionPolicy::Ignore);
        
        let settings = Self {
            name,
            endpoint,
            exception_policy,
        };
        
        // Validate endpoint
        let _ = settings.get_vsock_address();
        
        settings
    }
    
    /// Gets the default settings
    /// Matches C# Default property
    pub fn default() -> Self {
        let mut config = HashMap::new();
        config.insert("Name".to_string(), "SignClient".to_string());
        config.insert("Endpoint".to_string(), Self::DEFAULT_ENDPOINT.to_string());
        
        let config_value = serde_json::to_value(config).unwrap();
        Self::from_config(&config_value)
    }
    
    /// Gets the vsock address from the endpoint
    /// Matches C# GetVsockAddress method
    pub fn get_vsock_address(&self) -> Option<VsockAddress> {
        if let Ok(uri) = url::Url::parse(&self.endpoint) {
            if uri.scheme() == "vsock" {
                if let Ok(context_id) = uri.host_str().unwrap_or("").parse::<i32>() {
                    return Some(VsockAddress {
                        context_id,
                        port: uri.port().unwrap_or(0) as i32,
                    });
                } else {
                    panic!("Invalid vsock endpoint: {}", self.endpoint);
                }
            }
        }
        None
    }
}

/// Vsock address matching C# VsockAddress
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsockAddress {
    pub context_id: i32,
    pub port: i32,
}

impl Default for SignSettings {
    fn default() -> Self {
        Self::default()
    }
}