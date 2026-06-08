//! Metrics for neo-node health and performance.
//!
//! Node metrics are collected and exposed through the shared `neo-telemetry`
//! Prometheus stack; this module wires the node's runtime values into it.

// TimeoutStats in neo-network;
use sysinfo::{DiskExt, System, SystemExt};

/// Updates all node metrics by feeding the current runtime values into the
/// shared `neo-telemetry` Prometheus gauges/counters.
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

    if let Some(path) = storage_path {
        if let Some((free, total)) = disk_usage_for(path) {
            neo_telemetry::update_storage_metrics(free, total);
       }
   }
}

/// Gathers all metrics in Prometheus text format.
#[allow(dead_code)]
pub fn gather() -> Vec<u8> {
    neo_telemetry::gather_prometheus()
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
    fn update_metrics_delegates_prometheus_export_to_neo_telemetry() {
        update_metrics(
            100,
            105,
            5,
            12,
            TimeoutStats {
                handshake: 1,
                read: 2,
                write: 3},
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
   }
}
