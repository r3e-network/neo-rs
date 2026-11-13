//! Health endpoint and metrics pump setup.

use super::tasks::BackgroundTasks;
use crate::cli::NodeCli;
use crate::config::NodeConfig;
// use neo_system::state_service::metrics::state_root_ingest_stats; // Removed with neo-telemetry
use neo_system::NeoSystem;
use std::sync::Arc;
use sysinfo::{DiskExt, System, SystemExt};
use tokio::sync::RwLock;
use tracing::{error, info};

pub(crate) fn start_health_endpoint_if_enabled(
    tasks: &BackgroundTasks,
    cli: &NodeCli,
    node_config: &NodeConfig,
    health_state: Arc<RwLock<crate::health::HealthState>>,
) {
    let Some(health_port) = cli.health_port else {
        return;
    };

    let max_lag = cli
        .health_max_header_lag
        .unwrap_or(crate::health::DEFAULT_MAX_HEADER_LAG);
    let storage_for_health = node_config.storage_path();
    let rpc_enabled_for_health = node_config.rpc.enabled;
    let shutdown = tasks.cancellation_token();

    tasks.spawn("health endpoint", async move {
        if let Err(e) = crate::health::serve_health_with_state(
            health_port,
            max_lag,
            storage_for_health,
            rpc_enabled_for_health,
            health_state,
            async move {
                shutdown.cancelled().await;
            },
        )
        .await
        {
            error!(target: "neo", error = %e, "health endpoint failed");
        }
    });
    info!(target: "neo", port = health_port, "health endpoint started");
}

pub(crate) fn spawn_metrics_pump(
    tasks: &BackgroundTasks,
    system: Arc<NeoSystem>,
    storage_path: Option<String>,
    health_state: Arc<RwLock<crate::health::HealthState>>,
) {
    let shutdown = tasks.cancellation_token();
    let metrics_storage_path = storage_path;
    let metrics_system = system;
    let pump_health_state = health_state;

    tasks.spawn("metrics pump", async move {
        let tick = std::time::Duration::from_secs(1);
        const FAST_SYNC_ENABLE_LAG: u32 = 5_000;
        const FAST_SYNC_DISABLE_LAG: u32 = 500;
        let mut fast_sync_enabled = metrics_system.context().is_fast_sync_mode();
        loop {
            tokio::select! {
                () = tokio::time::sleep(tick) => {}
                () = shutdown.cancelled() => break,
            }

            let block_height = metrics_system.current_block_index();
            let header_height = metrics_system.ledger_context().highest_header_index();
            let header_lag = header_height.saturating_sub(block_height);
            let max_peer_height = metrics_system.max_peer_block_height().await.unwrap_or(0);
            let peer_lag = max_peer_height.saturating_sub(block_height);
            let effective_sync_lag = header_lag.max(peer_lag);
            let should_enable_fast_sync = if fast_sync_enabled {
                effective_sync_lag > FAST_SYNC_DISABLE_LAG
            } else {
                effective_sync_lag > FAST_SYNC_ENABLE_LAG
            };
            if should_enable_fast_sync != fast_sync_enabled {
                if should_enable_fast_sync {
                    metrics_system.context().enable_fast_sync_mode();
                    metrics_system.store().enable_fast_sync_mode();
                    info!(
                        target: "neo",
                        header_lag,
                        peer_lag,
                        max_peer_height,
                        "fast sync mode enabled"
                    );
                } else {
                    metrics_system.context().disable_fast_sync_mode();
                    metrics_system.store().disable_fast_sync_mode();
                    info!(
                        target: "neo",
                        header_lag,
                        peer_lag,
                        max_peer_height,
                        "fast sync mode disabled"
                    );
                }
                fast_sync_enabled = should_enable_fast_sync;
            }

            let peer_count = metrics_system.peer_count().await.unwrap_or(0);
            let (mempool_verified, mempool_unverified) = {
                // Bind the Arc before locking: the guard must not outlive the
                // temporary returned by `mempool()`.
                let pool = metrics_system.mempool();
                let pool = pool.lock();
                (pool.verified_count(), pool.unverified_count())
            };
            let mempool_size = (mempool_verified + mempool_unverified) as u32;

            // Ingest the real P2P timeout counters instead of dropping the
            // snapshot that was previously computed and discarded.
            let timeouts = neo_system::network::p2p::timeouts::stats();
            crate::metrics::update_timeout_metrics(
                peer_count,
                timeouts.handshake + timeouts.read + timeouts.write,
            );

            if let Some(storage_path) = metrics_storage_path.as_deref() {
                let storage_path = std::fs::canonicalize(storage_path)
                    .unwrap_or_else(|_| std::path::PathBuf::from(storage_path));
                let mut system = System::new();
                system.refresh_disks_list();
                if let Some(disk) = system
                    .disks()
                    .iter()
                    .filter(|disk| storage_path.starts_with(disk.mount_point()))
                    .max_by_key(|disk| disk.mount_point().as_os_str().len())
                {
                    const BYTES_PER_GB: f64 = 1024.0 * 1024.0 * 1024.0;
                    crate::metrics::update_storage_metrics(
                        disk.available_space() as f64 / BYTES_PER_GB,
                        disk.total_space() as f64 / BYTES_PER_GB,
                    );
                }
            }

            // State-service metrics (root indexes, ingest stats) were removed
            // together with neo-telemetry; there is no collection pipeline for
            // them in this build, so nothing is reported rather than
            // fabricating values.

            // Honest metrics: every reported value is backed by a real data
            // source. No verified-height tracker exists in this build, so the
            // `neo_verified_height` gauge stays at its not-collected initial
            // value instead of echoing `block_height`, which would fabricate
            // full verification at the chain tip.
            const VERIFIED_HEIGHT_NOT_COLLECTED: u32 = 0;
            crate::metrics::update_node_metrics(
                block_height,
                header_height,
                VERIFIED_HEIGHT_NOT_COLLECTED,
                mempool_verified,
                mempool_unverified,
                peer_count as usize,
                None, // timeout stats ingested via update_timeout_metrics above
                None, // state-service metrics not collected (removed with neo-telemetry)
            );

            {
                let mut state = pump_health_state.write().await;
                state.block_height = block_height;
                state.header_height = header_height;
                state.peer_count = peer_count as u32;
                state.mempool_size = mempool_size;
                state.is_syncing = metrics_system.context().is_fast_sync_mode() || header_lag > 0;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_protocol::ProtocolSettings;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread")]
    async fn metrics_pump_exits_on_background_shutdown() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
        let tasks = BackgroundTasks::new();
        let health_state = Arc::new(RwLock::new(crate::health::HealthState::default()));

        spawn_metrics_pump(&tasks, system.clone(), None, health_state);

        assert!(tasks.shutdown(Duration::from_secs(1)).await);
        system.shutdown().await.expect("system shutdown");
    }
}
