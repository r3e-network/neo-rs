//! Optional wall-clock observation for the canonical persistence pipeline.
//!
//! Profiling never changes execution, commit ordering, or failure handling. The
//! caller supplies a profiler only for diagnostic runs; the canonical entry point
//! does not even construct stage timers when profiling is disabled.

use std::time::{Duration, Instant};

/// Non-nested and nested persistence stages observed by the ledger adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistenceStage {
    SnapshotOpen,
    TransactionHash,
    OnPersist,
    TransactionLoop,
    TransactionVm,
    PostPersist,
    ParticipantPrecommit,
    StoreCommit,
    RuntimeUpdate,
    PostCommit,
}

/// Receives elapsed wall-clock measurements without influencing persistence.
pub trait PersistenceProfiler: Send + Sync {
    fn record(&self, stage: PersistenceStage, elapsed: Duration);
}

/// Records its stage on drop, including early-error exits from a profiled scope.
pub(crate) struct StageTimer<'a> {
    profiler: Option<&'a dyn PersistenceProfiler>,
    stage: PersistenceStage,
    started: Option<Instant>,
}

impl<'a> StageTimer<'a> {
    pub(crate) fn new(
        profiler: Option<&'a dyn PersistenceProfiler>,
        stage: PersistenceStage,
    ) -> Self {
        Self {
            profiler,
            stage,
            started: profiler.map(|_| Instant::now()),
        }
    }
}

impl Drop for StageTimer<'_> {
    fn drop(&mut self) {
        if let (Some(profiler), Some(started)) = (self.profiler, self.started) {
            profiler.record(self.stage, started.elapsed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestProfiler(Mutex<Vec<(PersistenceStage, Duration)>>);

    impl PersistenceProfiler for TestProfiler {
        fn record(&self, stage: PersistenceStage, elapsed: Duration) {
            self.0.lock().unwrap().push((stage, elapsed));
        }
    }

    #[test]
    fn disabled_timer_does_not_observe_and_enabled_timer_records_on_drop() {
        let profiler = TestProfiler::default();
        drop(StageTimer::new(None, PersistenceStage::SnapshotOpen));
        assert!(profiler.0.lock().unwrap().is_empty());

        drop(StageTimer::new(
            Some(&profiler),
            PersistenceStage::StoreCommit,
        ));
        let records = profiler.0.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, PersistenceStage::StoreCommit);
    }
}
