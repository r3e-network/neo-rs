//! Storage Dumper implementation
//!
//! Provides storage dumping functionality matching C# Neo.StorageDumper

use super::settings::StorageDumperSettings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Plugin information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub author: String,
}

/// Storage dump entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDumpEntry {
    pub key: String,
    pub value: Vec<u8>,
    pub timestamp: u64,
}

/// Storage Dumper plugin for dumping blockchain storage data
pub struct StorageDumper {
    pub info: PluginInfo,
    pub settings: StorageDumperSettings,
    pub is_initialized: bool,
    pub dump_data: Arc<RwLock<HashMap<String, StorageDumpEntry>>>,
}

impl StorageDumper {
    /// Create a new Storage Dumper instance
    pub fn new(settings: StorageDumperSettings) -> Self {
        Self {
            info: PluginInfo {
                name: "StorageDumper".to_string(),
                version: "1.0.0".to_string(),
                description: "Storage dumping functionality for Neo blockchain data".to_string(),
                category: "Utility".to_string(),
                author: "Neo Project".to_string(),
            },
            settings,
            is_initialized: false,
            dump_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize the Storage Dumper
    pub fn initialize(&mut self) -> Result<(), String> {
        if self.is_initialized {
            return Ok(());
        }

        // Create output directory if it doesn't exist
        let output_path = PathBuf::from(&self.settings.output_directory);
        if !output_path.exists() {
            std::fs::create_dir_all(&output_path).map_err(|e| e.to_string())?;
        }

        self.is_initialized = true;
        Ok(())
    }

    /// Shutdown the Storage Dumper
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.is_initialized = false;
        Ok(())
    }

    /// Add data to dump
    pub async fn add_data(&self, key: &str, value: &[u8]) -> Result<(), String> {
        if !self.is_initialized {
            return Err("Storage Dumper not initialized".to_string());
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let entry = StorageDumpEntry {
            key: key.to_string(),
            value: value.to_vec(),
            timestamp: now,
        };

        let mut dump_data = self.dump_data.write().await;
        dump_data.insert(key.to_string(), entry);

        Ok(())
    }

    /// Dump all data to files
    pub async fn dump_to_files(&self) -> Result<(), String> {
        if !self.is_initialized {
            return Err("Storage Dumper not initialized".to_string());
        }

        let dump_data = self.dump_data.read().await;
        let output_path = PathBuf::from(&self.settings.output_directory);

        // Create dump file
        let dump_file = output_path.join("storage_dump.json");
        let json_data = serde_json::to_string_pretty(&*dump_data).map_err(|e| e.to_string())?;
        
        std::fs::write(&dump_file, json_data).map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Get dump statistics
    pub async fn get_dump_stats(&self) -> (usize, usize) {
        let dump_data = self.dump_data.read().await;
        let count = dump_data.len();
        let total_size = dump_data.values().map(|entry| entry.value.len()).sum();
        (count, total_size)
    }

    /// Clear dump data
    pub async fn clear_dump_data(&self) -> Result<(), String> {
        let mut dump_data = self.dump_data.write().await;
        dump_data.clear();
        Ok(())
    }
}
