//! Node-specific metrics collection
//!
//! This module provides Prometheus metrics for neo-node including:
//! - Blockchain metrics (block height, header height)
//! - Network metrics (peer count, timeouts)
//! - Mempool metrics (size)
//! - State root metrics
//! - Storage metrics (disk usage)

use lazy_static::lazy_static;
use prometheus::{Counter, Encoder, Gauge, TextEncoder};
use std::sync::atomic::{AtomicU64, Ordering};

// Blockchain metrics
lazy_static! {
    /// Current block height
    pub static ref BLOCK_HEIGHT: Gauge =
        register_gauge("neo_block_height", "Current block height");

    /// Current header height
    pub static ref HEADER_HEIGHT: Gauge =
        register_gauge("neo_header_height", "Highest header seen");

    /// Header lag (difference between header and block height)
    pub static ref HEADER_LAG: Gauge =
        register_gauge("neo_header_lag", "Header lag in blocks");
}

// Mempool metrics
lazy_static! {
    /// Mempool transaction count
    pub static ref MEMPOOL_SIZE: Gauge =
        register_gauge("neo_mempool_size", "Mempool size (transactions)");
}

// Network metrics
lazy_static! {
    /// Connected peer count
    pub static ref PEER_COUNT: Gauge =
        register_gauge("neo_peer_count", "Number of connected peers");

    /// Handshake timeouts
    pub static ref TIMEOUT_HANDSHAKE: Gauge =
        register_gauge("neo_p2p_timeouts_handshake", "Handshake timeouts");

    /// Read timeouts
    pub static ref TIMEOUT_READ: Gauge =
        register_gauge("neo_p2p_timeouts_read", "Read timeouts");

    /// Write timeouts
    pub static ref TIMEOUT_WRITE: Gauge =
        register_gauge("neo_p2p_timeouts_write", "Write timeouts");
}

// State root metrics
lazy_static! {
    /// Local state root index
    pub static ref STATE_LOCAL_ROOT_INDEX: Gauge = register_gauge(
        "neo_state_local_root_index",
        "Current local state root index (block height) if known, otherwise -1"
    );

    /// Validated state root index
    pub static ref STATE_VALIDATED_ROOT_INDEX: Gauge = register_gauge(
        "neo_state_validated_root_index",
        "Current validated state root index if known, otherwise -1"
    );

    /// State root validation lag
    pub static ref STATE_VALIDATED_LAG: Gauge = register_gauge(
        "neo_state_validated_lag",
        "Difference between local and validated state roots; -1 when unknown"
    );

    /// Total accepted state roots
    pub static ref STATE_ROOT_INGEST_ACCEPTED: Gauge = register_gauge(
        "neo_state_roots_accepted_total",
        "Total accepted state roots since process start"
    );

    /// Total rejected state roots
    pub static ref STATE_ROOT_INGEST_REJECTED: Gauge = register_gauge(
        "neo_state_roots_rejected_total",
        "Total rejected state roots since process start"
    );

    /// Counter for accepted state roots
    pub static ref STATE_ROOT_INGEST_ACCEPTED_COUNTER: Counter = register_counter(
        "neo_state_roots_accepted",
        "Counter of accepted state roots since process start"
    );

    /// Counter for rejected state roots
    pub static ref STATE_ROOT_INGEST_REJECTED_COUNTER: Counter = register_counter(
        "neo_state_roots_rejected",
        "Counter of rejected state roots since process start"
    );
}

// Storage metrics
lazy_static! {
    /// Free disk space
    pub static ref DISK_FREE_BYTES: Gauge =
        register_gauge("neo_storage_free_bytes", "Free bytes on storage path disk");

    /// Total disk space
    pub static ref DISK_TOTAL_BYTES: Gauge =
        register_gauge("neo_storage_total_bytes", "Total bytes on storage path disk");
}

// Internal tracking for deltas
static STATE_ROOT_ACCEPTED_LAST: AtomicU64 = AtomicU64::new(0);
static STATE_ROOT_REJECTED_LAST: AtomicU64 = AtomicU64::new(0);

/// Helper to create and register a gauge
fn register_gauge(name: &str, help: &str) -> Gauge {
    let gauge = Gauge::new(name, help)
        .unwrap_or_else(|_| Gauge::new("neo_invalid", "Invalid").expect("fallback"));
    let _ = prometheus::register(Box::new(gauge.clone()));
    gauge
}

/// Helper to create and register a counter
fn register_counter(name: &str, help: &str) -> Counter {
    let counter = Counter::new(name, help)
        .unwrap_or_else(|_| Counter::new("neo_invalid", "Invalid").expect("fallback"));
    let _ = prometheus::register(Box::new(counter.clone()));
    counter
}

