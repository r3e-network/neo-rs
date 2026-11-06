#![allow(missing_docs, dead_code)] // WIP metrics; docs pending cleanup
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// State root ingestion metrics (network side)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRootIngestStats {
    /// Number of state roots accepted after validation.
    pub accepted: u64,
    /// Number of state roots rejected during validation.
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

static DIVERGENCES: AtomicU64 = AtomicU64::new(0);

/// Records a validator-signed state root that disagreed with the local root.
pub fn record_divergence() {
    DIVERGENCES.fetch_add(1, Ordering::Relaxed);
}

/// Returns how many validated/local state root divergences were observed.
pub fn state_root_divergences() -> u64 {
    DIVERGENCES.load(Ordering::Relaxed)
}

/// Returns the current ingestion counters.
pub fn state_root_ingest_stats() -> StateRootIngestStats {
    StateRootIngestStats {
        accepted: ACCEPTED.load(Ordering::Relaxed),
        rejected: REJECTED.load(Ordering::Relaxed),
    }
}

/// Metrics for async state root service
#[derive(Debug)]
pub struct StateRootMetrics {
    /// Total triggers sent to async worker pool
    total_triggered: Arc<AtomicU64>,
    /// Total successful computations
    total_computed: Arc<AtomicU64>,
    /// Total verification successes
    total_verified: Arc<AtomicU64>,
    /// Total verification failures
    total_verification_failures: Arc<AtomicU64>,
}

impl Clone for StateRootMetrics {
    fn clone(&self) -> Self {
        Self {
            total_triggered: Arc::clone(&self.total_triggered),
            total_computed: Arc::clone(&self.total_computed),
            total_verified: Arc::clone(&self.total_verified),
            total_verification_failures: Arc::clone(&self.total_verification_failures),
        }
    }
}

impl Default for StateRootMetrics {
    fn default() -> Self {
        Self {
            total_triggered: Arc::new(AtomicU64::new(0)),
            total_computed: Arc::new(AtomicU64::new(0)),
            total_verified: Arc::new(AtomicU64::new(0)),
            total_verification_failures: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Snapshot of async state root metrics
#[derive(Debug, Clone)]
pub struct AsyncStateRootStats {
    /// Total triggers sent
    pub total_triggered: u64,
    /// Total successful computations
    pub total_computed: u64,
    /// Total verified roots
    pub total_verified: u64,
    /// Total verification failures
    pub total_verification_failures: u64,
}

impl StateRootMetrics {
    pub fn inc_triggered(&self, _height: u64) {
        // Track per-height metrics can be added here
        self.total_triggered.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_computed(&self) {
        self.total_computed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_verified(&self, height: u64) {
        let _ = height; // Future implementation
        self.total_verified.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_verification_failure(&self, height: u64) {
        let _ = height; // Future implementation
        self.total_verification_failures
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> AsyncStateRootStats {
        AsyncStateRootStats {
            total_triggered: self.total_triggered.load(Ordering::Relaxed),
            total_computed: self.total_computed.load(Ordering::Relaxed),
            total_verified: self.total_verified.load(Ordering::Relaxed),
            total_verification_failures: self.total_verification_failures.load(Ordering::Relaxed),
        }
    }
}
