//! # Pack-store errors
//!
//! Typed caller-facing failures for configuration, persisted formats,
//! resource bounds, I/O, corruption, ownership, and committed maintenance.
//!
//! ## Boundary
//!
//! Store internals may attach `anyhow` context while decoding and recovering
//! files. This module classifies that chain exactly once before it crosses the
//! public API and retains the original source without flattening it to text.
//!
//! ## Contents
//!
//! - [`PackStoreError`]: stable categories on which callers can branch.
//! - [`PackStoreResult`]: result type returned by public pack-store operations.
//! - [`PackStoreErrorSource`]: retained internal source chain for diagnostics.

use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use super::config::PackStoreConfigError;

/// Result type returned by public pack-store operations.
pub type PackStoreResult<T> = std::result::Result<T, PackStoreError>;

/// Persisted or derived artifact involved in a format or corruption failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackStoreArtifact {
    /// Root directory and its required store layout.
    StoreDirectory,
    /// Append-only payload segment.
    Segment,
    /// Checksummed operation frame.
    Frame,
    /// Immutable sorted index run.
    IndexRun,
    /// Immutable visibility manifest.
    Manifest,
    /// External canonical commit horizon.
    CommitHorizon,
    /// Pinned immutable read generation.
    Snapshot,
    /// Derived compaction output.
    Compaction,
}

impl fmt::Display for PackStoreArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StoreDirectory => "store directory",
            Self::Segment => "segment",
            Self::Frame => "frame",
            Self::IndexRun => "index run",
            Self::Manifest => "manifest",
            Self::CommitHorizon => "commit horizon",
            Self::Snapshot => "snapshot",
            Self::Compaction => "compaction output",
        })
    }
}

/// Pack-store operation that failed while performing filesystem I/O.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackStoreOperation {
    /// Create a new pack store.
    Create,
    /// Open and recover an existing pack store.
    Open,
    /// Read payload or index bytes.
    Read,
    /// Append or replace durable bytes.
    Write,
    /// Fence file or directory durability.
    Sync,
    /// Recover a committed prefix after restart.
    Recover,
    /// Rebuild derived index state.
    Rebuild,
    /// Build or adopt derived compaction state.
    Compact,
    /// Publish a new visible generation.
    Publish,
    /// Reclaim files outside every pinned generation.
    Reclaim,
    /// Scrub committed bytes or derived indexes.
    Scrub,
}

impl fmt::Display for PackStoreOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Create => "create",
            Self::Open => "open",
            Self::Read => "read",
            Self::Write => "write",
            Self::Sync => "sync",
            Self::Recover => "recover",
            Self::Rebuild => "rebuild",
            Self::Compact => "compact",
            Self::Publish => "publish",
            Self::Reclaim => "reclaim",
            Self::Scrub => "scrub",
        })
    }
}

/// Hard bounded resource whose requested size was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackStoreLimit {
    /// Rows encoded in one frame.
    FrameRows,
    /// Encoded payload bytes in one frame.
    FramePayloadBytes,
    /// Bytes in one append segment.
    SegmentBytes,
    /// Resident decoded-index bytes.
    IndexMemoryBytes,
    /// Unpublished append bytes.
    PendingBytes,
    /// Recent immutable run count.
    RecentRuns,
    /// Derived index level count.
    IndexLevels,
    /// Excess immutable runs waiting for compaction.
    CompactionDebtRuns,
    /// Bytes returned for one indexed value.
    ValueBytes,
    /// Aggregate value bytes returned by one sorted lookup.
    SortedLookupValueBytes,
}

impl fmt::Display for PackStoreLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FrameRows => "frame rows",
            Self::FramePayloadBytes => "frame payload bytes",
            Self::SegmentBytes => "segment bytes",
            Self::IndexMemoryBytes => "decoded index memory bytes",
            Self::PendingBytes => "pending append bytes",
            Self::RecentRuns => "recent index runs",
            Self::IndexLevels => "index levels",
            Self::CompactionDebtRuns => "compaction debt runs",
            Self::ValueBytes => "indexed value bytes",
            Self::SortedLookupValueBytes => "sorted lookup value bytes",
        })
    }
}

/// Original contextual error retained behind a typed pack-store category.
///
/// The wrapper keeps internal context and concrete sources available to
/// diagnostics and callers that need a lower-level classification, without
/// exposing `anyhow::Error` as the public operation result.
#[derive(Debug)]
pub struct PackStoreErrorSource {
    inner: anyhow::Error,
}

impl PackStoreErrorSource {
    /// Returns a concrete error from the retained context chain, when present.
    pub fn downcast_ref<E>(&self) -> Option<&E>
    where
        E: fmt::Display + fmt::Debug + Send + Sync + 'static,
    {
        self.inner.downcast_ref::<E>()
    }

