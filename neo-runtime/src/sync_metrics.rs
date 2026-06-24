//! Lock-free sync metrics, shared across crates.
//!
//! The block-persist hot path (in neo-blockchain) calls [`record_block`] to
//! feed these atomics; the telemetry layer (in neo-node) reads them for the
//! Prometheus /metrics endpoint. Using a global avoids any dependency cycle
//! (neo-blockchain → neo-runtime ← neo-node).

use std::sync::atomic::{AtomicU64, Ordering};

static BLOCKS_PERSISTED: AtomicU64 = AtomicU64::new(0);
static HEIGHT: AtomicU64 = AtomicU64::new(0);
static AVG_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static AVG_VERIFY_US: AtomicU64 = AtomicU64::new(0);
static AVG_PERSIST_US: AtomicU64 = AtomicU64::new(0);
static AVG_COMMIT_US: AtomicU64 = AtomicU64::new(0);

/// Record a block persist with per-stage timing. Called from the
/// blockchain-service hot path. Lock-free.
pub fn record_block(
    height: u64,
    verify_us: u64,
    persist_us: u64,
    commit_us: u64,
    total_us: u64,
) {
    BLOCKS_PERSISTED.fetch_add(1, Ordering::Relaxed);
    HEIGHT.store(height, Ordering::Relaxed);
    ewma(&AVG_TOTAL_US, total_us);
    ewma(&AVG_VERIFY_US, verify_us);
    ewma(&AVG_PERSIST_US, persist_us);
    ewma(&AVG_COMMIT_US, commit_us);
}

/// Current node height.
pub fn height() -> u64 {
    HEIGHT.load(Ordering::Relaxed)
}

/// Total blocks persisted since startup.
pub fn blocks_persisted() -> u64 {
    BLOCKS_PERSISTED.load(Ordering::Relaxed)
}

/// EWMA per-block total time (microseconds).
pub fn avg_total_us() -> u64 {
    AVG_TOTAL_US.load(Ordering::Relaxed)
}

/// EWMA witness-verification time (microseconds).
pub fn avg_verify_us() -> u64 {
    AVG_VERIFY_US.load(Ordering::Relaxed)
}

/// EWMA native-contract-execution time (microseconds).
pub fn avg_persist_us() -> u64 {
    AVG_PERSIST_US.load(Ordering::Relaxed)
}

/// EWMA RocksDB-commit time (microseconds).
pub fn avg_commit_us() -> u64 {
    AVG_COMMIT_US.load(Ordering::Relaxed)
}

fn ewma(slot: &AtomicU64, sample: u64) {
    let prev = slot.load(Ordering::Relaxed);
    let updated = if prev == 0 {
        sample
    } else {
        let diff = (sample as i64 - prev as i64) / 16;
        (prev as i64 + diff).max(0) as u64
    };
    slot.store(updated, Ordering::Relaxed);
}
