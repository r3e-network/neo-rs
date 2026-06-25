//! Performance metrics rendering for the sync + execution pipeline.
//!
//! Reads the lock-free atomics from [`neo_runtime::sync_metrics`] and renders
//! them as Prometheus-format text for the /metrics telemetry endpoint. This is
//! the "analysis system" that continuously monitors sync and execution hot spots.
//!
//! The actual recording happens in [`neo_runtime::sync_metrics::record_block`],
//! called from the blockchain-service block-persist hot path.

/// Render sync metrics as Prometheus-format text.
pub fn render_prometheus() -> String {
    use neo_runtime::sync_metrics as m;

    let height = m::height();
    let peer_tip = m::peer_live_tip();
    let blocks = m::blocks_persisted();

    format!(
        "# HELP neo_sync_height Current block height\n\
         # TYPE neo_sync_height gauge\n\
         neo_sync_height {height}\n\
         # HELP neo_sync_peer_tip Peer-reported live chain tip\n\
         # TYPE neo_sync_peer_tip gauge\n\
         neo_sync_peer_tip {peer_tip}\n\
         # HELP neo_sync_lag Blocks behind live tip\n\
         # TYPE neo_sync_lag gauge\n\
         neo_sync_lag {}\n\
         # HELP neo_sync_blocks_persisted Total blocks persisted since startup\n\
         # TYPE neo_sync_blocks_persisted counter\n\
         neo_sync_blocks_persisted {blocks}\n\
         # HELP neo_sync_avg_total_us EWMA total per-block persist time (microseconds)\n\
         # TYPE neo_sync_avg_total_us gauge\n\
         neo_sync_avg_total_us {}\n\
         # HELP neo_sync_avg_verify_us EWMA witness verification time (microseconds)\n\
         # TYPE neo_sync_avg_verify_us gauge\n\
         neo_sync_avg_verify_us {}\n\
         # HELP neo_sync_avg_persist_us EWMA native contract execution time (microseconds)\n\
         # TYPE neo_sync_avg_persist_us gauge\n\
         neo_sync_avg_persist_us {}\n\
         # HELP neo_sync_avg_commit_us EWMA RocksDB commit time (microseconds)\n\
         # TYPE neo_sync_avg_commit_us gauge\n\
         neo_sync_avg_commit_us {}\n",
        peer_tip.saturating_sub(height),
        m::avg_total_us(),
        m::avg_verify_us(),
        m::avg_persist_us(),
        m::avg_commit_us(),
    )
}
