//! Metrics for neo-node health and performance.
//!
//! This module provides metrics collection using the neo-core telemetry system,
//! with optional Prometheus export support for backward compatibility.

use lazy_static::lazy_static;
use neo_core::network::p2p::timeouts::TimeoutStats;
use neo_core::telemetry::Telemetry;
// NOTE: RPC metrics temporarily stubbed during refactoring
// use neo_plugins::rpc_server::rpc_server::{RPC_ERR_TOTAL, RPC_REQ_TOTAL};
use prometheus::{Counter, Encoder, Gauge, TextEncoder};
use std::sync::Arc;
use sysinfo::{DiskExt, System, SystemExt};

fn register_gauge_best_effort(name: &str, help: &str) -> Gauge {
    let gauge = Gauge::new(name, help)
        .unwrap_or_else(|_| Gauge::new("neo_invalid_metric", "Invalid").unwrap());
    let _ = prometheus::register(Box::new(gauge.clone()));
    gauge
}

fn register_counter_best_effort(name: &str, help: &str) -> Counter {
    let counter = Counter::new(name, help)
        .unwrap_or_else(|_| Counter::new("neo_invalid_counter", "Invalid").unwrap());
    let _ = prometheus::register(Box::new(counter.clone()));
    counter
}

lazy_static! {
    /// Global telemetry instance for the node.
    pub static ref TELEMETRY: Arc<Telemetry> = Arc::new(Telemetry::new("neo-node", env!("CARGO_PKG_VERSION")));

    // RPC metrics (stubbed until neo-rpc is fully refactored)
    pub static ref RPC_REQ_TOTAL: Counter = register_counter_best_effort("neo_rpc_requests_total", "Total RPC requests");
    pub static ref RPC_ERR_TOTAL: Counter = register_counter_best_effort("neo_rpc_errors_total", "Total RPC errors");

    // Prometheus gauges for backward compatibility
    static ref HEADER_HEIGHT: Gauge =
        register_gauge_best_effort("neo_header_height", "Highest header seen");
    static ref BLOCK_HEIGHT: Gauge =
        register_gauge_best_effort("neo_block_height", "Highest block persisted");
    static ref HEADER_LAG: Gauge =
        register_gauge_best_effort("neo_header_lag", "Header lag in blocks");
    static ref MEMPOOL_SIZE: Gauge =
        register_gauge_best_effort("neo_mempool_size", "Mempool size (transactions)");
    static ref TIMEOUT_HANDSHAKE: Gauge =
        register_gauge_best_effort("neo_p2p_timeouts_handshake", "Handshake timeouts");
    static ref TIMEOUT_READ: Gauge =
        register_gauge_best_effort("neo_p2p_timeouts_read", "Read timeouts");
    static ref TIMEOUT_WRITE: Gauge =
        register_gauge_best_effort("neo_p2p_timeouts_write", "Write timeouts");
    static ref PEER_COUNT: Gauge = register_gauge_best_effort("neo_peer_count", "Peer count");
    static ref DISK_FREE_BYTES: Gauge =
        register_gauge_best_effort("neo_storage_free_bytes", "Free bytes on storage path disk");
    static ref DISK_TOTAL_BYTES: Gauge = register_gauge_best_effort(
        "neo_storage_total_bytes",
        "Total bytes on storage path disk",
    );
    static ref STATE_LOCAL_ROOT_INDEX: Gauge = register_gauge_best_effort(
        "neo_state_local_root_index",
        "Current local state root index (block height) if known, otherwise -1",
    );
    static ref STATE_VALIDATED_ROOT_INDEX: Gauge = register_gauge_best_effort(
        "neo_state_validated_root_index",
        "Current validated state root index if known, otherwise -1",
    );
    static ref STATE_VALIDATED_LAG: Gauge = register_gauge_best_effort(
        "neo_state_validated_lag",
        "Difference between local and validated state roots; -1 when unknown",
    );
    static ref STATE_ROOT_INGEST_ACCEPTED: Gauge = register_gauge_best_effort(
        "neo_state_roots_accepted_total",
        "Total accepted state roots since process start",
    );
    static ref STATE_ROOT_INGEST_REJECTED: Gauge = register_gauge_best_effort(
        "neo_state_roots_rejected_total",
        "Total rejected state roots since process start",
    );
    static ref STATE_ROOT_INGEST_ACCEPTED_COUNTER: Counter = register_counter_best_effort(
        "neo_state_roots_accepted",
        "Counter of accepted state roots since process start",
    );
    static ref STATE_ROOT_INGEST_REJECTED_COUNTER: Counter = register_counter_best_effort(
        "neo_state_roots_rejected",
        "Counter of rejected state roots since process start",
    );
    static ref STATE_ROOT_INGEST_ACCEPTED_LAST: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    static ref STATE_ROOT_INGEST_REJECTED_LAST: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
}

/// Returns the global telemetry instance.
#[allow(dead_code)]
pub fn telemetry() -> &'static Arc<Telemetry> {
    &TELEMETRY
}

