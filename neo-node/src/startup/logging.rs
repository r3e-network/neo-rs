//! Health endpoint and metrics pump setup.

use super::tasks::BackgroundTasks;
use crate::cli::NodeCli;
use crate::config::NodeConfig;
use neo_core::neo_system::NeoSystem;
use neo_core::state_service::metrics::state_root_ingest_stats;
use std::sync::Arc;
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
                _ = tokio::time::sleep(tick) => {}
                _ = shutdown.cancelled() => break,
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
            let mempool_size = metrics_system.mempool().lock().count() as u32;

            let timeouts = neo_core::network::p2p::timeouts::stats();

            let (state_local_root, state_validated_root) = match metrics_system.state_store() {
                Ok(Some(store)) => (store.local_root_index(), store.validated_root_index()),
                _ => (None, None),
            };
            let state_validated_lag = match (state_local_root, state_validated_root) {
                (Some(local), Some(validated)) => Some(local.saturating_sub(validated)),
                _ => None,
            };

            let ingest = state_root_ingest_stats();

            crate::metrics::update_metrics(
                block_height,
                header_height,
                header_lag,
                mempool_size,
                timeouts,
                peer_count,
                metrics_storage_path.as_deref(),
                state_local_root,
                state_validated_root,
                state_validated_lag,
                ingest.accepted,
                ingest.rejected,
            );

            {
                let mut state = pump_health_state.write().await;
                state.block_height = block_height;
                state.header_height = header_height;
                state.peer_count = peer_count;
                state.mempool_size = mempool_size;
                state.is_syncing = header_lag > 0;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::protocol_settings::ProtocolSettings;
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
