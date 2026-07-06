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
//! - `sync_metrics`: Sync-speed counters, summaries, and operator-facing
//!   throughput status.
//! - `tasks`: Task supervision, shutdown wiring, and background-service
//!   handles.
//! - `telemetry`: Telemetry startup and reporting helpers.
//! - `tests`: Module-local tests and regression coverage.

use clap::Parser;
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::{NativeContractLookup, NativeContractProvider};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

mod chain_acc;
mod cli;
mod config;
mod context;
mod fast_sync;
mod indexer_runtime;
mod inventory_relay;
mod ledger_source;
mod logging;
mod observability;
mod remote_ledger;
mod rpc_runtime;
mod seeds;
mod services;
mod shutdown;
mod sync_metrics;
mod tasks;
mod telemetry;

use cli::{
    LedgerMode, StoragePreflightMode, import_tip_reaches_stop_height, storage_preflight_mode,
    validate_cli_mode,
};
#[cfg(test)]
use config::validate_config;
use config::{
    NodeConfig, default_p2p_port, load_config, open_memory_store, open_store,
    service_store_provider, validate_config_for_ledger_mode, validate_state_service_storage,
    validate_storage,
};
use context::DaemonContext;
use inventory_relay::{
    FAST_SYNC_BLOCK_BATCH_FLUSH_MS, FAST_SYNC_BLOCK_BATCH_SIZE, FAST_SYNC_BURST_CAPACITY,
    flush_inventory_block_batch, handle_inbound_inventory_item,
};
use ledger_source::{LedgerBlockSource, RpcLedgerBlockSource};
use remote_ledger::RemoteLedgerStatus;
use rpc_runtime::start_rpc_server;
use services::OperationalServices;
use shutdown::wait_for_shutdown_signal;
use tasks::{TaskKind, spawn_daemon_task, spawn_daemon_task_result};

pub use cli::NodeCli;

fn flush_state_service_for_shutdown(node: &neo_system::Node) -> anyhow::Result<()> {
    if let Some(state_service) =
        node.get_service::<neo_state_service::commit_handlers::StateServiceCommitHandlers>()
    {
        flush_state_service(&state_service)?;
    }
    Ok(())
}

fn flush_state_service(
    state_service: &neo_state_service::commit_handlers::StateServiceCommitHandlers,
) -> anyhow::Result<()> {
    state_service
        .flush_result()
        .map_err(|err| anyhow::anyhow!("state service MPT worker failed during flush: {err}"))
}

fn restore_durable_store_mode(
    chain_store: &dyn neo_storage::persistence::store::Store,
    service_stores: &[Arc<dyn neo_storage::persistence::store::Store>],
) -> anyhow::Result<()> {
    if let Some(fs) = chain_store.as_fast_sync_store() {
        fs.disable_fast_sync_mode();
    }
    chain_store
        .flush()
        .map_err(|err| anyhow::anyhow!("flushing chain store after fast-sync mode: {err}"))?;
    for store in service_stores {
        if let Some(fs) = store.as_fast_sync_store() {
            fs.disable_fast_sync_mode();
        }
        store
            .flush()
            .map_err(|err| anyhow::anyhow!("flushing service store after fast-sync mode: {err}"))?;
    }
    Ok(())
}

fn abort_fast_sync_store_mode(
    chain_store: &dyn neo_storage::persistence::store::Store,
    service_stores: &[Arc<dyn neo_storage::persistence::store::Store>],
) {
    if let Some(fs) = chain_store.as_fast_sync_store() {
        fs.discard_pending_fast_sync_writes();
        fs.disable_fast_sync_mode();
    }
    for store in service_stores {
        if let Some(fs) = store.as_fast_sync_store() {
            fs.discard_pending_fast_sync_writes();
            fs.disable_fast_sync_mode();
        }
    }
}