    /// Returns the deepest concrete cause in the retained context chain.
    pub fn root_cause(&self) -> &(dyn StdError + 'static) {
        self.inner.root_cause()
    }

    pub(in crate::engine::store) fn new(inner: anyhow::Error) -> Self {
        Self { inner }
    }
}

impl fmt::Display for PackStoreErrorSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:#}", self.inner)
    }
}

impl StdError for PackStoreErrorSource {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.inner.as_ref())
    }
}

/// Typed failures produced by the pack store.
#[derive(Debug, thiserror::Error)]
pub enum PackStoreError {
    /// A bounded pack-store setting is invalid.
    #[error("invalid node-pack configuration: {0}")]
    Configuration(#[from] PackStoreConfigError),
    /// Persisted bytes do not conform to the artifact's declared format.
    #[error("invalid node-pack {artifact} format at {}: {source}", path.display())]
    InvalidFormat {
        /// Artifact whose structure is malformed.
        artifact: PackStoreArtifact,
        /// File or store path involved in validation.
        path: PathBuf,
        /// Original parser or validation failure chain.
        #[source]
        source: PackStoreErrorSource,
    },
    /// Persisted bytes declare a format version this binary cannot read.
    #[error(
        "unsupported node-pack {artifact} version {found}; supported versions are {supported:?}"
    )]
    UnsupportedVersion {
        /// Artifact carrying the unsupported version.
        artifact: PackStoreArtifact,
        /// Version decoded from persisted bytes.
        found: u32,
        /// Complete set of versions this binary accepts for that artifact.
        supported: &'static [u32],
    },
    /// A requested resource exceeds an explicit hard bound.
    #[error("node-pack {limit} {actual} exceeds hard limit {maximum}")]
    LimitExceeded {
        /// Resource whose size was rejected.
        limit: PackStoreLimit,
        /// Requested or decoded size.
        actual: u64,
        /// Inclusive hard maximum.
        maximum: u64,
    },
    /// A concrete filesystem operation failed.
    #[error("failed to {operation} node-pack path {}: {source}", path.display())]
    Io {
        /// Store operation being performed.
        operation: PackStoreOperation,
        /// Closest stable path owned by the public operation.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: io::Error,
    },
    /// Persisted committed state is inconsistent or fails authentication.
    #[error("node-pack {artifact} corruption at {}: {source}", path.display())]
    Corruption {
        /// Artifact that could not be trusted.
        artifact: PackStoreArtifact,
        /// File or store path involved in validation.
        path: PathBuf,
        /// Original authentication or consistency failure chain.
        #[source]
        source: PackStoreErrorSource,
    },
    /// Another process or handle owns the recovery and writer lease.
    #[error("node-pack writer is already active for {}", path.display())]
    WriterOwned {
        /// Lease-file path used for the ownership check.
        path: PathBuf,
    },
    /// The operating system could not acquire the kernel lease.
    #[error("failed to acquire node-pack writer lease for {}", path.display())]
    WriterLease {
        /// Lease-file path involved in the failed system call.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: io::Error,
    },
    /// The explicitly enabled parallel read path could not initialize its
    /// bounded worker pool. Persisted store bytes have not been classified as
    /// corrupt and callers may retry with one read worker.
    #[error("failed to initialize node-pack read worker pool with {workers} workers: {details}")]
    ReadWorkerPoolUnavailable {
        /// Number of worker threads requested from Rayon.
        workers: usize,
        /// Stable diagnostic returned by the worker-pool builder.
        details: String,
    },
    /// The current in-memory compaction implementation cannot build this
    /// output without exceeding the configured transient workspace bound.
    /// Source runs remain live and no output file has been created.
    #[error(
        "compaction workspace estimate {estimated_bytes} bytes exceeds configured bound {max_bytes} bytes"
    )]
    CompactionWorkspaceExceeded {
        /// Conservative peak allocation estimate for the selected inputs.
        estimated_bytes: u64,
        /// Maximum transient workspace allowed for one compaction build.
        max_bytes: u64,
    },
    /// The frame and manifest were durably activated, but best-effort derived
    /// index maintenance failed afterwards. Callers must not retry the same
    /// logical append through this store; reopen through the canonical marker
    /// and either rebuild or schedule maintenance instead.
    #[error("append committed; derived-index maintenance failed: {source}")]
    CommittedMaintenance {
        /// Original derived-maintenance failure chain.
        #[source]
        source: PackStoreErrorSource,
    },
}

impl PackStoreError {
    pub(in crate::engine::store) fn committed_maintenance(source: anyhow::Error) -> Self {
        Self::CommittedMaintenance {
            source: PackStoreErrorSource::new(source),
        }
    }

    pub(in crate::engine::store) fn unsupported_version(
        artifact: PackStoreArtifact,
        found: u32,
        supported: &'static [u32],
    ) -> Self {
        Self::UnsupportedVersion {
            artifact,
            found,
            supported,
        }
    }

