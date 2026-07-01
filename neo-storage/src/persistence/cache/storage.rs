//! Storage configuration helpers and shared enums.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Compression algorithm used by persistent backends that support value
/// compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression.
    None,
    /// LZ4 compression (fast).
    Lz4,
    /// Zstandard compression (high ratio).
    Zstd,
}

/// Compaction strategy used by LSM-style backends such as RocksDB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionStrategy {
    /// Level-based compaction.
    Level,
    /// Universal compaction.
    Universal,
    /// FIFO compaction.
    Fifo,
}

/// Provider-neutral storage configuration shared by persistent store
/// providers.
///
/// Backends own their engine-specific defaults. Callers should set only the
/// fields they intentionally tune.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Path to the database directory.
    pub path: PathBuf,
    /// Compression algorithm for backends that implement inline compression.
    ///
    /// MDBX stores Neo bytes directly and ignores this field; cold static-file
    /// compression belongs in the static-file layer, not this hot store config.
    pub compression_algorithm: CompressionAlgorithm,
    /// Compaction strategy for backends that compact SST/LSM files.
    ///
    /// MDBX is B+tree/MVCC and ignores this field.
    pub compaction_strategy: CompactionStrategy,
    /// Maximum number of open files.
    pub max_open_files: Option<u32>,
    /// Block cache size in bytes.
    pub cache_size: Option<usize>,
    /// Write buffer size in bytes.
    pub write_buffer_size: Option<usize>,
    /// MDBX maximum geometry size in bytes.
    pub mdbx_geometry_upper_bytes: Option<isize>,
    /// MDBX geometry growth step in bytes.
    pub mdbx_geometry_growth_bytes: Option<isize>,
    /// Maximum number of concurrent MDBX readers.
    pub mdbx_max_readers: Option<u32>,
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
            max_open_files: None,
            cache_size: None,
            write_buffer_size: None,
            mdbx_geometry_upper_bytes: None,
            mdbx_geometry_growth_bytes: None,
            mdbx_max_readers: None,
            enable_statistics: false,
            read_only: false,
        }
    }
}

// Re-export StorageError from neo-storage as the canonical definition.
pub use crate::StorageError;

#[cfg(test)]
#[path = "../../tests/persistence/storage.rs"]
mod tests;
