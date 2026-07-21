//! Typed pack-store operational failures.

use std::path::PathBuf;

/// Typed operational failures specific to the pack store.
///
/// Other format and I/O failures retain their detailed `anyhow` context.
/// Callers can downcast errors to distinguish active-writer ownership and
/// resource deferral from corruption or ordinary I/O failure.
#[derive(Debug, thiserror::Error)]
pub enum PackStoreError {
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
        source: std::io::Error,
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
    #[error("append committed; derived-index maintenance failed: {details}")]
    CommittedMaintenance {
        /// Stable, contextual maintenance failure text.
        details: String,
    },
}
