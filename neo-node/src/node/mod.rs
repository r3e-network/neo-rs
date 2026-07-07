//! # neo-node::node
//!
//! Daemon composition, CLI modes, and long-running node startup.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `chain_acc`: chain.acc import, reporting, and throughput accounting
//!   helpers.
//! - `cli`: Command-line arguments, ledger mode selection, and startup
//!   preflight policy.
//! - `config`: HSM provider configuration and signing profile records.
//! - `context`: Runtime context records carried through the local workflow.
//! - `fast_sync`: Built-in fast-sync package discovery, download, verification,
//!   and import flow.
//! - `indexer_runtime`: Indexer runtime wiring used by the node daemon.
//! - `inventory_relay`: Inbound peer-inventory batching and service forwarding.
//! - `ledger_source`: Local and remote ledger source abstractions used by node
//!   modes.
//! - `logging`: Logging, tracing, and operator diagnostics setup.
//! - `observability`: Metrics and observability endpoint wiring.
//! - `remote_ledger`: RPC-backed ledger source used when the node runs without
//!   a local ledger.
//! - `rpc_runtime`: RPC server runtime wiring and shutdown handling.
//! - `seeds`: Seed-node selection and network bootstrap helpers.
//! - `services`: Auxiliary service startup and handles used by the daemon.
//! - `shutdown`: OS, stop-height, and essential-task shutdown waiting.
//! - `startup_cleanup`: Startup import rollback, durable-mode restore, and
//!   shutdown flush helpers.
//! - `sync_downloader`: Coordinator-backed P2P block download startup.
//! - `sync_metrics`: Sync-speed counters, summaries, and operator-facing
//!   throughput status.
//! - `tasks`: Task supervision, shutdown wiring, and background-service
//!   handles.
//! - `telemetry`: Telemetry startup and reporting helpers.
//! - `tests`: Module-local tests and regression coverage.

use clap::Parser;
#[cfg(test)]
use neo_config::ProtocolSettings;
use std::net::SocketAddr;
#[cfg(test)]
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};

mod chain_acc;
mod cli;
mod composition;
mod config;
mod context;
mod fast_sync;
mod indexer_runtime;
mod inventory_relay;
mod ledger_source;
mod logging;
mod observability;
mod remote_ledger;
mod rpc_payload;
mod rpc_runtime;
mod seeds;
mod services;
mod shutdown;
mod startup_cleanup;
mod startup_import;
mod sync_downloader;
mod sync_metrics;
mod tasks;
mod telemetry;

#[cfg(test)]
use cli::import_tip_reaches_stop_height;
use cli::{LedgerMode, StoragePreflightMode, storage_preflight_mode, validate_cli_mode};
use composition::{RunningNode, build_node};
#[cfg(test)]
use config::{NodeConfig, open_store, validate_config};
use config::{default_p2p_port, load_config, validate_config_for_ledger_mode, validate_storage};
#[cfg(test)]
use context::DaemonContext;
#[cfg(test)]
use inventory_relay::{FAST_SYNC_BURST_CAPACITY, flush_inventory_block_batch};
use rpc_runtime::start_rpc_server;
use shutdown::wait_for_shutdown_signal;
#[cfg(test)]
use startup_cleanup::abort_fast_sync_store_mode;
use startup_cleanup::{flush_state_service_for_shutdown, restore_durable_store_mode};
use startup_import::{StartupImportContext, StartupImportOutcome, run_startup_imports};
use tasks::{TaskKind, spawn_daemon_task_result};

pub use cli::NodeCli;