    pub(in crate::engine::store) fn read_worker_pool_unavailable(
        workers: usize,
        details: String,
    ) -> Self {
        Self::ReadWorkerPoolUnavailable { workers, details }
    }

    pub(in crate::engine::store) fn classify_create(source: anyhow::Error, root: &Path) -> Self {
        Self::classify(
            source,
            PackStoreOperation::Create,
            root,
            PackStoreArtifact::StoreDirectory,
            FailureClass::InvalidFormat,
        )
    }

    pub(in crate::engine::store) fn classify_open(source: anyhow::Error, root: &Path) -> Self {
        Self::classify(
            source,
            PackStoreOperation::Open,
            root,
            PackStoreArtifact::StoreDirectory,
            FailureClass::Corruption,
        )
    }

    fn classify(
        source: anyhow::Error,
        operation: PackStoreOperation,
        path: &Path,
        artifact: PackStoreArtifact,
        fallback: FailureClass,
    ) -> Self {
        let source = match source.downcast::<Self>() {
            Ok(error) => return error,
            Err(source) => source,
        };
        let source = match source.downcast::<PackStoreConfigError>() {
            Ok(error) => return error.into(),
            Err(source) => source,
        };
        let source = match source.downcast::<io::Error>() {
            Ok(source) => {
                return Self::Io {
                    operation,
                    path: path.to_path_buf(),
                    source,
                };
            }
            Err(source) => source,
        };
        let source = PackStoreErrorSource::new(source);
        match fallback {
            FailureClass::InvalidFormat => Self::InvalidFormat {
                artifact,
                path: path.to_path_buf(),
                source,
            },
            FailureClass::Corruption => Self::Corruption {
                artifact,
                path: path.to_path_buf(),
                source,
            },
        }
    }
}

#[derive(Clone, Copy)]
enum FailureClass {
    InvalidFormat,
    Corruption,
}

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;
    use crate::engine::store::api::PackStoreConfigField;

    #[derive(Debug, thiserror::Error)]
    #[error("authenticated bytes differ")]
    struct AuthenticationFailure;

    #[test]
    fn classifier_preserves_an_existing_typed_pack_error() {
        let expected_path = PathBuf::from("/tmp/pack/writer.lock");
        let source = anyhow::Error::new(PackStoreError::WriterOwned {
            path: expected_path.clone(),
        })
        .context("open pack store");

        let error = PackStoreError::classify_open(source, Path::new("/tmp/pack"));

        assert!(matches!(
            error,
            PackStoreError::WriterOwned { path } if path == expected_path
        ));
    }

    #[test]
    fn classifier_retains_typed_configuration_errors() {
        let source = anyhow::Error::new(PackStoreConfigError::ValueOutOfRange {
            field: PackStoreConfigField::MaxIndexMemoryBytes,
            actual: 0,
            minimum: 1,
            maximum: i64::MAX as u64,
        })
        .context("validate store options");

        let error = PackStoreError::classify_create(source, Path::new("/tmp/pack"));

        assert!(matches!(
            error,
            PackStoreError::Configuration(PackStoreConfigError::ValueOutOfRange {
                field: PackStoreConfigField::MaxIndexMemoryBytes,
                actual: 0,
                ..
            })
        ));
    }

    #[test]
    fn classifier_exposes_the_concrete_io_error() {
        let io_error = io::Error::new(io::ErrorKind::PermissionDenied, "read-only filesystem");
        let source = Err::<(), _>(io_error)
            .context("create append segment")
            .expect_err("fixture must fail");

        let error = PackStoreError::classify_create(source, Path::new("/var/lib/neo/packs"));

        assert!(matches!(
            error,
            PackStoreError::Io {
                operation: PackStoreOperation::Create,
                source,
                ..
            } if source.kind() == io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn classifier_does_not_report_worker_pool_failure_as_corruption() {
        let source = anyhow::Error::new(PackStoreError::read_worker_pool_unavailable(
            4,
            "thread quota exhausted".to_owned(),
        ))
        .context("preflight parallel pack reads");

        let error = PackStoreError::classify_open(source, Path::new("/var/lib/neo/packs"));

        assert!(matches!(
            error,
            PackStoreError::ReadWorkerPoolUnavailable { workers: 4, details }
                if details == "thread quota exhausted"
        ));
    }

    #[test]
    fn corruption_fallback_retains_the_original_source_type() {
        let source = anyhow::Error::new(AuthenticationFailure).context("validate frame checksum");

        let error = PackStoreError::classify_open(source, Path::new("/var/lib/neo/packs"));

        let PackStoreError::Corruption { source, .. } = error else {
            panic!("open validation failure must be classified as corruption");
        };
        assert!(source.downcast_ref::<AuthenticationFailure>().is_some());
        assert_eq!(
            source.root_cause().to_string(),
            "authenticated bytes differ"
        );
    }
}
