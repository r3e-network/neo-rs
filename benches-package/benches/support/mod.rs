//! # neo-benches::support
//!
//! Shared storage fixtures for Criterion targets.
//!
//! ## Boundary
//!
//! This module owns benchmark setup and cleanup only. It may compose concrete
//! storage providers, but it does not define production storage policy or
//! include setup time in measured node operations.
//!
//! ## Contents
//!
//! - [`BenchTempDir`]: process-unique temporary-directory ownership.
//! - [`make_mdbx_store`]: production-shaped MDBX benchmark construction.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use neo_storage::{
    mdbx::{MdbxStore, MdbxStoreProvider},
    persistence::storage::StorageConfig,
};

static BENCH_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Temporary benchmark directory removed when its owning benchmark completes.
pub(super) struct BenchTempDir {
    path: PathBuf,
}

impl BenchTempDir {
    /// Creates a process-unique temporary directory path.
    pub(super) fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            BENCH_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        Self { path }
    }

    /// Returns the directory path.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for BenchTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Opens a benchmark MDBX store with production-scale geometry headroom.
pub(super) fn make_mdbx_store(prefix: &str) -> (Arc<MdbxStore>, BenchTempDir) {
    let tempdir = BenchTempDir::new(prefix);
    let store = MdbxStoreProvider::new(StorageConfig {
        path: tempdir.path().to_path_buf(),
        mdbx_geometry_upper_bytes: Some(8 * 1024 * 1024 * 1024),
        mdbx_geometry_growth_bytes: Some(64 * 1024 * 1024),
        ..Default::default()
    })
    .get_mdbx_store("")
    .expect("open benchmark MDBX store");
    (Arc::new(store), tempdir)
}
