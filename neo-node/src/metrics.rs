//! Metrics for neo-node health and performance.
//!
//! Provides Prometheus-compatible metrics exposition.
//!
//! # Acceleration telemetry
//!
//! The acceleration structures below already compute their own statistics in
//! the crates that own them; this module only aggregates snapshots into the
//! exposition text and never derives a number that no counter produced:
//!
//! | Family prefix | Source |
//! | --- | --- |
//! | `neo_fast_path_*` | [`FastPathStatsSnapshot`] from `neo-system` |
//! | `neo_storage_read_cache_*`, `neo_storage_write_buffer_*` | [`StoreAccelerationStats`] from `neo-storage` |
//! | `neo_state_root_*` | `StateRootNodeCacheStats` from `neo-state-service` |
//! | `neo_mpt_stage_seconds_total` | [`ExecutionProfileSnapshot`] from `neo-system` |
//! | `neo_native_hook_*` | [`ExecutionProfileSnapshot`] native hook phases from `neo-system` |
//!
//! Every family uses bounded labels (fixed value sets declared by the enums
//! below), and every unit is one of counts, bytes, or seconds. Capturing a
//! snapshot only reads atomics and short-lived locks, so telemetry can never
//! change an execution or consensus decision.

use std::sync::{Arc, LazyLock};

mod acceleration;
mod enums;
mod prometheus;
mod telemetry;

#[cfg(test)]
mod tests;

pub use acceleration::{AccelerationSnapshot, update_acceleration_metrics};
pub use prometheus::gather_prometheus;
pub use telemetry::Telemetry;

/// Global telemetry instance for the node
pub static TELEMETRY: LazyLock<Arc<Telemetry>> =
    LazyLock::new(|| Arc::new(Telemetry::new("neo-node", env!("CARGO_PKG_VERSION"))));

/// Returns the global telemetry instance
#[allow(dead_code)]
pub fn telemetry() -> &'static Arc<Telemetry> {
    &TELEMETRY
}

/// Updates all node metrics
pub fn update_node_metrics(
    block_height: u32,
    header_height: u32,
    verified_height: u32,
    mempool_verified: usize,
    mempool_unverified: usize,
    connection_count: usize,
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
