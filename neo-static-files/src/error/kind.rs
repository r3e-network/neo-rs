//! Error types for static-file validation, recovery, and I/O.

use std::path::PathBuf;

/// Result type for static-file operations.
pub type StaticFileResult<T> = Result<T, StaticFileError>;

/// Failures raised by static-file validation, recovery, compression, or I/O.
#[derive(Debug, thiserror::Error)]
pub enum StaticFileError {
    /// A filesystem operation failed.
    #[error("static-file {operation} failed for {}: {source}", path.display())]
    Io {
        /// Operation being performed.
        operation: &'static str,
        /// Affected archive path.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
    /// The persistent archive index could not be opened or updated.
    #[error("static-file index {operation} failed for {}: {message}", path.display())]
    Index {
        /// Index operation being performed.
        operation: &'static str,
        /// Affected MDBX sidecar directory.
        path: PathBuf,
        /// Backend diagnostic.
        message: String,
    },
    /// Another process owns the archive's writer and recovery lease.
    #[error("static-file archive writer is already active for {}", path.display())]
    WriterOwned {
        /// Contested archive path.
        path: PathBuf,
    },
    /// The file or frame format is invalid.
    #[error("invalid static-file format at offset {offset}: {reason}")]
    InvalidFormat {
        /// Byte offset where validation failed.
        offset: u64,
        /// Human-readable invariant violation.
        reason: String,
    },
    /// The file uses a format version this build cannot read.
    #[error("unsupported static-file version {actual}; expected {expected}")]
    UnsupportedVersion {
        /// Version accepted by this build.
        expected: u16,
        /// Version stored in the file.
        actual: u16,
    },
    /// A complete frame failed an integrity checksum.
    #[error("static-file {component} checksum mismatch at height {height}")]
    Checksum {
        /// Finalized height carried by the frame.
        height: u32,
        /// Frame component whose checksum failed.
        component: &'static str,
    },
    /// An appended height is not the next finalized height.
    #[error("non-contiguous static-file append: expected height {expected}, got {actual}")]
    NonContiguous {
        /// Required next height.
        expected: u32,
        /// Supplied height.
        actual: u32,
    },
    /// A record contains the same opaque key more than once.
    #[error("duplicate static-file row key at height {height} (xxh3={key_hash:#018x})")]
    DuplicateKey {
        /// Record height.
        height: u32,
        /// Diagnostic-only hash of the duplicate key.
        key_hash: u64,
    },
    /// A finalized-height record has no rows.
    #[error("static-file record at height {height} contains no rows")]
    EmptyRecord {
        /// Record height.
        height: u32,
    },
    /// A record exceeds a configured safety bound.
    #[error("static-file record at height {height} exceeds {limit}: {actual}")]
    LimitExceeded {
        /// Record height.
        height: u32,
        /// Name and configured value of the bound.
        limit: String,
        /// Actual value encountered.
        actual: u64,
    },
    /// Compression or decompression failed.
    #[error("static-file compression failed: {0}")]
    Compression(String),
    /// A prior partial write made the current handle unsafe for further appends.
    #[error("static-file writer is unhealthy after a failed durability operation; reopen it")]
    Unhealthy,
}

impl StaticFileError {
    pub(crate) fn io(
        operation: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }

    pub(crate) fn invalid(offset: u64, reason: impl Into<String>) -> Self {
        Self::InvalidFormat {
            offset,
            reason: reason.into(),
        }
    }

    pub(crate) fn index(
        operation: &'static str,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self::Index {
            operation,
            path: path.into(),
            message: message.into(),
        }
    }

    pub(crate) fn invalid_index(reason: impl Into<String>) -> Self {
        Self::InvalidFormat {
            offset: 0,
            reason: format!("persistent index: {}", reason.into()),
        }
    }
}
