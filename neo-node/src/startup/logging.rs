//! Health endpoint and metrics pump setup.

use super::tasks::BackgroundTasks;
use crate::cli::NodeCli;
use crate::config::NodeConfig;
use crate::metrics::AccelerationSnapshot;
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
            let timeouts = neo_p2p::p2p::timeouts::stats();
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

            // Acceleration telemetry: fast-path counters, the store's read
            // cache and write buffer, the state-root node cache, and the
            // opt-in MPT timings. Every value below is read from a counter the
            // execution or persistence layer already maintains; a component
            // that is not running reports zero with its `*_enabled` gauge at
            // zero instead of a fabricated number.
            crate::metrics::update_acceleration_metrics(collect_acceleration_snapshot(
                &metrics_system,
            ));

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

/// Maps the running system's acceleration statistics into the metrics shape.
///
/// Every field comes from a counter that the execution or persistence layer
/// already maintains:
///
/// - fast-path outcome counters from [`NeoSystem::execution_stats`]
/// - the selected [`NeoSystem::execution_mode`]
/// - read cache and write buffer snapshots from the store's
///   `Store::acceleration_stats` port
/// - state-root node cache and warm-root retention from the state service
/// - block-level native `OnPersist`/`PostPersist` and MPT stage timings from
///   the same persistence profile
///
/// Nothing here is derived by inference: a component that is not registered
/// answers `None`, which the exposition reports as zero. Reading the snapshots
/// takes only atomics and short locks, so this can never influence execution
/// or consensus.
fn collect_acceleration_snapshot(system: &NeoSystem) -> AccelerationSnapshot {
    let stats = system.execution_stats();
    let profile = stats.profile_snapshot();
    let acceleration = system.store().acceleration_stats();
    let state_root_node_cache = system
        .state_commit_handlers()
        .map(|handlers| handlers.node_cache_stats());

    AccelerationSnapshot::capture(
        stats.fast_path_snapshot(),
        stats.parallel_snapshot(),
        system.execution_mode(),
        &profile,
        acceleration.as_ref(),
        state_root_node_cache,
    )
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

    #[tokio::test(flavor = "multi_thread")]
    async fn acceleration_snapshot_reads_the_live_system() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");

        let snapshot = collect_acceleration_snapshot(&system);

        // The in-memory provider runs neither a read cache nor a write buffer,
        // and no state service is registered, so every optional source is
        // absent instead of silently reporting zeros as measurements.
        assert!(snapshot.read_cache.is_none());
        assert!(snapshot.write_buffer.is_none());
        assert!(snapshot.state_root_node_cache.is_none());
        assert!(!snapshot.profiling_enabled);
        assert_eq!(
            snapshot.fast_path,
            neo_system::FastPathStatsSnapshot::default()
        );
        assert_eq!(
            snapshot.execution_mode,
            neo_system::ExecutionMode::default()
        );

        let text = snapshot.export_prometheus();
        assert!(text.contains("neo_storage_acceleration_enabled{component=\"read_cache\"} 0"));
        assert!(text.contains("neo_storage_acceleration_enabled{component=\"write_buffer\"} 0"));
        assert!(text.contains("neo_execution_profiling_enabled 0"));
        assert!(text.contains("neo_native_hook_seconds_total{phase=\"on_persist\"} 0"));

        system.shutdown().await.expect("system shutdown");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn collecting_acceleration_metrics_is_observation_only() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
        let stats = system.execution_stats();
        let before = stats.fast_path_snapshot();

        let first = collect_acceleration_snapshot(&system);
        let second = collect_acceleration_snapshot(&system);

        // Two consecutive reads observe exactly the same values, so collection
        // neither advances a counter nor consumes a one-shot snapshot.
        assert_eq!(first.fast_path, second.fast_path);
        assert_eq!(first.execution_mode, second.execution_mode);
        assert_eq!(first.mpt_seconds, second.mpt_seconds);
        assert_eq!(first.native_hook_seconds, second.native_hook_seconds);
        assert_eq!(stats.fast_path_snapshot(), before);
        assert_eq!(first.fast_path, before);
        assert_eq!(system.execution_mode(), first.execution_mode);

        system.shutdown().await.expect("system shutdown");
    }
}
