//! Storage Dumper Settings
//!
//! Configuration settings for Storage Dumper plugin

use serde::{Deserialize, Serialize};

/// Settings for Storage Dumper plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDumperSettings {
    /// Output directory for dumped data
    pub output_directory: String,
    /// Enable compression for dumped files
    pub enable_compression: bool,
    /// Maximum file size before splitting
    pub max_file_size: usize,
    /// Include metadata in dump
    pub include_metadata: bool,
}

impl Default for StorageDumperSettings {
    fn default() -> Self {
        Self {
            output_directory: "./dump".to_string(),
            enable_compression: true,
            max_file_size: 100 * 1024 * 1024, // 100MB
            include_metadata: true,
        }
    }
}
