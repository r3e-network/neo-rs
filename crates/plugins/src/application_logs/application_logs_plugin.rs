//! Application Logs Plugin - Main plugin implementation
//!
//! This module provides the main Application Logs plugin that implements
//! application logging functionality for the Neo blockchain.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::settings::ApplicationLogsSettings;
use super::log_reader::LogReader;

/// Plugin information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub author: String,
}

/// Application Logs Plugin implementation (matches C# ApplicationLogsPlugin)
pub struct ApplicationLogsPlugin {
    /// Plugin information
    pub info: PluginInfo,
    /// Plugin settings
    pub settings: ApplicationLogsSettings,
    /// Log reader
    pub log_reader: Option<Arc<LogReader>>,
}

impl ApplicationLogsPlugin {
    /// Creates a new Application Logs plugin instance
    pub fn new(settings: ApplicationLogsSettings) -> Self {
        Self {
            info: PluginInfo {
                name: "ApplicationLogsPlugin".to_string(),
                version: "1.0.0".to_string(),
                description: "Application logging plugin for Neo blockchain".to_string(),
                category: "Utility".to_string(),
                author: "Neo Project".to_string(),
            },
            settings,
            log_reader: None,
        }
    }
}

impl ApplicationLogsPlugin {
    /// Initialize the plugin
    pub fn initialize(&mut self) -> Result<(), String> {
        // Initialize log reader
        let log_reader = Arc::new(LogReader::new(
            self.settings.clone(),
        )?);
        
        self.log_reader = Some(log_reader);
        
        Ok(())
    }

    /// Shutdown the plugin
    pub fn shutdown(&mut self) -> Result<(), String> {
        // Stop log reader
        self.log_reader = None;
        Ok(())
    }
}
