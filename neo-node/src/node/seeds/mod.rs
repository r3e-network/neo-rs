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
use tracing::{info, warn};

use super::observability::ObservabilityRuntime;

pub(super) fn spawn_seed_dialing(
    seeds: Vec<String>,
    network: neo_network::NetworkHandle,
    observability: Option<ObservabilityRuntime>,
) -> Option<JoinHandle<()>> {
    if seeds.is_empty() {
        return None;
    }

    Some(tokio::spawn(async move {
        for seed in seeds {
            match tokio::net::lookup_host(&seed).await {
                Ok(addrs) => {
                    dial_seed_addresses(&seed, addrs, &network, observability.as_ref()).await
                }
                Err(err) => {
                    warn!(target: "neo", seed = %seed, error = %err, "seed DNS resolution failed");
                    if let Some(observability) = &observability {
                        observability.report_runtime_error(
                            "seed_dns_resolution",
                            format_args!("{seed}: {err}"),
                        );
                    }
                }
            }
        }
    }))
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
