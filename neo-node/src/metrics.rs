//! Metrics for neo-node health and performance.
//!
//! Provides Prometheus-compatible metrics exposition.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::LazyLock;

/// Simple telemetry collector for node metrics
pub struct Telemetry {
    name: String,
    version: String,
    block_height: AtomicU32,
    header_height: AtomicU32,
    verified_height: AtomicU32,
    mempool_verified: AtomicUsize,
    mempool_unverified: AtomicUsize,
    connection_count: AtomicUsize,
    timeout_count: AtomicU64,
    storage_free_gb: AtomicU64, // stored as u64 (GB * 1000)
    storage_total_gb: AtomicU64,
}

impl Telemetry {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            block_height: AtomicU32::new(0),
            header_height: AtomicU32::new(0),
            verified_height: AtomicU32::new(0),
            mempool_verified: AtomicUsize::new(0),
            mempool_unverified: AtomicUsize::new(0),
            connection_count: AtomicUsize::new(0),
            timeout_count: AtomicU64::new(0),
            storage_free_gb: AtomicU64::new(0),
            storage_total_gb: AtomicU64::new(0),
        }
    }

    pub fn update_heights(&self, block: u32, header: u32, verified: u32) {
        self.block_height.store(block, Ordering::Relaxed);
        self.header_height.store(header, Ordering::Relaxed);
        self.verified_height.store(verified, Ordering::Relaxed);
    }

    pub fn update_mempool(&self, verified: usize, unverified: usize) {
        self.mempool_verified.store(verified, Ordering::Relaxed);
        self.mempool_unverified.store(unverified, Ordering::Relaxed);
    }

    pub fn update_connections(&self, count: usize) {
        self.connection_count.store(count, Ordering::Relaxed);
    }

    pub fn update_timeouts(&self, count: u64) {
        self.timeout_count.store(count, Ordering::Relaxed);
    }

    pub fn update_storage(&self, free_gb: f64, total_gb: f64) {
        self.storage_free_gb.store((free_gb * 1000.0) as u64, Ordering::Relaxed);
        self.storage_total_gb.store((total_gb * 1000.0) as u64, Ordering::Relaxed);
    }

    pub fn export_prometheus(&self) -> String {
        let block = self.block_height.load(Ordering::Relaxed);
        let header = self.header_height.load(Ordering::Relaxed);
        let verified = self.verified_height.load(Ordering::Relaxed);
        let mempool_v = self.mempool_verified.load(Ordering::Relaxed);
        let mempool_u = self.mempool_unverified.load(Ordering::Relaxed);
        let connections = self.connection_count.load(Ordering::Relaxed);
        let timeouts = self.timeout_count.load(Ordering::Relaxed);
        let free = self.storage_free_gb.load(Ordering::Relaxed) as f64 / 1000.0;
        let total = self.storage_total_gb.load(Ordering::Relaxed) as f64 / 1000.0;

        format!(
            r#"# HELP neo_node_info Node information
# TYPE neo_node_info gauge
neo_node_info{{name="{}",version="{}"}} 1

# HELP neo_block_height Current block height
# TYPE neo_block_height gauge
neo_block_height {{}} {}

# HELP neo_header_height Current header height
# TYPE neo_header_height gauge
neo_header_height {{}} {}

# HELP neo_verified_height Current verified height
# TYPE neo_verified_height gauge
neo_verified_height {{}} {}

# HELP neo_mempool_verified Verified transactions in mempool
# TYPE neo_mempool_verified gauge
neo_mempool_verified {{}} {}

# HELP neo_mempool_unverified Unverified transactions in mempool
# TYPE neo_mempool_unverified gauge
neo_mempool_unverified {{}} {}

# HELP neo_connections Active P2P connections
# TYPE neo_connections gauge
neo_connections {{}} {}

# HELP neo_timeouts_total Total timeout count
# TYPE neo_timeouts_total counter
neo_timeouts_total {{}} {}

# HELP neo_storage_free_gb Free storage space in GB
# TYPE neo_storage_free_gb gauge
neo_storage_free_gb {{}} {:.2}

# HELP neo_storage_total_gb Total storage space in GB
# TYPE neo_storage_total_gb gauge
neo_storage_total_gb {{}} {:.2}
"#,
            self.name,
            self.version,
            block,
            header,
            verified,
            mempool_v,
            mempool_u,
            connections,
            timeouts,
            free,
            total
        )
    }
}

/// Global telemetry instance for the node
pub static TELEMETRY: LazyLock<Arc<Telemetry>> =
    LazyLock::new(|| Arc::new(Telemetry::new("neo-node", env!("CARGO_PKG_VERSION"))));

/// Returns the global telemetry instance
#[allow(dead_code)]
pub fn telemetry() -> &'static Arc<Telemetry> {
    &TELEMETRY
}

/// Updates all node metrics
#[allow(clippy::too_many_arguments)]
pub fn update_node_metrics(
    block_height: u32,
    header_height: u32,
    verified_height: u32,
    mempool_verified: usize,
    mempool_unverified: usize,
    connection_count: usize,
    _timeout_stats: Option<Arc<dyn std::any::Any + Send + Sync>>,
    _state_service: Option<Arc<dyn std::any::Any + Send + Sync>>,
) {
    let tel = telemetry();
    tel.update_heights(block_height, header_height, verified_height);
    tel.update_mempool(mempool_verified, mempool_unverified);
    tel.update_connections(connection_count);
}

/// Updates timeout metrics
pub fn update_timeout_metrics(_peer_count: usize, timeout_count: usize) {
    telemetry().update_timeouts(timeout_count as u64);
}

/// Updates storage metrics
pub fn update_storage_metrics(free_gb: f64, total_gb: f64) {
    telemetry().update_storage(free_gb, total_gb);
}

/// Gathers Prometheus metrics
pub fn gather_prometheus() -> Vec<u8> {
    telemetry().export_prometheus().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_is_accessible() {
        let _ = telemetry();
    }

    #[test]
    fn update_metrics_exports_prometheus_format() {
        update_node_metrics(100, 100, 100, 50, 10, 5, None, None);
        let exported = gather_prometheus();
        let text = String::from_utf8(exported).unwrap();

        assert!(text.contains("neo_block_height"));
        assert!(text.contains("neo_mempool_verified"));
        assert!(text.contains("100"));
    }

    #[test]
    fn storage_metrics_work() {
        update_storage_metrics(50.5, 100.0);
        let exported = gather_prometheus();
        let text = String::from_utf8(exported).unwrap();

        assert!(text.contains("neo_storage_free_gb"));
        assert!(text.contains("50."));
    }
}
