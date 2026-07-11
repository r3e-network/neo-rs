//! Cross-process ownership for the archive writer and recovery handle.

use std::fs::{File, TryLockError};
use std::path::Path;

use crate::{StaticFileError, StaticFileResult};

/// Exclusive writer lease held by the archive file handle itself.
pub(super) struct WriterLease;

impl WriterLease {
    pub(super) fn acquire(file: &File, archive_path: &Path) -> StaticFileResult<()> {
        match file.try_lock() {
            Ok(()) => Ok(()),
            Err(TryLockError::WouldBlock) => Err(StaticFileError::WriterOwned {
                path: archive_path.to_path_buf(),
            }),
            Err(TryLockError::Error(source)) => Err(StaticFileError::io(
                "acquire writer lease",
                archive_path,
                source,
            )),
        }
    }
}
