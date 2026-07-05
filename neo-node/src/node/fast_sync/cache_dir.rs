//! Fast-sync cache directory resolution.
//!
//! The node can resolve the package cache from an explicit fast-sync cache,
//! a storage override, the configured storage directory, or the default local
//! data directory. Keeping this policy isolated avoids mixing operator path
//! selection with package import orchestration.

use std::path::{Path, PathBuf};

use super::super::config::NodeConfig;

pub(super) fn fast_sync_cache_dir(
    config: &NodeConfig,
    storage_override: Option<&Path>,
    cache_dir_override: Option<&Path>,
) -> PathBuf {
    if let Some(cache_dir) = cache_dir_override {
        return cache_dir.to_path_buf();
    }
    let storage_root = storage_override
        .map(Path::to_path_buf)
        .or_else(|| config.storage.data_directory())
        .unwrap_or_else(|| PathBuf::from("data"));
    storage_root.join("fast-sync")
}
