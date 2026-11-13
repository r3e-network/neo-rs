//! Optional wall-clock observation for transactional storage commits.
//!
//! Profiling is diagnostic only: it never selects write options, changes commit
//! ordering, or alters error handling. Disabled timers do not read the clock.

use std::time::{Duration, Instant};

/// Observable stages nested inside a canonical store commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreCommitStage {
    /// Open the fresh writable backend snapshot used by `StoreCache`.
    WriterSnapshotOpen,
    /// Apply tracked cache entries to that snapshot's pending changes.
    ChangeApplication,
    /// Encode pending changes into the backend write batch.
    WriteBatchBuild,
    /// Atomically write the batch with the configured WAL and sync options.
    DatabaseWrite,
    /// Invalidate latest-state cache entries after a successful database write.
    CacheInvalidation,
}

/// Receives elapsed storage timings without influencing persistence.
pub trait StoreCommitProfiler: Send + Sync {
    fn record(&self, stage: StoreCommitStage, elapsed: Duration);
}

/// Records its stage on drop, including early-error exits from a profiled scope.
pub(crate) struct StoreCommitTimer<'a> {
    profiler: Option<&'a dyn StoreCommitProfiler>,
    stage: StoreCommitStage,
    started: Option<Instant>,
}

impl<'a> StoreCommitTimer<'a> {
    pub(crate) fn new(
        profiler: Option<&'a dyn StoreCommitProfiler>,
        stage: StoreCommitStage,
    ) -> Self {
        Self {
            profiler,
            stage,
            started: profiler.map(|_| Instant::now()),
        }
    }
}

impl Drop for StoreCommitTimer<'_> {
    fn drop(&mut self) {
        if let (Some(profiler), Some(started)) = (self.profiler, self.started) {
            profiler.record(self.stage, started.elapsed());
        }
    }
}