/// Updates all node metrics.
///
/// This function updates both the internal telemetry system and the Prometheus
/// gauges for backward compatibility with existing monitoring infrastructure.
#[allow(clippy::too_many_arguments)]
pub fn update_metrics(
    block_height: u32,
    header_height: u32,
    header_lag: u32,
    mempool_size: u32,
    timeouts: TimeoutStats,
    peer_count: usize,
    storage_path: Option<&str>,
    state_local_root: Option<u32>,
    state_validated_root: Option<u32>,
    state_validated_lag: Option<u32>,
    state_root_accepted: u64,
    state_root_rejected: u64,
) {
    // Update telemetry system
    TELEMETRY.record_blockchain_metrics(block_height, header_height, mempool_size, peer_count);
    TELEMETRY.record_timeout_stats(
        timeouts.handshake as u64,
        timeouts.read as u64,
        timeouts.write as u64,
    );
    TELEMETRY.record_state_metrics(
        state_local_root,
        state_validated_root,
        state_root_accepted,
        state_root_rejected,
    );

    // Update Prometheus gauges for backward compatibility
    BLOCK_HEIGHT.set(block_height as f64);
    HEADER_HEIGHT.set(header_height as f64);
    HEADER_LAG.set(header_lag as f64);
    MEMPOOL_SIZE.set(mempool_size as f64);
    TIMEOUT_HANDSHAKE.set(timeouts.handshake as f64);
    TIMEOUT_READ.set(timeouts.read as f64);
    TIMEOUT_WRITE.set(timeouts.write as f64);
    PEER_COUNT.set(peer_count as f64);
    STATE_LOCAL_ROOT_INDEX.set(state_local_root.map(|v| v as f64).unwrap_or(-1.0));
    STATE_VALIDATED_ROOT_INDEX.set(state_validated_root.map(|v| v as f64).unwrap_or(-1.0));
    STATE_VALIDATED_LAG.set(state_validated_lag.map(|v| v as f64).unwrap_or(-1.0));
    STATE_ROOT_INGEST_ACCEPTED.set(state_root_accepted as f64);
    STATE_ROOT_INGEST_REJECTED.set(state_root_rejected as f64);

    // Increment counters based on deltas to avoid double counting.
    let prev_accepted = STATE_ROOT_INGEST_ACCEPTED_LAST
        .swap(state_root_accepted, std::sync::atomic::Ordering::Relaxed);
    let prev_rejected = STATE_ROOT_INGEST_REJECTED_LAST
        .swap(state_root_rejected, std::sync::atomic::Ordering::Relaxed);
    if state_root_accepted > prev_accepted {
        STATE_ROOT_INGEST_ACCEPTED_COUNTER.inc_by((state_root_accepted - prev_accepted) as f64);
        TELEMETRY.increment_counter_by(
            "neo_state_roots_accepted",
            state_root_accepted - prev_accepted,
        );
    }
    if state_root_rejected > prev_rejected {
        STATE_ROOT_INGEST_REJECTED_COUNTER.inc_by((state_root_rejected - prev_rejected) as f64);
        TELEMETRY.increment_counter_by(
            "neo_state_roots_rejected",
            state_root_rejected - prev_rejected,
        );
    }

    // Update storage metrics
    if let Some(path) = storage_path {
        if let Some((free, total)) = disk_usage_for(path) {
            DISK_FREE_BYTES.set(free as f64);
            DISK_TOTAL_BYTES.set(total as f64);
            TELEMETRY.record_storage_metrics(free, total);
        }
    }
}

/// Gathers all metrics in Prometheus text format.
pub fn gather() -> Vec<u8> {
    let _ = &*HEADER_HEIGHT;
    let _ = &*BLOCK_HEIGHT;
    let _ = &*HEADER_LAG;
    let _ = &*MEMPOOL_SIZE;
    let _ = &*TIMEOUT_HANDSHAKE;
    let _ = &*TIMEOUT_READ;
    let _ = &*TIMEOUT_WRITE;
    let _ = &*PEER_COUNT;
    let _ = &*DISK_FREE_BYTES;
    let _ = &*DISK_TOTAL_BYTES;
    let _ = &*STATE_LOCAL_ROOT_INDEX;
    let _ = &*STATE_VALIDATED_ROOT_INDEX;
    let _ = &*STATE_VALIDATED_LAG;
    let _ = &*STATE_ROOT_INGEST_ACCEPTED;
    let _ = &*STATE_ROOT_INGEST_REJECTED;
    let _ = &*STATE_ROOT_INGEST_ACCEPTED_COUNTER;
    let _ = &*STATE_ROOT_INGEST_REJECTED_COUNTER;
    let _ = &*RPC_REQ_TOTAL;
    let _ = &*RPC_ERR_TOTAL;

    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap_or(());
    buffer
}

/// Gathers metrics from the telemetry system in the specified format.
///
/// Supported formats: "prometheus", "json"
#[allow(dead_code)]
pub fn gather_telemetry(format: &str) -> String {
    let snapshot = TELEMETRY.snapshot();
    match format {
        "json" => snapshot.to_json(),
        _ => snapshot.to_prometheus_text(),
    }
}

/// Returns disk usage (free, total) for the given path.
fn disk_usage_for(path: &str) -> Option<(u64, u64)> {
    let mut system = System::new();
    system.refresh_disks_list();
    let disks = system.disks();
    let path = std::path::Path::new(path);
    let mut best: Option<(usize, u64, u64)> = None; // mount_len, free, total

    for disk in disks {
        let mount = disk.mount_point();
        if path.starts_with(mount) {
            let mount_len = mount.as_os_str().len();
            let free = disk.available_space();
            let total = disk.total_space();
            if best.map(|(len, _, _)| mount_len > len).unwrap_or(true) {
                best = Some((mount_len, free, total));
            }
        }
    }

    best.map(|(_, free, total)| (free, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_instance_is_initialized() {
        let t = telemetry();
        assert_eq!(t.service_name(), "neo-node");
    }

    #[test]
    fn gather_telemetry_returns_json() {
        // Record some metrics first
        TELEMETRY.record_gauge("test_metric", 42.0);

        let json = gather_telemetry("json");
        assert!(json.contains("gauges"));
    }

    #[test]
    fn gather_telemetry_returns_prometheus() {
        TELEMETRY.record_gauge("test_prom_metric", 100.0);

        let prom = gather_telemetry("prometheus");
        assert!(prom.contains("test_prom_metric"));
    }
}
