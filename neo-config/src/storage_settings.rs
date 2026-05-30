//! Storage backend (RocksDB) configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSettings {
    /// Path to data directory
    #[serde(default = "default_data_path")]
    pub path: PathBuf,

    /// `RocksDB` cache size in MB
    #[serde(default = "default_cache_size")]
    pub cache_size_mb: usize,

    /// Maximum open files for `RocksDB`
    #[serde(default = "default_max_open_files")]
    pub max_open_files: i32,

    /// Enable compression
    #[serde(default = "default_compression")]
    pub compression: bool,
}

fn default_data_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("neo-rs")
}

const fn default_cache_size() -> usize {
    256
}

const fn default_max_open_files() -> i32 {
    1000
}

const fn default_compression() -> bool {
    true
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            path: default_data_path(),
            cache_size_mb: default_cache_size(),
            max_open_files: default_max_open_files(),
            compression: default_compression(),
        }
    }
}
