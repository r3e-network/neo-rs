//! Metrics for neo-node health and performance.
//!
//! This module provides metrics collection using the neo-telemetry collector,
//! with optional Prometheus export support for backward compatibility.

use neo_system::network::p2p::timeouts::TimeoutStats;
use neo_telemetry::collector::Telemetry;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use sysinfo::{DiskExt, System, SystemExt};

/// Global telemetry instance for the node.
pub static TELEMETRY: LazyLock<Arc<Telemetry>> =
    LazyLock::new(|| Arc::new(Telemetry::new("neo-node", env!("CARGO_PKG_VERSION"))));

static STATE_ROOT_INGEST_ACCEPTED_LAST: AtomicU64 = AtomicU64::new(0);
static STATE_ROOT_INGEST_REJECTED_LAST: AtomicU64 = AtomicU64::new(0);

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
#[allow(dead_code)] // Will be used when full metrics integration is implemented
pub fn update_metrics(
    block_height: u32,
    header_height: u32,
    _header_lag: u32,
    mempool_size: u32,
    timeouts: TimeoutStats,
    peer_count: usize,
    storage_path: Option<&str>,
    state_local_root: Option<u32>,
    state_validated_root: Option<u32>,
    _state_validated_lag: Option<u32>,
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

    // Update shared Prometheus metrics in neo-telemetry.
    neo_telemetry::update_node_metrics(
        block_height,
        header_height,
        mempool_size,
        peer_count,
        state_local_root,
        state_validated_root,
        state_root_accepted,
        state_root_rejected,
    );
    neo_telemetry::update_timeout_metrics(
        timeouts.handshake as u64,
        timeouts.read as u64,
        timeouts.write as u64,
    );

    // Increment counters based on deltas to avoid double counting.
    let prev_accepted =
        STATE_ROOT_INGEST_ACCEPTED_LAST.swap(state_root_accepted, Ordering::Relaxed);
    let prev_rejected =
        STATE_ROOT_INGEST_REJECTED_LAST.swap(state_root_rejected, Ordering::Relaxed);
    if state_root_accepted > prev_accepted {
        TELEMETRY.increment_counter_by(
            "neo_state_roots_accepted",
            state_root_accepted - prev_accepted,
        );
    }
    if state_root_rejected > prev_rejected {
        TELEMETRY.increment_counter_by(
            "neo_state_roots_rejected",
            state_root_rejected - prev_rejected,
        );
    }

    // Update storage metrics
    if let Some(path) = storage_path
        && let Some((free, total)) = disk_usage_for(path)
    {
        neo_telemetry::update_storage_metrics(free, total);
        TELEMETRY.record_storage_metrics(free, total);
    }
}

/// Gathers all metrics in Prometheus text format.
#[allow(dead_code)]
pub fn gather() -> Vec<u8> {
    neo_telemetry::gather_prometheus()
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
#[allow(dead_code)] // Will be used when full disk metrics are implemented
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

    #[test]
    fn update_metrics_delegates_prometheus_export_to_neo_telemetry() {
        update_metrics(
            100,
            105,
            5,
            12,
            TimeoutStats {
                handshake: 1,
                read: 2,
                write: 3,
            },
            8,
            None,
            Some(100),
            Some(95),
            Some(5),
            7,
            1,
        );

        let text = String::from_utf8(gather()).expect("prometheus text is UTF-8");
        assert!(text.contains("neo_block_height"));
        assert!(text.contains("neo_p2p_timeouts_handshake"));
        assert!(text.contains("neo_state_roots_accepted"));

        // Repeated totals must not double count; decreases reset the delta baseline.
        for (accepted, rejected, expected_accepted, expected_rejected) in
            [(7, 1, 7, 1), (10, 3, 10, 3), (2, 0, 10, 3), (5, 1, 13, 4)]
        {
            update_metrics(
                100,
                105,
                5,
                12,
                TimeoutStats {
                    handshake: 1,
                    read: 2,
                    write: 3,
                },
                8,
                None,
                Some(100),
                Some(95),
                Some(5),
                accepted,
                rejected,
            );

            let snapshot = TELEMETRY.snapshot();
            assert_eq!(
                snapshot.counters.get("neo_state_roots_accepted"),
                Some(&expected_accepted)
            );
            assert_eq!(
                snapshot.counters.get("neo_state_roots_rejected"),
                Some(&expected_rejected)
            );
            assert_eq!(
                snapshot.gauges.get("neo_state_roots_accepted_total"),
                Some(&(accepted as f64))
            );
            assert_eq!(
                snapshot.gauges.get("neo_state_roots_rejected_total"),
                Some(&(rejected as f64))
            );

            let json: serde_json::Value =
                serde_json::from_str(&gather_telemetry("json")).expect("valid metrics JSON");
            assert_eq!(
                json["counters"]["neo_state_roots_accepted"],
                expected_accepted
            );
            assert_eq!(
                json["counters"]["neo_state_roots_rejected"],
                expected_rejected
            );
            let prometheus = gather_telemetry("prometheus");
            assert!(prometheus.contains(&format!(
                "# TYPE neo_state_roots_accepted counter\nneo_state_roots_accepted {expected_accepted}\n"
            )));
            assert!(prometheus.contains(&format!(
                "# TYPE neo_state_roots_rejected counter\nneo_state_roots_rejected {expected_rejected}\n"
            )));
        }
    }
}