fn abort_startup_after_import_failure(
    node: &neo_system::Node,
    durable_service_stores: &[Arc<dyn neo_storage::persistence::store::Store>],
    handles: Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&observability::ObservabilityRuntime>,
    operation: &'static str,
    err: anyhow::Error,
) -> anyhow::Error {
    let mut cleanup_errors = Vec::new();
    if let Err(cleanup_err) = flush_state_service_for_shutdown(node) {
        cleanup_errors.push(format!("state-service flush failed: {cleanup_err:#}"));
    }
    abort_fast_sync_store_mode(node.storage().as_ref(), durable_service_stores);
    for handle in handles {
        handle.abort();
    }

    let mut message = format!(
        "{operation} failed; startup aborted to avoid continuing with a partial local ledger"
    );
    if !cleanup_errors.is_empty() {
        message.push_str("; cleanup errors: ");
        message.push_str(&cleanup_errors.join("; "));
    }
    let failure = err.context(message);
    if let Some(observability) = observability {
        observability.report_startup_error(&failure);
    }
    failure
}

/// The composed, running node and the handles that keep it alive.
struct RunningNode {
    node: Arc<neo_system::Node>,
    network: neo_network::NetworkHandle,
    handles: Vec<tokio::task::JoinHandle<()>>,
    shutdown: CancellationToken,
    durable_service_stores: Vec<Arc<dyn neo_storage::persistence::store::Store>>,
}

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

    // Optional: import blocks from a chain.acc file before starting live sync.
    if let Some(import_path) = &cli.import_chain {
        let blockchain = node.blockchain();
        match chain_acc::import_chain_acc_until_height(
            &blockchain,
            import_path,
            false,
            cli.stop_at_height,
            Some(node.storage()),
        )
        .await
        {
            Ok(count) => {
                restore_durable_store_mode(node.storage().as_ref(), &durable_service_stores)?;
                info!(
                    target: "neo",
                    imported = count,
                    "chain.acc import completed successfully; continuing with network sync"
                );
                match blockchain.get_height().await {
                    Ok(height) if import_tip_reaches_stop_height(height, cli.stop_at_height) => {
                        info!(
                            target: "neo",
                            height,
                            stop_at_height = cli.stop_at_height.unwrap_or_default(),
                            imported = count,
                            "chain.acc import reached stop height; shutting down"
                        );
                        flush_state_service_for_shutdown(&node)?;
                        restore_durable_store_mode(
                            node.storage().as_ref(),
                            &durable_service_stores,
                        )?;
                        for handle in handles {
                            handle.abort();
                        }
                        return Ok(());
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!(
                            target: "neo",
                            error = %err,
                            "failed to read chain height after chain.acc import; continuing with network sync"
                        );
                    }
                }
            }
            Err(err) => {
                return Err(abort_startup_after_import_failure(
                    &node,
                    &durable_service_stores,
                    std::mem::take(&mut handles),
                    observability.as_ref(),
                    "chain.acc import",
                    err,
                ));
            }
        }
    }
    if cli.fast_sync {
        let blockchain = node.blockchain();
        match fast_sync::run_fast_sync_report(
            &blockchain,
            node.storage(),
            &config,
            cli.storage_path.as_deref(),
            cli.fast_sync_cache.as_deref(),
            settings.network,
            cli.stop_at_height,
            cli.fast_sync_reference_rpc.as_deref(),
            node.state_store().as_ref(),
            node.get_service::<neo_state_service::commit_handlers::StateServiceCommitHandlers>()
                .as_ref(),
        )
        .await
        {
            Ok(report) => {
                if let Some(path) = &cli.fast_sync_report {
                    fast_sync::write_fast_sync_report_sidecar(path, &report)?;
                }
                restore_durable_store_mode(node.storage().as_ref(), &durable_service_stores)?;
                let count = report.import.imported_blocks;
                info!(
                    target: "neo::fast_sync",
                    imported = count,
                    package = %report.package.filename,
                    end_height = report.package.end_height,
                    average_blocks_per_second = report.import.average_blocks_per_second,
                    throughput_status = ?report.import.throughput_status,
                    "fast-sync package import completed successfully; continuing with network sync"
                );
                match blockchain.get_height().await {
                    Ok(height) if import_tip_reaches_stop_height(height, cli.stop_at_height) => {
                        info!(
                            target: "neo::fast_sync",
                            height,
                            stop_at_height = cli.stop_at_height.unwrap_or_default(),
                            imported = count,
                            "fast-sync import reached stop height; shutting down"
                        );
                        flush_state_service_for_shutdown(&node)?;
                        restore_durable_store_mode(
                            node.storage().as_ref(),
                            &durable_service_stores,
                        )?;
                        for handle in handles {
                            handle.abort();
                        }
                        return Ok(());
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!(
                            target: "neo::fast_sync",
                            error = %err,
                            "failed to read chain height after fast-sync import; continuing with network sync"
                        );
                    }
                }
            }
            Err(err) => {
                return Err(abort_startup_after_import_failure(
                    &node,
                    &durable_service_stores,
                    std::mem::take(&mut handles),
                    observability.as_ref(),
                    "fast-sync package import",
                    err,
                ));
            }
        }
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

/// Constructs the [`neo_system::Node`] with a live blockchain service
/// and a spawned [`neo_network::LocalNodeService`].
async fn build_node(
    settings: Arc<ProtocolSettings>,
    config: &NodeConfig,
    storage_override: Option<&Path>,
    stop_at_height: Option<u32>,
    ledger_mode: LedgerMode<'_>,
    startup_bulk_import: bool,
    observability: Option<observability::ObservabilityRuntime>,
) -> anyhow::Result<RunningNode> {
    use neo_blockchain::service::BlockchainService;
    use neo_blockchain::{HeaderCache, LedgerContext};
    use neo_storage::persistence::StoreCache;

    // ----- storage backend -----
    let store: Arc<dyn neo_storage::persistence::store::Store> = match ledger_mode {
        LedgerMode::Local => open_store(config, storage_override)?,
        LedgerMode::RemoteRpc { .. } => {
            info!(
                target: "neo::remote_ledger",
                "using ephemeral in-memory chain context; configured local ledger store will not be opened"
            );
            open_memory_store()?
        }
    };

    // Enable backend-specific fast-sync optimizations during initial catch-up
    // for higher write throughput. The node re-enables durable mode once it
    // approaches the live tip.
    let durable_tip_index = {
        let probe = StoreCache::new_from_store(Arc::clone(&store), false);
        neo_native_contracts::LedgerContract::new()
            .current_index(probe.data_cache())
            .ok()
    };
    let service_storage_provider = service_store_provider(config)?;
    validate_state_service_storage(
        config,
        settings.network,
        durable_tip_index,
        &service_storage_provider,
    )?;
    let durable_tip_height = durable_tip_index.unwrap_or(0);
    let use_fast_sync_store_mode = ledger_mode.uses_local_replay_services()
        && (durable_tip_height == 0 || startup_bulk_import);
    if use_fast_sync_store_mode {
        info!(
            target: "neo::sync",
            startup_bulk_import,
            durable_tip_height,
            "enabling fast-sync store mode for initial catch-up (WAL disabled, auto-compaction off)"
        );
        if let Some(fs) = store.as_fast_sync_store() {
            fs.enable_fast_sync_mode();
        }
    }

    // Native dispatch must be available before genesis initialization, and the
    // composed Node should expose the same provider object. Build it once here,
    // install it for the legacy neo-execution lookup seam, then hand the same
    // Arc to NodeBuilder instead of letting the builder create a second one.
    let native_contract_provider = Arc::new(neo_native_contracts::StandardNativeProvider::new())
        as Arc<dyn NativeContractProvider>;
    NativeContractLookup::install_provider(Arc::clone(&native_contract_provider));

    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    // The consensus driver now mints a fresh snapshot from the store at the start
    // of each round (see ConsensusDriver::fresh_round_snapshot), so it takes the
    // store handle directly rather than a frozen startup snapshot.
    // The durable tip at startup, read before the snapshot is moved into the
    // service contexts; used to seed the advertised height / sync cursor.
    let durable_tip = neo_native_contracts::LedgerContract::new()
        .current_index(&snapshot)
        .unwrap_or(0);

    let mempool = Arc::new(neo_mempool::MemoryPool::new(&settings));
    let header_cache = Arc::new(HeaderCache::default());
    // Seed the in-memory ledger tip from the durable store so a node restarted
    // on a populated chain accepts the next block (`index == current_height + 1`)
    // instead of parking every incoming block as "ahead of tip" (which would
    // stall sync at the persisted height after a restart).
    let ledger_ctx = Arc::new(LedgerContext::default());
    if durable_tip > 0 {
        ledger_ctx.record_tip(durable_tip);
    }

    let OperationalServices {
        state_store,
        state_service,
        indexer_service,
        application_logs_service,
        tokens_tracker_service,
        tokens_tracker_runtime,
        durable_stores,
    } = services::build_operational_services(
        config,
        settings.network,
        ledger_mode.uses_local_replay_services(),
        use_fast_sync_store_mode,
    )?;

    // A second handle on the shared snapshot serves peers' block requests, and
    // the shared mempool answers `Inv`/`Mempool`/`GetData` for unconfirmed txs.
    let mut advertised_tip = durable_tip;
    let mut remote_advertised_tip = None;
    let mut remote_tip_error = None;
    let block_source: Arc<dyn neo_network::BlockSource> = match ledger_mode {
        LedgerMode::RemoteRpc { endpoint } => {
            let source = RpcLedgerBlockSource::new(endpoint.to_string(), Arc::clone(&mempool))?;
            match source.remote_tip_height() {
                Ok(height) => {
                    advertised_tip = height;
                    remote_advertised_tip = Some(height);
                }
                Err(err) => {
                    remote_tip_error = Some(err.to_string());
                    warn!(
                        target: "neo::remote_ledger",
                        endpoint,
                        error = %err,
                        "failed to read remote ledger tip height; advertising local height"
                    );
                }
            }
            info!(
                target: "neo::remote_ledger",
                endpoint,
                height = advertised_tip,
                "using remote RPC endpoint as ledger source"
            );
            Arc::new(source)
        }
        LedgerMode::Local => Arc::new(LedgerBlockSource::new(
            Arc::clone(&snapshot),
            Arc::clone(&ledger_ctx),
            Arc::clone(&mempool),
        )),
    };
    let daemon_ctx = Arc::new(
        DaemonContext::new(
            Arc::clone(&settings),
            snapshot,
            store_cache,
            state_service.clone(),
            config.state_service.track_during_catchup,
            indexer_service.clone(),
            application_logs_service.clone(),
        )
        .with_native_contract_provider(Arc::clone(&native_contract_provider)),
    );
    let system_ctx = Arc::clone(&daemon_ctx);
    let (mut service, blockchain) = BlockchainService::with_defaults(
        system_ctx,
        Arc::clone(&ledger_ctx),
        Arc::clone(&header_cache),
        Arc::clone(&mempool),
    );
    service.set_stop_at_height(stop_at_height);

    let shutdown = CancellationToken::new();
    let mut handles = Vec::new();
    spawn_daemon_task(
        &mut handles,
        observability.as_ref(),
        &shutdown,
        TaskKind::Essential,
        "blockchain_service",
        service.run(),
    );

    if ledger_mode.uses_local_replay_services() {
        // C# Blockchain.OnInitialize: persist genesis on an empty store.
        blockchain
            .initialize()
            .await
            .map_err(|_| anyhow::anyhow!("blockchain service command loop closed during init"))?;
    } else {
        info!(
            target: "neo::remote_ledger",
            "local ledger initialization disabled; JSON-RPC ledger reads are delegated upstream"
        );
    }

    // ----- dBFT consensus participation -----
    // Build the validator set + this node's role from the protocol settings and
    // the `[consensus]` config. The driver itself is spawned after the network
    // exists (it needs the outbound relay handle); the inbound channel is set up
    // here so the forwarder can feed it decoded dBFT payloads.
    let consensus_enabled = config.consensus.enabled || config.consensus.auto_start;
    let consensus_setup = crate::consensus::build_consensus_setup(
        &settings,
        consensus_enabled,
        config.consensus.private_key_hex.as_deref(),
        config.consensus.hsm.as_ref(),
    )?;
    let consensus_configured = consensus_setup.is_some();
    let consensus_validators = consensus_setup
        .as_ref()
        .map(|s| Arc::new(parking_lot::RwLock::new(s.validators.clone())));
    // Validators + network magic the forwarder uses to decode/authenticate
    // inbound dBFT extensible payloads.
    let consensus_decode = consensus_setup
        .as_ref()
        .zip(consensus_validators.as_ref())
        .map(|(s, validators)| (Arc::clone(validators), s.network));
    let (consensus_inbound_tx, consensus_inbound_rx) = if consensus_configured {
        let (tx, rx) =
            tokio::sync::mpsc::channel::<neo_consensus::messages::ConsensusPayload>(1024);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    // Late-transaction feed (C# `ConsensusService.OnTransaction`): the inventory
    // forwarder pushes the hash of every freshly-accepted mempool transaction
    // here, and the consensus driver feeds it into the state machine so a backup
    // waiting on a proposal transaction can resume the round when it arrives
    // rather than view-changing on `TxNotFound`.
    let (consensus_tx_feed_tx, consensus_tx_feed_rx) = if consensus_configured {
        let (tx, rx) = tokio::sync::mpsc::channel::<neo_primitives::UInt256>(1024);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    // ----- signed StateRoot (StateValidators) participation -----
    // The driver runs whenever the state service is enabled: validators sign +
    // relay votes; observers verify + persist inbound signed roots. The inbound
    // channel is set up here so the forwarder can feed it StateService payloads;
    // the driver task is spawned after the network exists (it needs the relay).
    let state_root_setup = crate::state_root::build_state_root_setup(
        &settings,
        config.state_service.enabled && ledger_mode.uses_local_replay_services(),
        config.state_service.validator_key_hex.as_deref(),
    )?;
    let (state_root_inbound_tx, state_root_inbound_rx) = if state_root_setup.is_some() {
        let (tx, rx) = tokio::sync::mpsc::channel::<neo_payloads::ExtensiblePayload>(1024);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    // ----- inbound inventory relay: peer blocks/transactions -> ledger -----
    // The network layer is decoupled from the blockchain (C# `NeoSystem`
    // mediator), so each per-peer task forwards decoded inventory over this
    // channel; the forwarder hands blocks to the blockchain service, which
    // applies the C# `Blockchain.OnNewBlock` sequencing. The forwarder is
    // spawned *after* the network exists so it can re-announce accepted
    // transactions to peers via `Inv` (C# `LocalNode.RelayDirectly`).
    let (inv_tx, mut inv_rx) =
        tokio::sync::mpsc::channel::<neo_network::InboundInventory>(FAST_SYNC_BURST_CAPACITY);

    // ----- P2P service -----
    let channels_config = config.p2p.channels_config()?;
    let (net_service, network) =
        neo_network::LocalNodeService::with_config(Arc::clone(&settings), channels_config);
    let net_service = if ledger_mode.uses_local_replay_services() {
        net_service.with_inventory_sink(inv_tx)
    } else {
        info!(
            target: "neo::remote_ledger",
            "inbound P2P inventory disabled; peer blocks and transactions will not populate local state"
        );
        net_service
    };
    let net_service = net_service.with_block_source(block_source);
    spawn_daemon_task(
        &mut handles,
        observability.as_ref(),
        &shutdown,
        TaskKind::Essential,
        "p2p_service",
        net_service.run(),
    );

    if ledger_mode.uses_local_replay_services() {
        let blockchain = blockchain.clone();
        let relay = network.clone();
        let consensus_decode = consensus_decode.clone();
        let consensus_inbound_tx = consensus_inbound_tx.clone();
        let consensus_tx_feed_tx = consensus_tx_feed_tx.clone();
        let state_root_inbound_tx = state_root_inbound_tx.clone();
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Essential,
            "inventory_relay",
            async move {
                use tokio::time::{Duration, MissedTickBehavior};

                let mut pending_blocks: Vec<Arc<neo_payloads::Block>> =
                    Vec::with_capacity(FAST_SYNC_BLOCK_BATCH_SIZE);
                let mut flush_timer =
                    tokio::time::interval(Duration::from_millis(FAST_SYNC_BLOCK_BATCH_FLUSH_MS));
                flush_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        maybe_item = inv_rx.recv() => {
                            let Some(item) = maybe_item else {
                                flush_inventory_block_batch(&blockchain, &mut pending_blocks).await;
                                break;
                            };
                            handle_inbound_inventory_item(
                                item,
                                &blockchain,
                                &relay,
                                &consensus_decode,
                                &consensus_inbound_tx,
                                &consensus_tx_feed_tx,
                                &state_root_inbound_tx,
                                &mut pending_blocks,
                            ).await;
                        }
                        _ = flush_timer.tick() => {
                            flush_inventory_block_batch(&blockchain, &mut pending_blocks).await;
                        }
                    }
                }
            },
        );
    } else {
        drop(inv_rx);
    }

    // ----- dBFT consensus driver -----
    // Spawn the round-driving task now that the network relay handle exists.
    // A configured key that is not in the current validator set stays idle but
    // keeps tracking imports so it can participate after a committee change.
    if let (Some(setup), Some(inbound_rx), Some(tx_feed_rx)) = (
        consensus_setup.filter(|_| ledger_mode.uses_local_replay_services()),
        consensus_inbound_rx,
        consensus_tx_feed_rx,
    ) {
        // dBFT recovery-log directory: the persistent data directory (same
        // resolution as the ledger store), or `None` for an in-memory node —
        // which disables persistence (C# `DbftSettings.IgnoreRecoveryLogs`).
        let consensus_data_dir = storage_override
            .map(std::path::Path::to_path_buf)
            .or_else(|| config.storage.data_directory());
        if let Some(task) = crate::consensus::consensus_driver_task(
            setup,
            blockchain.clone(),
            Arc::clone(&mempool),
            network.clone(),
            Arc::clone(&settings),
            consensus_validators.expect("configured consensus has validators"),
            Arc::clone(&store),
            consensus_data_dir.as_deref(),
            inbound_rx,
            tx_feed_rx,
        ) {
            info!(target: "neo", "dBFT consensus driver started (validator node)");
            spawn_daemon_task(
                &mut handles,
                observability.as_ref(),
                &shutdown,
                TaskKind::Essential,
                "consensus_driver",
                task,
            );
        }
    }

    // ----- signed StateRoot driver -----
    // Spawn the StateValidators vote/aggregate/verify driver now that the
    // network relay handle exists. Requires the local state store (roots to
    // attest and a home for finalized signed roots).
    if let (Some(setup), Some(inbound_rx), Some(state_store)) =
        (state_root_setup, state_root_inbound_rx, state_store.clone())
    {
        let has_key = setup.keypair.is_some();
        let task = crate::state_root::state_root_driver_task(
            setup,
            blockchain.clone(),
            network.clone(),
            Arc::clone(&settings),
            Arc::clone(&store),
            Arc::clone(&native_contract_provider),
            state_store,
            inbound_rx,
        );
        info!(
            target: "neo",
            validator = has_key,
            "signed StateRoot driver started",
        );
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Essential,
            "state_root_driver",
            task,
        );
    }

    // ----- ledger height -> network advertisement -----
    // Seed the advertised height from the DURABLE tip before P2P sync starts,
    // so a node restarted on a populated store advertises its real height and
    // the block-sync cursor (`local_height + 1`) resumes from the persisted tip
    // instead of re-requesting the entire chain from block 1.
    let _ = network.set_block_height(advertised_tip).await;
    info!(target: "neo", height = advertised_tip, "advertised ledger tip to peers");

    // As the ledger persists blocks, advertise the new height to peers
    // (version + ping) so block-sync requests advance their cursor and
    // peers learn our progress (C# `LocalNode` reads `Ledger.CurrentIndex`).
    {
        let mut events = blockchain.subscribe();
        let network = network.clone();
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Normal,
            "network_height_advertiser",
            async move {
                use neo_blockchain::RuntimeEvent;
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    match events.recv().await {
                        Ok(RuntimeEvent::Imported { height, .. }) => {
                            let _ = network.set_block_height(height).await;
                        }
                        Ok(_) => {}
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            },
        );
    }

    let service_registry = neo_system::ServiceRegistry::new();
    if let LedgerMode::RemoteRpc { endpoint } = ledger_mode {
        let status = match remote_tip_error {
            Some(error) => RemoteLedgerStatus::unavailable(endpoint.to_string(), error),
            None => RemoteLedgerStatus::new(endpoint.to_string(), remote_advertised_tip),
        };
        service_registry.register(Arc::new(status));
    }
    if let Some(state_store) = &state_store {
        service_registry.register(Arc::clone(state_store));
    }
    if let Some(state_service) = &state_service {
        service_registry.register(Arc::clone(state_service));
    }
    if let Some(indexer) = &indexer_service {
        service_registry.register(Arc::clone(indexer));
    }
    if let Some(application_logs) = &application_logs_service {
        service_registry.register(Arc::clone(application_logs));
    }
    if let Some(tokens_tracker) = &tokens_tracker_service {
        service_registry.register(Arc::clone(tokens_tracker));
    }

    let node = Arc::new(
        neo_system::Node::builder()
            .with_settings(settings)
            .with_storage(store)
            .with_blockchain(blockchain.clone())
            .with_network(network.clone())
            .with_mempool(mempool)
            .with_header_cache(header_cache)
            .with_services(service_registry)
            .with_native_contract_provider(native_contract_provider)
            .build()
            .map_err(|e| anyhow::anyhow!("node build failed: {e}"))?,
    );
    daemon_ctx.set_node(Arc::clone(&node));
    if let Some((tracker_settings, tracker_store)) = tokens_tracker_runtime {
        daemon_ctx.set_tokens_tracker(Some(Arc::new(
            neo_rpc::plugins::tokens_tracker::TokensTracker::new(
                tracker_settings,
                tracker_store,
                node.settings(),
            ),
        )));
    }

    if let Some(indexer) = indexer_service {
        // The indexer runtime is expensive during catch-up, but service
        // provider profiles must start indexing automatically once sync is
        // close enough to the peer tip. Keep one supervisor alive so cold-start
        // service nodes do not require a manual restart after initial sync.
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            &shutdown,
            TaskKind::Normal,
            "indexer_runtime",
            indexer_runtime::run_live_indexer_when_ready(
                blockchain.clone(),
                indexer,
                application_logs_service,
                config.indexer.backfill_on_startup,
                u64::from(durable_tip_height),
            ),
        );
    }

    Ok(RunningNode {
        node,
        network,
        handles,
        shutdown,
        durable_service_stores: durable_stores,
    })
}

#[cfg(test)]
#[path = "../tests/node/mod.rs"]
mod tests;
