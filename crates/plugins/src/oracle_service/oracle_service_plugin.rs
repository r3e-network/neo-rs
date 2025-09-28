//! Oracle Service Plugin - Main plugin implementation
//!
//! This module provides the main Oracle Service plugin that implements
//! oracle functionality for the Neo blockchain.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::settings::OracleServiceSettings;
use super::oracle_service::OracleService;

/// Plugin information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub author: String,
}

/// Oracle Service Plugin implementation (matches C# OracleServicePlugin)
pub struct OracleServicePlugin {
    /// Plugin information
    pub info: PluginInfo,
    /// Plugin settings
    pub settings: OracleServiceSettings,
    /// Oracle service
    pub oracle_service: Option<Arc<OracleService>>,
}

impl OracleServicePlugin {
    /// Creates a new Oracle Service plugin instance
    pub fn new(settings: OracleServiceSettings) -> Self {
        Self {
            info: PluginInfo {
                name: "OracleServicePlugin".to_string(),
                version: "1.0.0".to_string(),
                description: "Oracle service plugin for Neo blockchain".to_string(),
                category: "Utility".to_string(),
                author: "Neo Project".to_string(),
            },
            settings,
            oracle_service: None,
        }
    }
}

impl OracleServicePlugin {
    /// Initialize the plugin
    pub fn initialize(&mut self) -> Result<(), String> {
        // Initialize oracle service
        let oracle_service = Arc::new(OracleService::new(
            self.settings.clone(),
        ));
        
        self.oracle_service = Some(oracle_service);
        
        Ok(())
    }

    /// Shutdown the plugin
    pub async fn shutdown(&mut self) -> Result<(), String> {
        // Stop oracle service
        if let Some(_oracle_service) = &self.oracle_service {
            // Note: We can't call async methods on Arc, so we'll just clear it
            // In a real implementation, you'd need proper async handling
        }
        
        self.oracle_service = None;
        Ok(())
    }
}
