//! Local replay recovery guard.
//!
//! StateService and a persistent indexer can publish from the blockchain
//! pre-commit hook, while the canonical ledger reaches durable storage at a
//! later fence. Those stores cannot share one transaction. Before either
//! observer runs, the guard writes and fsyncs a marker. Both observer backends
//! are explicitly fenced before Ledger. A successful canonical fence removes
//! and directory-syncs the marker; a crash or failed fence leaves it in place,
//! requests shutdown, and makes startup refuse the data set until an operator
//! restores matching stores. ApplicationLogs and TokensTracker commit only
//! from the post-canonical hook and therefore do not arm this guard. The static
//! Ledger archive stages durable but provider-invisible bytes before canonical
//! storage and publishes their index only after hot success. Startup recovers
//! and truncates any cold-ahead suffix, so this self-reconciling mirror also
//! does not require the poison marker.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use tokio_util::sync::CancellationToken;
use tracing::error;

const LOCAL_REPLAY_POISON_MARKER: &str = ".neo-local-replay-poisoned";

/// Returns the marker path for a persistent canonical store root.
pub(in crate::node) fn local_replay_marker_path(storage_root: Option<&Path>) -> Option<PathBuf> {
    storage_root.map(|root| root.join(LOCAL_REPLAY_POISON_MARKER))
}

/// Refuses startup when a previous dual-store commit did not finish safely.
pub(in crate::node) fn refuse_local_replay_marker(
    marker_path: Option<&Path>,
) -> anyhow::Result<()> {
    let Some(marker_path) = marker_path else {
        return Ok(());
    };
    if marker_path
        .try_exists()
        .with_context(|| format!("checking recovery marker {}", marker_path.display()))?
    {
        anyhow::bail!(
            "local replay is poisoned by an incomplete cross-store commit (marker: {}); restore matching canonical and pre-commit observer stores, then remove the marker",
            marker_path.display()
        );
    }
    Ok(())
}

/// Tracks whether a pre-commit observer entered a distinct durability domain.
pub(in crate::node) struct LocalReplayGuard {
    marker_path: Option<PathBuf>,
    shutdown: CancellationToken,
    observer_commit_pending: AtomicBool,
    poisoned: AtomicBool,
}

impl std::fmt::Debug for LocalReplayGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalReplayGuard")
            .field("marker_path", &self.marker_path)
            .field(
                "observer_commit_pending",
                &self.observer_commit_pending.load(Ordering::Acquire),
            )
            .field("poisoned", &self.poisoned.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl LocalReplayGuard {
    /// Creates a guard for the composed node shutdown domain.
    pub(in crate::node) fn new(marker_path: Option<PathBuf>, shutdown: CancellationToken) -> Self {
        Self {
            marker_path,
            shutdown,
            observer_commit_pending: AtomicBool::new(false),
            poisoned: AtomicBool::new(false),
        }
    }

    /// Arms crash recovery before a pre-commit observer can publish.
    pub(in crate::node) fn begin_observer_commit(&self) -> bool {
        if self.poisoned.load(Ordering::Acquire) {
            self.shutdown.cancel();
            return false;
        }
        if self.observer_commit_pending.load(Ordering::Acquire) {
            return true;
        }

        if let Some(marker_path) = &self.marker_path
            && let Err(marker_error) =
                write_recovery_marker(marker_path, "pre-commit observer publication in progress")
        {
            error!(
                target: "neo::recovery",
                path = %marker_path.display(),
                error = %marker_error,
                "failed to arm local replay recovery marker"
            );
            self.shutdown.cancel();
            return false;
        }
        self.observer_commit_pending.store(true, Ordering::Release);
        true
    }

    /// Clears the hazard only after the canonical durability fence succeeds.
    pub(in crate::node) fn canonical_commit_succeeded(&self) {
        if !self.observer_commit_pending.load(Ordering::Acquire)
            || self.poisoned.load(Ordering::Acquire)
        {
            return;
        }
        if let Some(marker_path) = &self.marker_path
            && let Err(marker_error) = clear_recovery_marker(marker_path)
        {
            error!(
                target: "neo::recovery",
                path = %marker_path.display(),
                error = %marker_error,
                "failed to clear local replay recovery marker after canonical commit"
            );
            self.shutdown.cancel();
            return;
        }
        self.observer_commit_pending.store(false, Ordering::Release);
    }

    /// Requests shutdown and persists a marker when observer stores may be ahead.
    pub(in crate::node) fn canonical_commit_failed(&self, reason: &str) {
        let observer_commit_pending = self.observer_commit_pending.load(Ordering::Acquire);
        if observer_commit_pending {
            self.poisoned.store(true, Ordering::Release);
            if let Some(marker_path) = &self.marker_path
                && let Err(marker_error) = write_recovery_marker(marker_path, reason)
            {
                error!(
                    target: "neo::recovery",
                    path = %marker_path.display(),
                    error = %marker_error,
                    "failed to persist local replay poison marker"
                );
            }
            error!(
                target: "neo::recovery",
                reason,
                "canonical and observer stores may be inconsistent; requesting shutdown"
            );
        } else {
            error!(
                target: "neo::recovery",
                reason,
                "canonical storage durability failed; requesting shutdown"
            );
        }
        self.shutdown.cancel();
    }

    /// Requests a clean restart for a recoverable post-canonical failure.
    ///
    /// Unlike [`Self::canonical_commit_failed`], this does not create a poison
    /// marker: the canonical and pre-commit observer stores are already
    /// consistent, and startup can rebuild the lagging mirror from Ledger.
    pub(in crate::node) fn request_recoverable_restart(&self, reason: &str) {
        error!(
            target: "neo::recovery",
            reason,
            "recoverable post-canonical service failure; requesting restart"
        );
        self.shutdown.cancel();
    }

    /// Returns whether recovery policy has made local replay fatal.
    pub(in crate::node) fn shutdown_requested(&self) -> bool {
        self.shutdown.is_cancelled()
    }
}

fn write_recovery_marker(marker_path: &Path, reason: &str) -> anyhow::Result<()> {
    let parent = marker_path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating recovery marker directory {}", parent.display()))?;

    let mut marker = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(marker_path)
        .with_context(|| format!("creating recovery marker {}", marker_path.display()))?;
    writeln!(marker, "reason={reason}")
        .with_context(|| format!("writing recovery marker {}", marker_path.display()))?;
    marker
        .sync_all()
        .with_context(|| format!("syncing recovery marker {}", marker_path.display()))?;

    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("syncing recovery marker directory {}", parent.display()))?;
    Ok(())
}

fn clear_recovery_marker(marker_path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(marker_path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("removing recovery marker {}", marker_path.display()));
        }
    }

    let parent = marker_path.parent().unwrap_or_else(|| Path::new("."));
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("syncing recovery marker directory {}", parent.display()))
}
