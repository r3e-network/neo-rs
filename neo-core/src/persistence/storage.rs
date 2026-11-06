//! Compatibility paths for storage-owned configuration and shared enums.

pub use neo_storage::StorageError;
pub use neo_storage::persistence::storage::{
    BatchOperation, CompactionStrategy, CompressionAlgorithm, StorageConfig,
};

/// Legacy core result alias; raw storage APIs use `neo_storage::StorageResult`.
pub type StorageResult<T> = neo_smart_contract::error::CoreResult<T>;

#[cfg(test)]
mod tests {
    use super::*;
    use neo_smart_contract::error::CoreError;

    #[test]
    fn storage_error_converts_to_core_error() {
        let storage_err = StorageError::KeyNotFound {
            key: "some_key".to_string(),
        };
        let core_err: CoreError = storage_err.into();
        assert!(matches!(core_err, CoreError::InvalidOperation { .. }));
    }

    #[test]
    fn storage_config_compatibility_is_the_storage_authority() {
        let config: neo_storage::persistence::storage::StorageConfig = StorageConfig::default();
        let canonical: neo_io_crate::compression::CompressionAlgorithm =
            config.compression_algorithm;
        assert_eq!(canonical, CompressionAlgorithm::Lz4);
    }
}
