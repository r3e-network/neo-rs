use std::sync::atomic::{AtomicU64, Ordering};

/// Snapshot of state root ingestion counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRootIngestStats {
    pub accepted: u64,
    pub rejected: u64,
}

static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static REJECTED: AtomicU64 = AtomicU64::new(0);

/// Records the outcome of processing a state root from the network.
pub fn record_ingest_result(accepted: bool) {
    if accepted {
        ACCEPTED.fetch_add(1, Ordering::Relaxed);
    } else {
        REJECTED.fetch_add(1, Ordering::Relaxed);
    }
}

/// Returns the current ingestion counters.
pub fn state_root_ingest_stats() -> StateRootIngestStats {
    StateRootIngestStats {
        accepted: ACCEPTED.load(Ordering::Relaxed),
        rejected: REJECTED.load(Ordering::Relaxed),
    }
}
