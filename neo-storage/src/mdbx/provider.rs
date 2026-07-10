use super::store::MdbxStore;
use crate::persistence::{storage::StorageConfig, store_provider::StoreProvider};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const DEFAULT_MAP_SIZE: isize = 256 * 1024 * 1024 * 1024;
const DEFAULT_GROWTH_STEP: isize = 256 * 1024 * 1024;
const DEFAULT_MAX_READERS: u32 = 4096;

/// Resolved MDBX provider tuning.
///
/// `max_readers` is the requested reader limit retained at the provider
/// boundary. With the current `libmdbx` wrapper it is not guaranteed to be
/// enforced by the opened environment until the adapter moves to an open path
/// that forwards `MDBX_opt_max_readers` before `mdbx_env_open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MdbxTuning {
    /// Maximum MDBX map geometry in bytes.
    pub map_size: isize,
    /// MDBX geometry growth step in bytes.
    pub growth_step: isize,
    /// Requested maximum concurrent MDBX readers.
    pub max_readers: u32,
}

/// MDBX-backed store provider compatible with Neo's `Store`.
#[derive(Debug, Clone)]
pub struct MdbxStoreProvider {
    base_config: StorageConfig,
    map_size: isize,
    growth_step: isize,
    max_readers: u32,
}

impl MdbxStoreProvider {
    /// Creates a provider with the supplied base storage configuration.
    pub fn new(base_config: StorageConfig) -> Self {
        let map_size = base_config
            .mdbx_geometry_upper_bytes
            .unwrap_or(DEFAULT_MAP_SIZE);
        let growth_step = base_config
            .mdbx_geometry_growth_bytes
            .unwrap_or(DEFAULT_GROWTH_STEP);
        let max_readers = base_config.mdbx_max_readers.unwrap_or(DEFAULT_MAX_READERS);

        Self {
            base_config,
            map_size,
            growth_step,
            max_readers,
        }
    }

    /// Overrides the MDBX maximum geometry size in bytes.
    pub fn with_map_size(mut self, bytes: isize) -> Self {
        self.map_size = bytes;
        self
    }

    /// Overrides the MDBX geometry growth step in bytes.
    pub fn with_growth_step(mut self, bytes: isize) -> Self {
        self.growth_step = bytes;
        self
    }

    /// Records the requested maximum number of concurrent MDBX readers.
    ///
    /// The current `libmdbx` wrapper exposes this option but does not enforce
    /// it during environment open. Keep this method as the provider boundary so
    /// the value starts applying when the MDBX adapter moves to a wrapper/open
    /// path that forwards `MDBX_opt_max_readers` before `mdbx_env_open`.
    pub fn with_max_readers(mut self, readers: u32) -> Self {
        self.max_readers = readers;
        self
    }

    /// Returns the resolved provider-level MDBX tuning.
    pub fn tuning(&self) -> MdbxTuning {
        MdbxTuning {
            map_size: self.map_size,
            growth_step: self.growth_step,
            max_readers: self.max_readers,
        }
    }

    fn resolved_path(&self, override_path: &Path) -> PathBuf {
        if override_path.as_os_str().is_empty() {
            self.base_config.path.clone()
        } else {
            override_path.to_path_buf()
        }
    }

    fn build_store(&self, path: &Path) -> crate::StorageResult<MdbxStore> {
        MdbxStore::open(
            &self.resolved_path(path),
            self.map_size,
            self.growth_step,
            self.max_readers,
            self.base_config.read_only,
        )
    }

    /// Opens a store without erasing it behind a `Store` trait object.
    pub fn get_mdbx_store<P>(&self, path: P) -> crate::StorageResult<MdbxStore>
    where
        P: AsRef<Path>,
    {
        self.build_store(path.as_ref())
    }

    /// Opens a shared MDBX store.
    pub fn get_store<P>(&self, path: P) -> crate::StorageResult<Arc<MdbxStore>>
    where
        P: AsRef<Path>,
    {
        self.build_store(path.as_ref()).map(Arc::new)
    }
}

impl StoreProvider for MdbxStoreProvider {
    type Store = MdbxStore;

    fn name(&self) -> &str {
        "mdbx"
    }

    fn get_store(&self, path: &Path) -> crate::StorageResult<Arc<MdbxStore>> {
        self.build_store(path).map(Arc::new)
    }

    fn get_store_with_config(&self, config: StorageConfig) -> crate::StorageResult<Arc<MdbxStore>> {
        Self::new(config).get_store(Path::new(""))
    }
}