/// Update all node metrics
///
/// This is the main entry point for updating metrics from the node runtime.
#[allow(clippy::too_many_arguments)]
pub fn update_node_metrics(
    block_height: u32,
    header_height: u32,
    mempool_size: u32,
    peer_count: usize,
    state_local_root: Option<u32>,
    state_validated_root: Option<u32>,
    state_root_accepted: u64,
    state_root_rejected: u64,
) {
    // Update blockchain metrics
    BLOCK_HEIGHT.set(block_height as f64);
    HEADER_HEIGHT.set(header_height as f64);
    HEADER_LAG.set(header_height.saturating_sub(block_height) as f64);

    // Update mempool
    MEMPOOL_SIZE.set(mempool_size as f64);

    // Update network
    PEER_COUNT.set(peer_count as f64);

    // Update state root metrics
    STATE_LOCAL_ROOT_INDEX.set(state_local_root.map(|v| v as f64).unwrap_or(-1.0));
    STATE_VALIDATED_ROOT_INDEX.set(state_validated_root.map(|v| v as f64).unwrap_or(-1.0));

    let lag = match (state_local_root, state_validated_root) {
        (Some(local), Some(validated)) => local.saturating_sub(validated) as f64,
        _ => -1.0,
    };
    STATE_VALIDATED_LAG.set(lag);

    STATE_ROOT_INGEST_ACCEPTED.set(state_root_accepted as f64);
    STATE_ROOT_INGEST_REJECTED.set(state_root_rejected as f64);

    // Update counters based on deltas
    let prev_accepted = STATE_ROOT_ACCEPTED_LAST.swap(state_root_accepted, Ordering::Relaxed);
    let prev_rejected = STATE_ROOT_REJECTED_LAST.swap(state_root_rejected, Ordering::Relaxed);

    if state_root_accepted > prev_accepted {
        STATE_ROOT_INGEST_ACCEPTED_COUNTER.inc_by((state_root_accepted - prev_accepted) as f64);
    }
    if state_root_rejected > prev_rejected {
        STATE_ROOT_INGEST_REJECTED_COUNTER.inc_by((state_root_rejected - prev_rejected) as f64);
    }
}

/// Update timeout metrics
pub fn update_timeout_metrics(handshake: u64, read: u64, write: u64) {
    TIMEOUT_HANDSHAKE.set(handshake as f64);
    TIMEOUT_READ.set(read as f64);
    TIMEOUT_WRITE.set(write as f64);
}

/// Update storage metrics
pub fn update_storage_metrics(free_bytes: u64, total_bytes: u64) {
    DISK_FREE_BYTES.set(free_bytes as f64);
    DISK_TOTAL_BYTES.set(total_bytes as f64);
}

/// Gather all metrics in Prometheus text format
pub fn gather_prometheus() -> Vec<u8> {
    // Ensure all metrics are initialized
    let _ = &*BLOCK_HEIGHT;
    let _ = &*HEADER_HEIGHT;
    let _ = &*HEADER_LAG;
    let _ = &*MEMPOOL_SIZE;
    let _ = &*PEER_COUNT;
    let _ = &*TIMEOUT_HANDSHAKE;
    let _ = &*TIMEOUT_READ;
    let _ = &*TIMEOUT_WRITE;
    let _ = &*DISK_FREE_BYTES;
    let _ = &*DISK_TOTAL_BYTES;
    let _ = &*STATE_LOCAL_ROOT_INDEX;
    let _ = &*STATE_VALIDATED_ROOT_INDEX;
    let _ = &*STATE_VALIDATED_LAG;
    let _ = &*STATE_ROOT_INGEST_ACCEPTED;
    let _ = &*STATE_ROOT_INGEST_REJECTED;

    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap_or(());
    buffer
}

/// Container for all node metrics
#[derive(Debug, Default)]
pub struct NodeMetrics {
    pub blockchain: BlockMetrics,
    pub mempool: MempoolMetrics,
    pub network: NetworkMetrics,
    pub state_root: StateRootMetrics,
    pub storage: StorageMetrics,
}

/// Blockchain-specific metrics
#[derive(Debug, Default)]
pub struct BlockMetrics {
    pub block_height: u32,
    pub header_height: u32,
}

/// Mempool metrics
#[derive(Debug, Default)]
pub struct MempoolMetrics {
    pub size: u32,
}

/// Network metrics
#[derive(Debug, Default)]
pub struct NetworkMetrics {
    pub peer_count: usize,
    pub handshake_timeouts: u64,
    pub read_timeouts: u64,
    pub write_timeouts: u64,
}

/// State root metrics
#[derive(Debug, Default)]
pub struct StateRootMetrics {
    pub local_root_index: Option<u32>,
    pub validated_root_index: Option<u32>,
    pub accepted_total: u64,
    pub rejected_total: u64,
}

/// Storage metrics
#[derive(Debug, Default)]
pub struct StorageMetrics {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

impl NodeMetrics {
    /// Update all Prometheus metrics from this snapshot
    pub fn update_prometheus(&self) {
        update_node_metrics(
            self.blockchain.block_height,
            self.blockchain.header_height,
            self.mempool.size,
            self.network.peer_count,
            self.state_root.local_root_index,
            self.state_root.validated_root_index,
            self.state_root.accepted_total,
            self.state_root.rejected_total,
        );
        update_timeout_metrics(
            self.network.handshake_timeouts,
            self.network.read_timeouts,
            self.network.write_timeouts,
        );
        update_storage_metrics(self.storage.free_bytes, self.storage.total_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_node_metrics() {
        update_node_metrics(
            100,                    // block_height
            105,                    // header_height
            50,                     // mempool_size
            10,                     // peer_count
            Some(100),              // state_local_root
            Some(95),               // state_validated_root
            1000,                   // state_root_accepted
            10,                     // state_root_rejected
        );

        assert_eq!(BLOCK_HEIGHT.get(), 100.0);
        assert_eq!(HEADER_HEIGHT.get(), 105.0);
        assert_eq!(HEADER_LAG.get(), 5.0);
    }

    #[test]
    fn test_gather_prometheus() {
        // Set a value to ensure metrics are registered
        BLOCK_HEIGHT.set(100.0);
        let output = gather_prometheus();
        let text = String::from_utf8(output).expect("valid UTF-8");
        assert!(text.contains("neo_block_height"));
    }
}
