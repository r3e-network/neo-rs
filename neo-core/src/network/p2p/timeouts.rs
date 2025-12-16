//! Shared timeout counters for P2P operations (handshake/read/write).
use std::sync::atomic::{AtomicUsize, Ordering};

static HANDSHAKE_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);
static READ_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);
static WRITE_TIMEOUTS: AtomicUsize = AtomicUsize::new(0);

/// Increment the handshake timeout counter.
pub fn inc_handshake_timeout() {
    HANDSHAKE_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
}

/// Increment the read timeout counter.
pub fn inc_read_timeout() {
    READ_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
}

/// Increment the write timeout counter.
pub fn inc_write_timeout() {
    WRITE_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
}

/// Snapshot of timeout counters (best-effort, relaxed ordering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutStats {
    pub handshake: usize,
    pub read: usize,
    pub write: usize,
}

/// Returns the current timeout counters.
pub fn stats() -> TimeoutStats {
    TimeoutStats {
        handshake: HANDSHAKE_TIMEOUTS.load(Ordering::Relaxed),
        read: READ_TIMEOUTS.load(Ordering::Relaxed),
        write: WRITE_TIMEOUTS.load(Ordering::Relaxed),
    }
}

/// Emits timeout stats via tracing for observability.
pub fn log_stats() {
    let stats = stats();
    tracing::info!(
        target: "neo",
        handshake_timeouts = stats.handshake,
        read_timeouts = stats.read,
        write_timeouts = stats.write,
        "timeout counters snapshot"
    );
}

// Note: no test-only reset helper is kept here to avoid unused dead code in the crate;
// tests can snapshot counters via `stats()` if they need to assert behavior.
