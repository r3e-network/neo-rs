//! Daemon shutdown workflow.
//!
//! This module coordinates the high-level graceful shutdown path after live
//! services have started: wait for a shutdown source, cancel spawned work,
//! abort remaining handles after a short grace period, flush state-service
//! writes, and restore durable store modes.

use std::sync::Arc;
use std::time::Duration;

use neo_storage::persistence::Store;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::observability::ObservabilityRuntime;
use super::services::NodeServiceHandles;
use super::shutdown::wait_for_shutdown_signal;
use super::startup_cleanup::{flush_state_service_for_shutdown, restore_durable_store_mode};

pub(in crate::node) async fn run_daemon_shutdown<S, ServiceS>(
    node: &Arc<neo_system::Node<neo_native_contracts::StandardNativeProvider, S>>,
    services: &NodeServiceHandles<S>,
    stop_at_height: Option<u32>,
    shutdown: CancellationToken,
    handles: Vec<tokio::task::JoinHandle<()>>,
    durable_service_stores: &[Arc<ServiceS>],
    observability: Option<&ObservabilityRuntime>,
) -> anyhow::Result<()>
where
    S: Store + 'static,
    ServiceS: Store + 'static,
{
    // Wait for a shutdown signal, handling SIGTERM as well as Ctrl-C (SIGINT)
    // so `kill`/`pkill`, Docker, and systemd all stop the node gracefully. The
    // validation stop-height path uses the same shutdown route after observing a
    // committed block event, so the persistent store is dropped cleanly.
    let shutdown_signalled =
        wait_for_shutdown_signal(node.blockchain(), stop_at_height, shutdown.clone()).await;

    match shutdown_signalled {
        Ok(signal_name) => {
            info!(target: "neo", signal = signal_name, "shutdown signal received; shutting down")
        }
        Err(err) => {
            warn!(target: "neo", error = %err, "shutdown-signal handler failed; falling back to pending forever");
            if let Some(observability) = observability {
                observability.report_runtime_error("shutdown_signal", &err);
            }
            std::future::pending::<()>().await;
        }
    }

    // Signal cancellation to all spawned tasks, then wait a short grace period
    // for them to observe the token and clean up before aborting. This prevents
    // the state service MPT workers from being killed mid-write and avoids
    // leaving durable store files in an inconsistent state.
    shutdown.cancel();
    // Give tasks up to 2 seconds to observe cancellation and unwind cleanly.
    tokio::time::sleep(Duration::from_secs(2)).await;

    for handle in handles {
        handle.abort();
    }
    flush_state_service_for_shutdown(services)?;
    restore_durable_store_mode(node.storage().as_ref(), durable_service_stores)?;
    Ok(())
}
