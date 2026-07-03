//! # neo-node::node::seeds
//!
//! Seed-node selection and network bootstrap helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `seeds`: seed-node selection and fallback lists.

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::observability::ObservabilityRuntime;
use super::tasks::{TaskKind, spawn_daemon_task_result};

pub(super) fn spawn_seed_dialing(
    seeds: Vec<String>,
    network: neo_network::NetworkHandle,
    observability: Option<ObservabilityRuntime>,
    shutdown: CancellationToken,
) -> Option<JoinHandle<()>> {
    if seeds.is_empty() {
        return None;
    }

    let task_shutdown = shutdown.clone();
    let task_observability = observability.clone();
    let task = async move {
        for seed in seeds {
            // Abort early when the node is shutting down.
            if task_shutdown.is_cancelled() {
                break;
            }
            match tokio::net::lookup_host(&seed).await {
                Ok(addrs) => {
                    dial_seed_addresses(&seed, addrs, &network, task_observability.as_ref()).await
                }
                Err(err) => {
                    warn!(target: "neo", seed = %seed, error = %err, "seed DNS resolution failed");
                    if let Some(observability) = &task_observability {
                        observability.report_runtime_error(
                            "seed_dns_resolution",
                            format_args!("{seed}: {err}"),
                        );
                    }
                }
            }
        }
        Ok(())
    };

    let mut handles = Vec::new();
    spawn_daemon_task_result(
        &mut handles,
        observability.as_ref(),
        &shutdown,
        TaskKind::Normal,
        "seed_dialer",
        task,
    );
    handles.pop()
}

async fn dial_seed_addresses(
    seed: &str,
    addrs: impl IntoIterator<Item = std::net::SocketAddr>,
    network: &neo_network::NetworkHandle,
    observability: Option<&ObservabilityRuntime>,
) {
    if let Some(addr) = addrs.into_iter().next() {
        match network.connect_peer(addr).await {
            Ok(id) => info!(target: "neo", %addr, ?id, "connected to seed"),
            Err(err) => {
                warn!(target: "neo", %addr, error = %err, "seed dial failed");
                if let Some(observability) = observability {
                    observability
                        .report_runtime_error("seed_dial", format_args!("{seed} ({addr}): {err}"));
                }
            }
        }
    } else {
        warn!(target: "neo", seed = %seed, "seed DNS resolved no addresses");
        if let Some(observability) = observability {
            observability.report_runtime_error(
                "seed_no_addresses",
                format_args!("{seed}: DNS returned no socket addresses"),
            );
        }
    }
}
