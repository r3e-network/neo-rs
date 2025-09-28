//! LevelDB Store Settings
//!
//! Configuration settings for LevelDB Store plugin

use serde::{Deserialize, Serialize};

/// Settings for LevelDB Store plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelDBStoreSettings {
    /// Path to the LevelDB database
    pub path: String,
    /// Cache size in bytes
    pub cache_size: usize,
    /// Write buffer size in bytes
    pub write_buffer_size: usize,
    /// Maximum number of open files
    pub max_open_files: i32,
}

impl Default for LevelDBStoreSettings {
    fn default() -> Self {
        Self {
            path: "./data".to_string(),
            cache_size: 8 * 1024 * 1024, // 8MB
            write_buffer_size: 4 * 1024 * 1024, // 4MB
            max_open_files: 1000,
        }
    }
}
