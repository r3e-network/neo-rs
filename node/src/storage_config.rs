//! Storage configuration module for persistent blockchain data
//!
//! This module provides configuration and management for persistent storage
//! of blockchain data using RocksDB. It handles data directories, storage
//! options, and database configuration.

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use neo_config::NetworkType;

/// Storage configuration for the blockchain node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Base data directory path
    pub data_path: PathBuf,
    
    /// Enable compression for stored data
    pub enable_compression: bool,
    
    /// Cache size in MB for RocksDB
    pub cache_size_mb: usize,
    
    /// Write buffer size in MB
    pub write_buffer_size_mb: usize,
    
    /// Maximum number of open files
    pub max_open_files: i32,
    
    /// Enable statistics collection
    pub enable_statistics: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_path: Self::default_data_path(),
            enable_compression: true,
            cache_size_mb: 512, // 512 MB cache
            write_buffer_size_mb: 64, // 64 MB write buffer
            max_open_files: 1000,
            enable_statistics: false,
        }
    }
}

impl StorageConfig {
    /// Create a new storage configuration
    pub fn new(data_path: PathBuf) -> Self {
        Self {
            data_path,
            ..Default::default()
        }
    }
    
    /// Get the default data path
    pub fn default_data_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let base = std::env::var("NEO_DATA_PATH")
            .unwrap_or_else(|_| format!("{}/.neo-rs", home));
        PathBuf::from(base)
    }
    
    /// Get the full path for a specific network
    pub fn get_network_path(&self, network: NetworkType) -> PathBuf {
        let subdir = match network {
            NetworkType::MainNet => "mainnet",
            NetworkType::TestNet => "testnet",
            NetworkType::Private => "private",
        };
        self.data_path.join(subdir)
    }
    
    /// Create storage directories if they don't exist
    pub fn create_directories(&self, network: NetworkType) -> Result<PathBuf> {
        let network_path = self.get_network_path(network);
        
        // Create main data directory
        std::fs::create_dir_all(&network_path)
            .with_context(|| format!("Failed to create data directory: {:?}", network_path))?;
        
        // Create subdirectories for different data types
        let subdirs = ["blocks", "state", "contracts", "indexes", "logs"];
        for subdir in &subdirs {
            let path = network_path.join(subdir);
            std::fs::create_dir_all(&path)
                .with_context(|| format!("Failed to create subdirectory: {:?}", path))?;
        }
        
        Ok(network_path)
    }
    
    /// Get RocksDB options configured for blockchain storage
    pub fn get_rocksdb_options(&self) -> rocksdb::Options {
        let mut opts = rocksdb::Options::default();
        
        // Basic options
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        // Performance options
        opts.set_write_buffer_size(self.write_buffer_size_mb * 1024 * 1024);
        opts.set_max_open_files(self.max_open_files);
        opts.increase_parallelism(num_cpus::get() as i32);
        
        // Cache configuration
        if self.cache_size_mb > 0 {
            let cache = rocksdb::Cache::new_lru_cache(self.cache_size_mb * 1024 * 1024);
            opts.set_row_cache(&cache);
        }
        
        // Compression
        if self.enable_compression {
            opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        }
        
        // Statistics
        if self.enable_statistics {
            opts.enable_statistics();
        }
        
        // Optimize for SSD
        opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);
        opts.optimize_level_style_compaction(512 * 1024 * 1024); // 512MB
        
        opts
    }
    
    /// Validate storage configuration
    pub fn validate(&self) -> Result<()> {
        // Check if data path is accessible
        if !self.data_path.exists() {
            // Try to create it
            std::fs::create_dir_all(&self.data_path)
                .with_context(|| format!("Cannot create data path: {:?}", self.data_path))?;
        }
        
        // Check write permissions
        let test_file = self.data_path.join(".write_test");
        std::fs::write(&test_file, b"test")
            .with_context(|| format!("No write permission for data path: {:?}", self.data_path))?;
        std::fs::remove_file(test_file).ok();
        
        // Validate cache size
        if self.cache_size_mb > 4096 {
            tracing::warn!("Large cache size configured: {} MB. This may consume significant memory.", self.cache_size_mb);
        }
        
        Ok(())
    }
    
    /// Get storage info as a formatted string
    pub fn info(&self) -> String {
        format!(
            "Storage Configuration:\n\
             ├─ Data Path: {:?}\n\
             ├─ Compression: {}\n\
             ├─ Cache Size: {} MB\n\
             ├─ Write Buffer: {} MB\n\
             ├─ Max Open Files: {}\n\
             └─ Statistics: {}",
            self.data_path,
            if self.enable_compression { "Enabled" } else { "Disabled" },
            self.cache_size_mb,
            self.write_buffer_size_mb,
            self.max_open_files,
            if self.enable_statistics { "Enabled" } else { "Disabled" }
        )
    }
}

/// Storage statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    /// Total size of stored data in bytes
    pub total_size: u64,
    
    /// Number of blocks stored
    pub block_count: u64,
    
    /// Number of transactions stored
    pub transaction_count: u64,
    
    /// Number of contracts stored
    pub contract_count: u64,
    
    /// Database write operations per second
    pub writes_per_second: f64,
    
    /// Database read operations per second
    pub reads_per_second: f64,
}

impl StorageStats {
    /// Format statistics as a string
    pub fn format(&self) -> String {
        format!(
            "Storage Statistics:\n\
             ├─ Total Size: {:.2} GB\n\
             ├─ Blocks: {}\n\
             ├─ Transactions: {}\n\
             ├─ Contracts: {}\n\
             ├─ Write Rate: {:.1} ops/sec\n\
             └─ Read Rate: {:.1} ops/sec",
            self.total_size as f64 / (1024.0 * 1024.0 * 1024.0),
            self.block_count,
            self.transaction_count,
            self.contract_count,
            self.writes_per_second,
            self.reads_per_second
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_storage_config_default() {
        let config = StorageConfig::default();
        assert_eq!(config.cache_size_mb, 512);
        assert_eq!(config.write_buffer_size_mb, 64);
        assert!(config.enable_compression);
    }
    
    #[test]
    fn test_network_paths() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::new(temp_dir.path().to_path_buf());
        
        let mainnet_path = config.get_network_path(NetworkType::MainNet);
        assert_eq!(mainnet_path, temp_dir.path().join("mainnet"));
        
        let testnet_path = config.get_network_path(NetworkType::TestNet);
        assert_eq!(testnet_path, temp_dir.path().join("testnet"));
    }
    
    #[test]
    fn test_create_directories() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::new(temp_dir.path().to_path_buf());
        
        let network_path = config.create_directories(NetworkType::TestNet).unwrap();
        
        // Check that all subdirectories were created
        assert!(network_path.join("blocks").exists());
        assert!(network_path.join("state").exists());
        assert!(network_path.join("contracts").exists());
        assert!(network_path.join("indexes").exists());
        assert!(network_path.join("logs").exists());
    }
    
    #[test]
    fn test_validate_config() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::new(temp_dir.path().to_path_buf());
        
        // Should succeed with valid config
        assert!(config.validate().is_ok());
    }
}