/// Entry point: parse CLI, load config, build the node, start P2P +
/// RPC, and wait for `Ctrl-C`.
pub async fn run() -> anyhow::Result<()> {
    let cli = NodeCli::parse();
    validate_cli_mode(&cli)?;
    let ledger_mode = LedgerMode::from_cli(&cli);
    let (settings, config) = load_config(&cli.config, cli.network_magic)?;
    let _logging_guards = logging::init_tracing(&config.logging)?;
    let settings = Arc::new(settings);
    info!(
        target: "neo",
        network = format_args!("0x{:08X}", settings.network),
        config = %cli.config.display(),
        "loaded protocol settings"
    );
    validate_config_for_ledger_mode(&config, settings.network, ledger_mode)?;

    let check_config = cli.check_config || cli.check_all;
    let storage_preflight = storage_preflight_mode(&cli, ledger_mode);
    if check_config && storage_preflight == StoragePreflightMode::None {
        info!(target: "neo", config = %cli.config.display(), "configuration preflight passed");
        println!("configuration OK: {}", cli.config.display());
        return Ok(());
    }
    match storage_preflight {
        StoragePreflightMode::None => {}
        StoragePreflightMode::ValidateLocal => {
            validate_storage(&config, cli.storage_path.as_deref(), settings.network)?;
            info!(target: "neo", config = %cli.config.display(), "storage preflight passed");
            println!("storage OK: {}", cli.config.display());
            return Ok(());
        }
        StoragePreflightMode::SkipRemoteLedger => {
            info!(
                target: "neo::remote_ledger",
                config = %cli.config.display(),
                "storage preflight skipped; remote-ledger mode does not open a local canonical ledger"
            );
            println!(
                "storage skipped for remote ledger: {}",
                cli.config.display()
            );
            return Ok(());
        }
    }

    if check_config {
        info!(target: "neo", config = %cli.config.display(), "configuration preflight passed");
        println!("configuration OK: {}", cli.config.display());
        return Ok(());
    }

    let observability =
        observability::ObservabilityRuntime::from_config(&config.observability, settings.network)?;
    if let Some(observability) = &observability {
        observability.install_panic_hook();
    }

    let running_node = match build_node(
        Arc::clone(&settings),
        &config,
        cli.storage_path.as_deref(),
        cli.stop_at_height,
        ledger_mode,
        cli.import_chain.is_some() || cli.fast_sync,
        observability.clone(),
    )
    .await
    {
        Ok(running_node) => running_node,
        Err(err) => {
            let err = err.context("failed to construct neo-system Node");
            if let Some(observability) = &observability {
                observability.report_startup_error(&err);
            }
            return Err(err);
        }
    };
    let RunningNode {
        node,
        network,
        mut handles,
        shutdown,
        durable_service_stores,
    } = running_node;
    info!(target: "neo", "neo-system Node built; blockchain service running");

    let startup_import = run_startup_imports(StartupImportContext {
        cli: &cli,
        node: &node,
        config: &config,
        network: settings.network,
        durable_service_stores: &durable_service_stores,
        handles: &mut handles,
        observability: observability.as_ref(),
    })
    .await?;
    if startup_import == StartupImportOutcome::StopHeightReached {
        return Ok(());
    }

    match telemetry::metrics_server_task(&config.telemetry.metrics, Arc::clone(&node)) {
        Ok(Some(task)) => spawn_daemon_task_result(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Normal,
            "telemetry_metrics",
            task,
        ),
        Ok(None) => {}
        Err(err) => {
            let err = err.context("failed to start metrics endpoint");
            if let Some(observability) = &observability {
                observability.report_startup_error(&err);
            }
            for handle in handles {
                handle.abort();
            }
            return Err(err);
        }
    }

    // ----- P2P listener -----
    let p2p_port = config
        .p2p
        .port
        .unwrap_or(default_p2p_port(settings.network));
    let p2p_bind = config.p2p.bind_address.as_deref().unwrap_or("0.0.0.0");
    match format!("{p2p_bind}:{p2p_port}").parse::<SocketAddr>() {
        Ok(bind_addr) => match network.start(bind_addr).await {
            Ok(()) => info!(target: "neo", %bind_addr, "P2P listener started"),
            Err(err) => {
                warn!(target: "neo", %bind_addr, error = %err, "failed to start P2P listener");
                if let Some(observability) = &observability {
                    observability.report_runtime_error("p2p_listener", &err);
                }
            }
        },
        Err(err) => {
            warn!(target: "neo", addr = %format!("{p2p_bind}:{p2p_port}"), error = %err, "invalid P2P bind address");
            if let Some(observability) = &observability {
                observability.report_runtime_error("p2p_bind_address", &err);
            }
        }
    }

    if ledger_mode.uses_local_replay_services() {
        let seed_nodes = if config.p2p.seed_nodes.is_empty() {
            settings.seed_list.clone()
        } else {
            config.p2p.seed_nodes.clone()
        };
        if let Some(handle) = seeds::spawn_seed_dialing(
            seed_nodes,
            network.clone(),
            observability.clone(),
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

    // ----- RPC server -----
    let _rpc_keepalive = if config.rpc.enabled {
        match start_rpc_server(
            &node,
            &config,
            settings.network,
            ledger_mode.remote_endpoint(),
        ) {
            Ok(server) => Some(server),
            Err(err) => {
                let err = err.context("failed to start RPC server");
                if let Some(observability) = &observability {
                    observability.report_startup_error(&err);
                }
                for handle in handles {
                    handle.abort();
                }
                return Err(err);
            }
        }
    } else {
        info!(target: "neo", "RPC server disabled in config");
        None
    };
    if let Some(observability) = &observability {
        handles.extend(observability.spawn_heartbeat_tasks(Arc::clone(&node)));
    }

    // Wait for a shutdown signal, handling SIGTERM as well as Ctrl-C (SIGINT)
    // so `kill`/`pkill`, Docker, and systemd all stop the node gracefully. The
    // validation stop-height path uses the same shutdown route after observing a
    // committed block event, so the persistent store is dropped cleanly.
    let shutdown_signalled =
        wait_for_shutdown_signal(node.blockchain(), cli.stop_at_height, shutdown.clone()).await;

    match shutdown_signalled {
        Ok(signal_name) => {
            info!(target: "neo", signal = signal_name, "shutdown signal received; shutting down")
        }
        Err(err) => {
            warn!(target: "neo", error = %err, "shutdown-signal handler failed; falling back to pending forever");
            if let Some(observability) = &observability {
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
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    for handle in handles {
        handle.abort();
    }
    flush_state_service_for_shutdown(&node)?;
    restore_durable_store_mode(node.storage().as_ref(), &durable_service_stores)?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/node/mod.rs"]
mod tests;
