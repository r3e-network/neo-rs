//! Storage configuration helpers and shared enums.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// MDBX storage configuration shared by the provider and node composition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Path to the database directory.
    pub path: PathBuf,
    /// MDBX maximum geometry size in bytes.
    pub mdbx_geometry_upper_bytes: Option<isize>,
    /// MDBX geometry growth step in bytes.
    pub mdbx_geometry_growth_bytes: Option<isize>,
    /// Maximum number of concurrent MDBX readers.
    pub mdbx_max_readers: Option<u32>,
    /// Open database in read-only mode.
    pub read_only: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("./data"),
            mdbx_geometry_upper_bytes: None,
            mdbx_geometry_growth_bytes: None,
            mdbx_max_readers: None,
            read_only: false,
        }
    }
}

// Re-export StorageError from neo-storage as the canonical definition.
pub use crate::StorageError;

#[cfg(test)]
#[path = "../../tests/persistence/storage.rs"]
mod tests;
