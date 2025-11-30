//! Storage configuration helpers and shared enums.

use crate::error::{CoreError, CoreResult};
use crate::neo_config::MAX_SCRIPT_SIZE;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Compression algorithms (matches C# Neo compression support)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    None,
    Lz4,
    Zstd,
}

/// Compaction strategy for database optimization (matches C# Neo)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionStrategy {
    Level,
    Universal,
    Fifo,
}

/// Batch operation for bulk database operations (matches C# Neo)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchOperation {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

/// Storage configuration (matches C# Neo storage configuration, RocksDB only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub path: PathBuf,
    pub compression_algorithm: CompressionAlgorithm,
    pub compaction_strategy: CompactionStrategy,
    pub max_open_files: Option<u32>,
    pub cache_size: Option<usize>,
    pub write_buffer_size: Option<usize>,
    pub enable_statistics: bool,
    pub read_only: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("./data"),
            compression_algorithm: CompressionAlgorithm::Lz4,
            compaction_strategy: CompactionStrategy::Level,
            max_open_files: Some(1000),
            cache_size: Some(64 * MAX_SCRIPT_SIZE * MAX_SCRIPT_SIZE),
            write_buffer_size: Some(16 * MAX_SCRIPT_SIZE * MAX_SCRIPT_SIZE),
            enable_statistics: false,
            read_only: false,
        }
    }
}

/// Convenience alias for storage-related results.
pub type StorageResult<T> = CoreResult<T>;

/// Error type used by cache operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("key not found")]
    NotFound,
    #[error("cache is read only")]
    ReadOnly,
    #[error("{0}")]
    Other(String),
}

impl From<StorageError> for CoreError {
    fn from(err: StorageError) -> Self {
        CoreError::InvalidOperation {
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{StorageItem, StorageKey};

    // ============================================================================
    // CompressionAlgorithm Tests
    // ============================================================================

    #[test]
    fn compression_algorithm_equality() {
        assert_eq!(CompressionAlgorithm::None, CompressionAlgorithm::None);
        assert_eq!(CompressionAlgorithm::Lz4, CompressionAlgorithm::Lz4);
        assert_eq!(CompressionAlgorithm::Zstd, CompressionAlgorithm::Zstd);
        assert_ne!(CompressionAlgorithm::None, CompressionAlgorithm::Lz4);
    }

    #[test]
    fn compression_algorithm_clone() {
        let algo = CompressionAlgorithm::Zstd;
        let cloned = algo;
        assert_eq!(algo, cloned);
    }

    // ============================================================================
    // CompactionStrategy Tests
    // ============================================================================

    #[test]
    fn compaction_strategy_equality() {
        assert_eq!(CompactionStrategy::Level, CompactionStrategy::Level);
        assert_eq!(CompactionStrategy::Universal, CompactionStrategy::Universal);
        assert_eq!(CompactionStrategy::Fifo, CompactionStrategy::Fifo);
        assert_ne!(CompactionStrategy::Level, CompactionStrategy::Fifo);
    }

    // ============================================================================
    // BatchOperation Tests
    // ============================================================================

    #[test]
    fn batch_operation_put_equality() {
        let op1 = BatchOperation::Put {
            key: vec![1, 2, 3],
            value: vec![4, 5, 6],
        };
        let op2 = BatchOperation::Put {
            key: vec![1, 2, 3],
            value: vec![4, 5, 6],
        };
        assert_eq!(op1, op2);
    }

    #[test]
    fn batch_operation_delete_equality() {
        let op1 = BatchOperation::Delete { key: vec![1, 2, 3] };
        let op2 = BatchOperation::Delete { key: vec![1, 2, 3] };
        assert_eq!(op1, op2);
    }

    #[test]
    fn batch_operation_different_types_not_equal() {
        let put = BatchOperation::Put {
            key: vec![1],
            value: vec![2],
        };
        let delete = BatchOperation::Delete { key: vec![1] };
        assert_ne!(put, delete);
    }

    // ============================================================================
    // StorageConfig Tests
    // ============================================================================

    #[test]
    fn storage_config_default_values() {
        let config = StorageConfig::default();
        assert_eq!(config.path, PathBuf::from("./data"));
        assert_eq!(config.compression_algorithm, CompressionAlgorithm::Lz4);
        assert_eq!(config.compaction_strategy, CompactionStrategy::Level);
        assert_eq!(config.max_open_files, Some(1000));
        assert!(!config.enable_statistics);
        assert!(!config.read_only);
    }

    #[test]
    fn storage_config_clone() {
        let config = StorageConfig::default();
        let cloned = config.clone();
        assert_eq!(config.path, cloned.path);
        assert_eq!(config.compression_algorithm, cloned.compression_algorithm);
    }

    // ============================================================================
    // StorageKey Tests
    // ============================================================================

    #[test]
    fn storage_key_new_creates_key() {
        let key = StorageKey::new(42, vec![1, 2, 3]);
        assert_eq!(key.id, 42);
        assert_eq!(key.suffix(), &[1, 2, 3]);
    }

    #[test]
    fn storage_key_to_bytes_and_from_bytes_roundtrip() {
        let original = StorageKey::new(12345, vec![0xAB, 0xCD, 0xEF]);
        let bytes = original.to_array();
        let restored = StorageKey::from_bytes(&bytes);

        assert_eq!(original.id, restored.id);
        assert_eq!(original.suffix(), restored.suffix());
    }

    #[test]
    fn storage_key_from_bytes_exact_four_bytes() {
        let bytes = vec![1, 0, 0, 0]; // id = 1 in little-endian
        let key = StorageKey::from_bytes(&bytes);
        assert_eq!(key.id, 1);
        assert!(key.suffix().is_empty());
    }

    #[test]
    fn storage_key_equality() {
        let key1 = StorageKey::new(1, vec![1, 2, 3]);
        let key2 = StorageKey::new(1, vec![1, 2, 3]);
        let key3 = StorageKey::new(2, vec![1, 2, 3]);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn storage_key_hash_consistency() {
        use std::collections::HashSet;

        let key1 = StorageKey::new(1, vec![1, 2, 3]);
        let key2 = StorageKey::new(1, vec![1, 2, 3]);

        let mut set = HashSet::new();
        set.insert(key1);
        assert!(set.contains(&key2));
    }

    // ============================================================================
    // StorageItem Tests
    // ============================================================================

    #[test]
    fn storage_item_new_creates_item() {
        let item = StorageItem::from_bytes(vec![1, 2, 3]);
        assert_eq!(item.get_value(), vec![1, 2, 3]);
    }

    #[test]
    fn storage_item_default_is_empty() {
        let item = StorageItem::new();
        assert!(item.get_value().is_empty());
    }

    // ============================================================================
    // StorageError Tests
    // ============================================================================

    #[test]
    fn storage_error_not_found_display() {
        let err = StorageError::NotFound;
        assert_eq!(format!("{}", err), "key not found");
    }

    #[test]
    fn storage_error_read_only_display() {
        let err = StorageError::ReadOnly;
        assert_eq!(format!("{}", err), "cache is read only");
    }

    #[test]
    fn storage_error_other_display() {
        let err = StorageError::Other("custom error".to_string());
        assert_eq!(format!("{}", err), "custom error");
    }

    #[test]
    fn storage_error_converts_to_core_error() {
        let storage_err = StorageError::NotFound;
        let core_err: CoreError = storage_err.into();
        assert!(matches!(core_err, CoreError::InvalidOperation { .. }));
    }
}
