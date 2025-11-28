//! Prometheus metrics for neo-node health.
use lazy_static::lazy_static;
use neo_core::network::p2p::timeouts::TimeoutStats;
use neo_plugins::rpc_server::rpc_server::{RPC_ERR_TOTAL, RPC_REQ_TOTAL};
use prometheus::core::Collector;
use prometheus::{register_gauge, Encoder, Gauge, TextEncoder};
use sysinfo::{DiskExt, System, SystemExt};

lazy_static! {
    static ref HEADER_HEIGHT: Gauge =
        register_gauge!("neo_header_height", "Highest header seen").unwrap();
    static ref BLOCK_HEIGHT: Gauge =
        register_gauge!("neo_block_height", "Highest block persisted").unwrap();
    static ref HEADER_LAG: Gauge =
        register_gauge!("neo_header_lag", "Header lag in blocks").unwrap();
    static ref MEMPOOL_SIZE: Gauge =
        register_gauge!("neo_mempool_size", "Mempool size (transactions)").unwrap();
    static ref TIMEOUT_HANDSHAKE: Gauge =
        register_gauge!("neo_p2p_timeouts_handshake", "Handshake timeouts").unwrap();
    static ref TIMEOUT_READ: Gauge =
        register_gauge!("neo_p2p_timeouts_read", "Read timeouts").unwrap();
    static ref TIMEOUT_WRITE: Gauge =
        register_gauge!("neo_p2p_timeouts_write", "Write timeouts").unwrap();
    static ref PEER_COUNT: Gauge = register_gauge!("neo_peer_count", "Peer count").unwrap();
    static ref DISK_FREE_BYTES: Gauge =
        register_gauge!("neo_storage_free_bytes", "Free bytes on storage path disk").unwrap();
    static ref DISK_TOTAL_BYTES: Gauge = register_gauge!(
        "neo_storage_total_bytes",
        "Total bytes on storage path disk"
    )
    .unwrap();
}

pub fn update_metrics(
    block_height: u32,
    header_height: u32,
    header_lag: u32,
    mempool_size: u32,
    timeouts: TimeoutStats,
    peer_count: usize,
    storage_path: Option<&str>,
) {
    BLOCK_HEIGHT.set(block_height as f64);
    HEADER_HEIGHT.set(header_height as f64);
    HEADER_LAG.set(header_lag as f64);
    MEMPOOL_SIZE.set(mempool_size as f64);
    TIMEOUT_HANDSHAKE.set(timeouts.handshake as f64);
    TIMEOUT_READ.set(timeouts.read as f64);
    TIMEOUT_WRITE.set(timeouts.write as f64);
    PEER_COUNT.set(peer_count as f64);
    if let Some(path) = storage_path {
        if let Some((free, total)) = disk_usage_for(path) {
            DISK_FREE_BYTES.set(free as f64);
            DISK_TOTAL_BYTES.set(total as f64);
        }
    }
}

pub fn gather() -> Vec<u8> {
    let encoder = TextEncoder::new();
    let mut metric_families = prometheus::gather();
    metric_families.extend_from_slice(&[
        RPC_REQ_TOTAL.collect()[0].clone(),
        RPC_ERR_TOTAL.collect()[0].clone(),
    ]);
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap_or(());
    buffer
}

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
