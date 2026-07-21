//! # neo-state-packs store writer lease
//!
//! Owns the kernel-held lease that excludes concurrent pack-store writers and recovery passes.
//!
//! ## Boundary
//!
//! This module controls writer ownership for one store directory. It does not open pack segments,
//! perform recovery, or publish state; those operations merely retain the returned file lease.
//!
//! ## Contents
//!
//! - [`acquire_writer_lease`]: opens and locks the stable writer-lease inode.

use super::api::PackStoreError;
use super::io::sync_directory;
use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

const WRITER_LEASE_FILE: &str = "writer.lock";
const WRITER_LEASE_RETRY_ATTEMPTS: usize = 10;
const WRITER_LEASE_RETRY_DELAY: Duration = Duration::from_millis(5);

/// Acquires one kernel-held lease before startup recovery can inspect or
/// mutate pack files. A dedicated inode keeps the lease stable while recovery
/// reopens or truncates the active pack segment.
pub(super) fn acquire_writer_lease(root: &Path) -> Result<File> {
    let lease_path = root.join(WRITER_LEASE_FILE);
    let (lease, created) = match OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(&lease_path)
    {
        Ok(file) => (file, true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (
            OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_CLOEXEC)
                .open(&lease_path)
                .with_context(|| format!("open writer lease {}", lease_path.display()))?,
            false,
        ),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("create writer lease {}", lease_path.display()));
        }
    };
    for attempt in 0..=WRITER_LEASE_RETRY_ATTEMPTS {
        match lease.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if attempt < WRITER_LEASE_RETRY_ATTEMPTS => {
                // `flock` is inherited between fork and exec. A concurrent
                // subprocess can therefore retain another test/thread's
                // CLOEXEC lease for a few milliseconds after its owner drops.
                std::thread::sleep(WRITER_LEASE_RETRY_DELAY);
            }
            Err(TryLockError::WouldBlock) => {
                return Err(PackStoreError::WriterOwned {
                    path: fs::canonicalize(&lease_path).unwrap_or(lease_path),
                }
                .into());
            }
            Err(TryLockError::Error(source)) => {
                return Err(PackStoreError::WriterLease {
                    path: fs::canonicalize(&lease_path).unwrap_or(lease_path),
                    source,
                }
                .into());
            }
        }
    }
    if created {
        sync_directory(root)?;
    }
    Ok(lease)
}
