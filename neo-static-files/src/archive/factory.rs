//! Factory open path, format initialization, and startup tail recovery.

use std::fs::OpenOptions;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use lru::LruCache;
use parking_lot::{Mutex, RwLock};

use super::index::{ScanResult, scan_archive};
use super::io::{read_exact_at, sync_parent_directory, write_all_at};
use super::lease::WriterLease;
use super::provider::{ArchiveInner, StaticFileArchive};
use super::{StaticFileConfig, StaticFileProviderFactory};
use crate::format::{FILE_HEADER_LEN, file_header, validate_file_header};
use crate::{StaticFileError, StaticFileResult};

/// Factory for the versioned append-only file provider.
#[derive(Clone, Copy, Debug, Default)]
pub struct StaticFileArchiveFactory {
    config: StaticFileConfig,
}

impl StaticFileArchiveFactory {
    /// Creates a factory with explicit limits and compression policy.
    #[must_use]
    pub const fn new(config: StaticFileConfig) -> Self {
        Self { config }
    }

    /// Returns the configured archive policy.
    #[must_use]
    pub const fn config(self) -> StaticFileConfig {
        self.config
    }
}

impl StaticFileProviderFactory for StaticFileArchiveFactory {
    type Provider = StaticFileArchive;

    fn open(&self, path: &Path) -> StaticFileResult<Self::Provider> {
        self.config.validate()?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|source| StaticFileError::io("create directory", parent, source))?;
        }
        let existed = path.exists();
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|source| StaticFileError::io("open", path, source))?;
        WriterLease::acquire(&file, path)?;
        let mut file_len = file
            .metadata()
            .map_err(|source| StaticFileError::io("read metadata", path, source))?
            .len();

        if file_len < u64::try_from(FILE_HEADER_LEN).expect("header length fits u64") {
            file.set_len(0)
                .map_err(|source| StaticFileError::io("reset partial header", path, source))?;
            write_all_at(&file, 0, &file_header())
                .map_err(|source| StaticFileError::io("write header", path, source))?;
            file.sync_all()
                .map_err(|source| StaticFileError::io("sync header", path, source))?;
            if !existed {
                sync_parent_directory(path)?;
            }
            file_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
        } else {
            let mut header = [0u8; FILE_HEADER_LEN];
            read_exact_at(&file, 0, &mut header)
                .map_err(|source| StaticFileError::io("read header", path, source))?;
            validate_file_header(&header, 0)?;
        }

        let ScanResult {
            index,
            valid_file_len,
        } = scan_archive(&file, path, self.config, file_len)?;
        if valid_file_len != file_len {
            file.set_len(valid_file_len)
                .map_err(|source| StaticFileError::io("truncate torn tail", path, source))?;
            file.sync_all()
                .map_err(|source| StaticFileError::io("sync recovered tail", path, source))?;
        }

        Ok(StaticFileArchive {
            inner: Arc::new(ArchiveInner {
                file,
                path: path.to_path_buf(),
                config: self.config,
                write_lock: Mutex::new(()),
                index: RwLock::new(index),
                cache: Mutex::new(LruCache::new(
                    NonZeroUsize::new(self.config.cache_capacity)
                        .expect("config validation rejects a zero cache"),
                )),
                healthy: AtomicBool::new(true),
            }),
        })
    }
}
