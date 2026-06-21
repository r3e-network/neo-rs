//! Storage configuration helpers and shared enums.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Compression algorithms (matches C# Neo compression support)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression.
    None,
    /// LZ4 compression (fast).
    Lz4,
    /// Zstandard compression (high ratio).
    Zstd,
}

/// Compaction strategy for database optimization (matches C# Neo)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionStrategy {
    /// Level-based compaction.
    Level,
    /// Universal compaction.
    Universal,
    /// FIFO compaction.
    Fifo,
}

/// Batch operation for bulk database operations (matches C# Neo)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchOperation {
    /// Insert or update a key-value pair.
    Put {
        /// The key to insert.
        key: Vec<u8>,
        /// The value to store.
        value: Vec<u8>,
    },
    /// Delete a key.
    Delete {
        /// The key to delete.
        key: Vec<u8>,
    },
}

/// Storage configuration (matches C# Neo storage configuration, RocksDB only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Path to the database directory.
    pub path: PathBuf,
    /// Compression algorithm to use.
    pub compression_algorithm: CompressionAlgorithm,
    /// Compaction strategy.
    pub compaction_strategy: CompactionStrategy,
    /// Maximum number of open files.
    pub max_open_files: Option<u32>,
    /// Block cache size in bytes.
    pub cache_size: Option<usize>,
    /// Write buffer size in bytes.
    pub write_buffer_size: Option<usize>,
    /// Enable statistics collection.
    pub enable_statistics: bool,
    /// Open database in read-only mode.
    pub read_only: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("./data"),
            compression_algorithm: CompressionAlgorithm::Lz4,
            compaction_strategy: CompactionStrategy::Level,
            max_open_files: Some(1000),
            cache_size: Some(512 * 1024 * 1024), // 512MB block cache
            write_buffer_size: Some(256 * 1024 * 1024), // 256MB write buffer (matches build_db_options)
            enable_statistics: false,
            read_only: false,
        }
    }
}

// Re-export StorageError from neo-storage as the canonical definition.
pub use crate::StorageError;

#[cfg(test)]
#[path = "../tests/persistence/storage.rs"]
mod tests;
