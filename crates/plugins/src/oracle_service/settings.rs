//! Oracle Service Plugin Settings
//!
//! This module provides configuration settings for the Oracle Service plugin,
//! matching the C# OracleServiceSettings exactly.

use serde::{Deserialize, Serialize};

/// Oracle Service Plugin settings (matches C# OracleServiceSettings)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleServiceSettings {
    /// Enable oracle service
    pub enabled: bool,
    /// Oracle nodes
    pub nodes: Vec<String>,
    /// Request timeout in milliseconds
    pub timeout: u64,
    /// Maximum concurrent requests
    pub max_concurrent_requests: u32,
    /// Cache timeout in seconds
    pub cache_timeout: u64,
    /// Enable caching
    pub enable_cache: bool,
}

impl Default for OracleServiceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            nodes: vec![
                "https://api.neoline.io".to_string(),
                "https://api.neotracker.io".to_string(),
            ],
            timeout: 30000, // 30 seconds
            max_concurrent_requests: 10,
            cache_timeout: 300, // 5 minutes
            enable_cache: true,
        }
    }
}

impl OracleServiceSettings {
    /// Creates new Oracle Service settings with default values
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Creates new Oracle Service settings for mainnet
    pub fn mainnet() -> Self {
        Self {
            enabled: true,
            nodes: vec![
                "https://api.neoline.io".to_string(),
                "https://api.neotracker.io".to_string(),
            ],
            timeout: 30000,
            max_concurrent_requests: 20,
            cache_timeout: 600, // 10 minutes
            enable_cache: true,
        }
    }
    
    /// Creates new Oracle Service settings for testnet
    pub fn testnet() -> Self {
        Self {
            enabled: true,
            nodes: vec![
                "https://testnet-api.neoline.io".to_string(),
                "https://testnet-api.neotracker.io".to_string(),
            ],
            timeout: 15000, // Faster timeout for testnet
            max_concurrent_requests: 5,
            cache_timeout: 60, // 1 minute
            enable_cache: true,
        }
    }
}
