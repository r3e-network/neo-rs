//! Live daemon service startup after local catch-up imports.
//!
//! This module owns the operator-facing startup of services that should only
//! begin after storage preflight, node composition, and optional startup imports:
//! telemetry metrics, the P2P listener, seed dialing, RPC, and observability
//! heartbeats.

use std::net::SocketAddr;
use std::sync::Arc;

use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::Store;
use neo_storage::persistence::providers::RuntimeStore;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::cli::LedgerMode;
use super::config::{NodeConfig, default_p2p_port};
use super::observability::ObservabilityRuntime;
use super::rpc_runtime::start_rpc_server;
use super::seeds;
use super::services::NodeServiceHandles;
use super::tasks::{TaskKind, spawn_daemon_task_result};
use super::telemetry;

/// Handles whose lifetime keeps live services running.
pub(in crate::node) struct LiveServiceGuards {
    _rpc_keepalive: Option<Arc<parking_lot::RwLock<neo_rpc::server::RpcServer>>>,
}

/// Starts post-import live services and appends spawned task handles to
/// `handles`. On fatal startup failures, already-spawned handles are aborted
/// before returning the error, matching the daemon entrypoint's old inline
/// cleanup behavior.
pub(in crate::node) async fn start_live_services(
    node: &Arc<neo_system::Node<neo_native_contracts::StandardNativeProvider, RuntimeStore>>,
    services: &Arc<NodeServiceHandles<RuntimeStore>>,
    network: &neo_network::NetworkHandle,
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    shutdown: &CancellationToken,
    config: &NodeConfig,
    network_magic: u32,
    ledger_mode: LedgerMode<'_>,
    observability: Option<&ObservabilityRuntime>,
) -> anyhow::Result<LiveServiceGuards> {
    start_metrics_endpoint(node, services, handles, shutdown, config, observability)?;
    start_p2p_listener(network, config, network_magic, observability).await;
    start_seed_dialing(
        network,
        handles,
        shutdown,
        config,
        ledger_mode,
        node,
        observability,
    );
    let rpc_keepalive = start_rpc(
        node,
        services,
        config,
        network_magic,
        ledger_mode,
        handles,
        observability,
    )?;
    if let Some(observability) = observability {
        handles.extend(observability.spawn_heartbeat_tasks(Arc::clone(node), Arc::clone(services)));
    }
    Ok(LiveServiceGuards {
        _rpc_keepalive: rpc_keepalive,
    })
}

fn start_metrics_endpoint<P, S>(
    node: &Arc<neo_system::Node<P, S>>,
    services: &Arc<NodeServiceHandles<S>>,
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    shutdown: &CancellationToken,
    config: &NodeConfig,
    observability: Option<&ObservabilityRuntime>,
) -> anyhow::Result<()>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    match telemetry::metrics_server_task(
        &config.telemetry.metrics,
        Arc::clone(node),
        Arc::clone(services),
    ) {
        Ok(Some(task)) => {
            spawn_daemon_task_result(
                handles,
                observability,
                shutdown,
                TaskKind::Normal,
                "telemetry_metrics",
                task,
            );
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(err) => {
            let err = err.context("failed to start metrics endpoint");
            if let Some(observability) = observability {
                observability.report_startup_error(&err);
            }
            abort_handles(handles);
            Err(err)
        }
    }
}

async fn start_p2p_listener(
    network: &neo_network::NetworkHandle,
    config: &NodeConfig,
    network_magic: u32,
    observability: Option<&ObservabilityRuntime>,
) {
    let p2p_port = config.p2p.port.unwrap_or(default_p2p_port(network_magic));
    let p2p_bind = config.p2p.bind_address.as_deref().unwrap_or("0.0.0.0");
    match format!("{p2p_bind}:{p2p_port}").parse::<SocketAddr>() {
        Ok(bind_addr) => match network.start(bind_addr).await {
            Ok(()) => info!(target: "neo", %bind_addr, "P2P listener started"),
            Err(err) => {
                warn!(target: "neo", %bind_addr, error = %err, "failed to start P2P listener");
                if let Some(observability) = observability {
                    observability.report_runtime_error("p2p_listener", &err);
                }
            }
        },
        Err(err) => {
            warn!(target: "neo", addr = %format!("{p2p_bind}:{p2p_port}"), error = %err, "invalid P2P bind address");
            if let Some(observability) = observability {
                observability.report_runtime_error("p2p_bind_address", &err);
            }
        }
    }
}

fn start_seed_dialing<P, S>(
    network: &neo_network::NetworkHandle,
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    shutdown: &CancellationToken,
    config: &NodeConfig,
    ledger_mode: LedgerMode<'_>,
    node: &Arc<neo_system::Node<P, S>>,
    observability: Option<&ObservabilityRuntime>,
) where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    if ledger_mode.uses_local_replay_services() {
        let seed_nodes = if config.p2p.seed_nodes.is_empty() {
            node.settings().seed_list.clone()
        } else {
            config.p2p.seed_nodes.clone()
        };
        if let Some(handle) = seeds::spawn_seed_dialing(
            seed_nodes,
            network.clone(),
            observability.cloned(),
            shutdown.clone(),
        ) {
            handles.push(handle);
        }
    } else {
        info!(
            target: "neo::remote_ledger",
            "seed dialing disabled; remote-ledger mode does not sync blocks into a local canonical ledger"
        );
    }
}

fn start_rpc(
    node: &Arc<neo_system::Node<neo_native_contracts::StandardNativeProvider, RuntimeStore>>,
    services: &NodeServiceHandles<RuntimeStore>,
    config: &NodeConfig,
    network_magic: u32,
    ledger_mode: LedgerMode<'_>,
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&ObservabilityRuntime>,
) -> anyhow::Result<Option<Arc<parking_lot::RwLock<neo_rpc::server::RpcServer>>>> {
    if !config.rpc.enabled {
        info!(target: "neo", "RPC server disabled in config");
        return Ok(None);
    }

    match start_rpc_server(
        node,
        services,
        config,
        network_magic,
        ledger_mode.remote_endpoint(),
    ) {
        Ok(server) => Ok(Some(server)),
        Err(err) => {
            let err = err.context("failed to start RPC server");
            if let Some(observability) = observability {
                observability.report_startup_error(&err);
            }
            abort_handles(handles);
            Err(err)
        }
    }
}

fn abort_handles(handles: &mut Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles.drain(..) {
        handle.abort();
    }
}
