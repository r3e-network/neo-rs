//! # neo-node::lifecycle_lock
//!
//! Exclusive process ownership for a local node data directory.
//!
//! ## Boundary
//!
//! This module coordinates node and offline-maintenance process lifetimes. It
//! deliberately does not lock individual database tables or pack segments;
//! those stores retain their own transactional and writer-lease contracts.
//!
//! ## Contents
//!
//! - [`NodeLifecycleLock`]: RAII ownership guard for a data directory.
//! - [`NodeLifecycleLockError`]: typed acquisition failures.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Stable lock-file name shared by the daemon and offline maintenance tools.
pub const NODE_LIFECYCLE_LOCK_FILE: &str = "node.lifecycle.lock";

/// Exclusive process ownership of one local node data directory.
///
/// The advisory kernel lock is held until this guard is dropped. Every process
/// that can mutate the canonical node store must acquire this guard before
/// opening that store and retain it through its final durable operation.
#[derive(Debug)]
pub struct NodeLifecycleLock {
    _file: File,
    path: PathBuf,
}

impl NodeLifecycleLock {
    /// Acquires exclusive lifecycle ownership for `data_directory`.
    ///
    /// The directory and persistent lock file are created when absent. The
    /// lock attempt never waits: concurrent ownership returns
    /// [`NodeLifecycleLockError::AlreadyHeld`].
    ///
    /// # Errors
    ///
    /// Returns an error when the directory or lock file cannot be opened, the
    /// kernel rejects the lock operation, or another process owns the lock.
    pub fn acquire(data_directory: impl AsRef<Path>) -> Result<Self, NodeLifecycleLockError> {
        let data_directory = data_directory.as_ref();
        std::fs::create_dir_all(data_directory).map_err(|source| {
            NodeLifecycleLockError::CreateDirectory {
                directory: data_directory.to_path_buf(),
                source,
            }
        })?;
        let data_directory = std::fs::canonicalize(data_directory).map_err(|source| {
            NodeLifecycleLockError::CanonicalizeDirectory {
                directory: data_directory.to_path_buf(),
                source,
            }
        })?;

        let path = data_directory.join(NODE_LIFECYCLE_LOCK_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| NodeLifecycleLockError::Open {
                path: path.clone(),
                source,
            })?;

        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file, path }),
            Err(TryLockError::WouldBlock) => Err(NodeLifecycleLockError::AlreadyHeld { path }),
            Err(TryLockError::Error(source)) => {
                Err(NodeLifecycleLockError::Acquire { path, source })
            }
        }
    }

    /// Returns the persistent file whose kernel lock this guard owns.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Failure to acquire exclusive node lifecycle ownership.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NodeLifecycleLockError {
    /// The node data directory could not be created.
    #[error("failed to create node data directory {directory}")]
    CreateDirectory {
        /// Requested node data directory.
        directory: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// The created node data directory could not be resolved to one stable path.
    #[error("failed to canonicalize node data directory {directory}")]
    CanonicalizeDirectory {
        /// Requested node data directory.
        directory: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// The persistent lifecycle lock file could not be opened.
    #[error("failed to open node lifecycle lock {path}")]
    Open {
        /// Lifecycle lock path.
        path: PathBuf,
        /// Filesystem failure.
        #[source]
        source: io::Error,
    },
    /// Another node or maintenance process already owns the directory.
    #[error(
        "node data directory is already in use (lifecycle lock {path}); stop the running node or maintenance tool before retrying"
    )]
    AlreadyHeld {
        /// Contended lifecycle lock path.
        path: PathBuf,
    },
    /// The operating system rejected the lock operation.
    #[error("failed to acquire node lifecycle lock {path}")]
    Acquire {
        /// Lifecycle lock path.
        path: PathBuf,
        /// Operating-system failure.
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    const HOLDER_DIRECTORY_ENV: &str = "NEO_NODE_LOCK_TEST_HOLDER_DIRECTORY";
    const HOLDER_READY_ENV: &str = "NEO_NODE_LOCK_TEST_HOLDER_READY";

    #[test]
    fn creates_directory_and_releases_on_drop() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let data_directory = temporary.path().join("chain");

        let guard = NodeLifecycleLock::acquire(&data_directory).expect("acquire lifecycle lock");
        assert_eq!(guard.path(), data_directory.join(NODE_LIFECYCLE_LOCK_FILE));
        assert!(guard.path().is_file());

        drop(guard);
        NodeLifecycleLock::acquire(&data_directory).expect("reacquire released lifecycle lock");
    }

    #[cfg(unix)]
    #[test]
    fn canonical_path_prevents_symlink_alias_bypass() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().expect("create temporary directory");
        let data_directory = temporary.path().join("chain");
        std::fs::create_dir(&data_directory).expect("create real node-data directory");
        let alias = temporary.path().join("chain-alias");
        symlink(&data_directory, &alias).expect("create node-data symlink alias");

        let held = NodeLifecycleLock::acquire(&data_directory).expect("acquire canonical lock");
        let error = NodeLifecycleLock::acquire(&alias)
            .expect_err("symlink alias must address the same lock");
        match error {
            NodeLifecycleLockError::AlreadyHeld { path } => assert_eq!(path, held.path()),
            other => panic!("unexpected alias acquisition error: {other}"),
        }
    }

    #[test]
    fn rejects_contention_from_another_process() {
        let temporary = tempfile::tempdir().expect("create temporary directory");
        let data_directory = temporary.path().join("chain");
        let ready_path = temporary.path().join("holder.ready");
        let test_binary = std::env::current_exe().expect("resolve current test binary");
        let mut child = Command::new(test_binary)
            .args([
                "--exact",
                "lifecycle_lock::tests::subprocess_lock_holder",
                "--nocapture",
            ])
            .env(HOLDER_DIRECTORY_ENV, &data_directory)
            .env(HOLDER_READY_ENV, &ready_path)
            .stdin(Stdio::piped())
            .spawn()
            .expect("spawn lifecycle lock holder");

        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready_path.exists() {
            if let Some(status) = child.try_wait().expect("poll lock holder") {
                panic!("lifecycle lock holder exited before readiness: {status}");
            }
            assert!(
                Instant::now() < deadline,
                "lifecycle lock holder did not become ready"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let error = NodeLifecycleLock::acquire(&data_directory)
            .expect_err("concurrent process must own lifecycle lock");
        assert!(matches!(error, NodeLifecycleLockError::AlreadyHeld { .. }));

        drop(child.stdin.take());
        let status = child.wait().expect("wait for lifecycle lock holder");
        assert!(status.success(), "lifecycle lock holder failed: {status}");
        NodeLifecycleLock::acquire(&data_directory)
            .expect("acquire lifecycle lock after holder exits");
    }

    #[test]
    fn subprocess_lock_holder() {
        let Some(data_directory) = std::env::var_os(HOLDER_DIRECTORY_ENV) else {
            return;
        };
        let ready_path = std::env::var_os(HOLDER_READY_ENV).expect("holder ready path");
        let _guard = NodeLifecycleLock::acquire(data_directory).expect("holder acquires lock");
        std::fs::write(ready_path, []).expect("publish holder readiness");
        std::io::stdin()
            .read_to_end(&mut Vec::new())
            .expect("wait for parent release");
    }
}
