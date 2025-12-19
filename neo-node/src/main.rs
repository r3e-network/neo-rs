//! Neo Node - Neo N3 node daemon (server)
//!
//! `neo-node` is a long-running daemon: it runs the Neo N3 protocol, syncs the chain over P2P,
//! and (optionally) exposes a JSON-RPC server for external clients.

mod cli;
mod config;
mod health;
#[cfg(feature = "hsm")]
mod hsm_integration;
#[cfg(feature = "hsm")]
mod hsm_wallet;
mod logging;
mod metrics;
mod startup;
#[cfg(feature = "tee")]
mod tee_integration;

use anyhow::{bail, Context, Result};
use clap::Parser;
use cli::NodeCli;
use config::NodeConfig;
use neo_core::{
    neo_system::NeoSystem,
    network::p2p::channels_config::ChannelsConfig,
    protocol_settings::ProtocolSettings,
    state_service::{metrics::state_root_ingest_stats, state_store::StateServiceSettings},
    wallets::{Nep6Wallet, Wallet as CoreWallet},
};
use neo_rpc::server::{
    RpcServer, RpcServerBlockchain, RpcServerNode, RpcServerSettings, RpcServerSmartContract,
    RpcServerState, RpcServerUtilities, RpcServerWallet,
};
use parking_lot::RwLock as ParkingRwLock;
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc};
use tokio::signal;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = NodeCli::parse();
    let mut node_config = NodeConfig::load(&cli.config)?;

    let logging_handles = logging::init_tracing(&node_config.logging, cli.daemon)?;
    let _log_guard = logging_handles.guard;

    apply_cli_overrides(&cli, &mut node_config);

    let storage_config = node_config.storage_config();
    let storage_path = cli
        .storage
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| node_config.storage_path());
    let backend_name = node_config.storage_backend().map(|name| name.to_string());

    let protocol_settings: ProtocolSettings = node_config.protocol_settings();
    let read_only_storage = node_config.storage.read_only.unwrap_or(false);

    startup::validate_node_config(
        &node_config,
        storage_path.as_deref(),
        backend_name.as_deref(),
        &protocol_settings,
        cli.rpc_hardened,
    )?;

    if cli.check_all {
        startup::check_storage_access(
            backend_name.as_deref(),
            storage_path.as_deref(),
            storage_config.clone(),
        )?;
        info!(target: "neo", "configuration validated; exiting due to --check-all");
        return Ok(());
    }

    if cli.check_storage {
        startup::check_storage_access(
            backend_name.as_deref(),
            storage_path.as_deref(),
            storage_config.clone(),
        )?;
        info!(target: "neo", "storage backend validated; exiting due to --check-storage");
        return Ok(());
    }

    if cli.check_config {
        info!(target: "neo", "configuration validated; exiting due to --check-config");
        return Ok(());
    }

    info!(
        target: "neo",
        seeds = ?protocol_settings.seed_list,
        network_magic = format_args!("0x{:08x}", protocol_settings.network),
        listen_port = node_config.p2p.listen_port,
        storage = storage_path.as_deref().unwrap_or("<none>"),
        backend = backend_name.as_deref().unwrap_or("memory"),
        "using protocol settings"
    );
    info!(
        target: "neo",
        build = %startup::build_feature_summary(),
        "build profile"
    );

    if read_only_storage {
        bail!("read-only storage mode is only supported with --check-* flags; cannot start the node in read-only mode");
    }

    let store_provider = startup::select_store_provider(backend_name.as_deref(), storage_config)?;
    if let (Some(_provider), Some(path)) = (&store_provider, &storage_path) {
        startup::check_storage_network(path, protocol_settings.network, read_only_storage)?;
    }
    if store_provider.is_some() && storage_path.is_none() {
        let backend = backend_name.unwrap_or_else(|| "unknown".to_string());
        bail!(
            "storage backend '{}' requires a data path (--storage or [storage.path])",
            backend
        );
    }

    let state_service_settings = build_state_service_settings(&cli, storage_path.as_deref());

    // Generate the RpcServer.json consumed by the neo-rpc server settings loader.
    let rpc_plugin_config_path = node_config
        .write_rpc_server_plugin_config(&protocol_settings)?
        .map(|path| path.to_string_lossy().to_string());
    if let Some(path) = rpc_plugin_config_path.as_deref() {
        info!(target: "neo", path, "rpc server configuration emitted");
    }

    let system = NeoSystem::new_with_state_service(
        protocol_settings.clone(),
        store_provider.clone(),
        storage_path.clone(),
        state_service_settings.clone(),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
    .context("failed to initialise NeoSystem")?;

    let channels_config = build_channels_config(&node_config);
    system
        .start_node(channels_config)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to start P2P subsystem")?;

    let rpc_server = start_rpc_server_if_enabled(
        &node_config,
        system.clone(),
        protocol_settings.network,
        rpc_plugin_config_path.as_deref(),
    )
    .context("failed to start RPC server")?;

    let hsm_wallet_enabled =
        maybe_enable_hsm_wallet(&cli, &node_config, &rpc_server, &system).await?;
    if !hsm_wallet_enabled {
        maybe_open_wallet(&cli, &node_config, &rpc_server, &system)?;
    }

    let health_state = Arc::new(RwLock::new(health::HealthState::default()));
    start_health_endpoint_if_enabled(&cli, &node_config, health_state.clone()).await;
    let (pump_shutdown_tx, mut pump_shutdown_rx) = tokio::sync::watch::channel(false);
    let metrics_storage_path = storage_path.clone();
    let metrics_system = system.clone();
    let pump_health_state = health_state.clone();
    let metrics_handle = tokio::spawn(async move {
        let tick = std::time::Duration::from_secs(1);
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tick) => {}
                _ = pump_shutdown_rx.changed() => {
                    if *pump_shutdown_rx.borrow() {
                        break;
                    }
                    continue;
                }
            }

            let block_height = metrics_system.current_block_index();
            let header_height = metrics_system.ledger_context().highest_header_index();
            let header_lag = header_height.saturating_sub(block_height);

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

            metrics::update_metrics(
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

    info!(target: "neo", "node running, press Ctrl+C to stop");
    if let Err(err) = signal::ctrl_c().await {
        error!(target: "neo", error = %err, "failed to wait for shutdown signal");
    } else {
        info!(target: "neo", "shutdown signal received (Ctrl+C)");
    }

    let _ = pump_shutdown_tx.send(true);
    let _ = metrics_handle.await;

    if let Some(server) = rpc_server {
        if let Some(mut guard) = server.try_write() {
            guard.stop_rpc_server();
        }
    }

    system
        .shutdown()
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to shut down NeoSystem")?;

    info!(target: "neo", "shutdown complete");
    Ok(())
}

fn build_channels_config(node_config: &NodeConfig) -> ChannelsConfig {
    node_config.channels_config()
}

fn start_rpc_server_if_enabled(
    node_config: &NodeConfig,
    system: Arc<NeoSystem>,
    network: u32,
    rpc_config_path: Option<&str>,
) -> Result<Option<Arc<ParkingRwLock<RpcServer>>>> {
    if !node_config.rpc.enabled {
        return Ok(None);
    }

    let mut settings_json: Option<Value> = None;
    if let Some(path) = rpc_config_path {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read rpc config at {}", path))?;
        settings_json = Some(serde_json::from_str(&raw).context("invalid rpc config json")?);
    }
    RpcServerSettings::load(settings_json.as_ref())
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to load rpc server settings")?;

    let settings = RpcServerSettings::current()
        .server_for_network(network)
        .unwrap_or_default();

    let mut server = RpcServer::new(system, settings);
    server.register_handlers(RpcServerNode::register_handlers());
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerState::register_handlers());
    server.register_handlers(RpcServerUtilities::register_handlers());
    server.register_handlers(RpcServerSmartContract::register_handlers());
    server.register_handlers(RpcServerWallet::register_handlers());

    let handle = Arc::new(ParkingRwLock::new(server));
    neo_rpc::server::register_server(network, Arc::clone(&handle));
    handle.write().start_rpc_server(Arc::downgrade(&handle));

    Ok(Some(handle))
}

async fn maybe_enable_hsm_wallet(
    cli: &NodeCli,
    node_config: &NodeConfig,
    rpc_server: &Option<Arc<ParkingRwLock<RpcServer>>>,
    system: &Arc<NeoSystem>,
) -> Result<bool> {
    #[cfg(feature = "hsm")]
    {
        if !cli.hsm {
            return Ok(false);
        }

        let Some(server) = rpc_server else {
            warn!(target: "neo", "HSM requested but RPC is disabled; skipping HSM wallet");
            return Ok(false);
        };

        if resolve_wallet_config(cli, node_config)?.is_some() {
            warn!(
                target: "neo",
                "HSM requested; ignoring NEP-6 wallet configuration to avoid conflicting signers"
            );
        }

        let runtime = hsm_integration::initialize_hsm(cli, system.settings().address_version)
            .await
            .context("failed to initialize HSM")?;
        hsm_integration::print_hsm_status(&runtime);
        let wallet =
            hsm_wallet::HsmWallet::from_runtime(runtime, Arc::new(system.settings().clone()))
                .await
                .context("failed to build HSM wallet")?;
        server.write().set_wallet(Some(Arc::new(wallet)));
        return Ok(true);
    }

    #[cfg(not(feature = "hsm"))]
    {
        let _ = (cli, node_config, rpc_server, system);
        Ok(false)
    }
}

fn resolve_wallet_config(
    cli: &NodeCli,
    node_config: &NodeConfig,
) -> Result<Option<(PathBuf, String)>> {
    if let Some(path) = cli.wallet.clone() {
        let password = cli
            .wallet_password
            .clone()
            .or_else(|| node_config.unlock_wallet.password.clone())
            .ok_or_else(|| anyhow::anyhow!("wallet password required (--wallet-password)"))?;
        return Ok(Some((path, password)));
    }

    if node_config.unlock_wallet.is_active {
        let path = node_config
            .unlock_wallet
            .path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("unlock_wallet.path must be set when enabled"))?;
        let password =
            node_config.unlock_wallet.password.clone().ok_or_else(|| {
                anyhow::anyhow!("unlock_wallet.password must be set when enabled")
            })?;
        return Ok(Some((PathBuf::from(path), password)));
    }

    Ok(None)
}

fn maybe_open_wallet(
    cli: &NodeCli,
    node_config: &NodeConfig,
    rpc_server: &Option<Arc<ParkingRwLock<RpcServer>>>,
    system: &Arc<NeoSystem>,
) -> Result<()> {
    let Some((wallet_path, password)) = resolve_wallet_config(cli, node_config)? else {
        return Ok(());
    };

    let Some(server) = rpc_server else {
        warn!(
            target: "neo",
            path = %wallet_path.display(),
            "wallet configured but RPC is disabled; skipping wallet load"
        );
        return Ok(());
    };

    if !wallet_path.exists() {
        bail!("wallet file not found: {}", wallet_path.display());
    }

    let settings = Arc::new(system.settings().clone());
    let wallet = Nep6Wallet::from_file(&wallet_path, &password, settings)
        .map_err(|err| anyhow::anyhow!(err.to_string()))
        .context("failed to open wallet")?;
    let wallet_arc: Arc<dyn CoreWallet> = Arc::new(wallet);
    server.write().set_wallet(Some(wallet_arc));

    info!(
        target: "neo",
        path = %wallet_path.display(),
        "wallet opened for RPC signing"
    );
    Ok(())
}

async fn start_health_endpoint_if_enabled(
    cli: &NodeCli,
    node_config: &NodeConfig,
    health_state: Arc<RwLock<health::HealthState>>,
) {
    let Some(health_port) = cli.health_port else {
        return;
    };

    let max_lag = cli
        .health_max_header_lag
        .unwrap_or(health::DEFAULT_MAX_HEADER_LAG);
    let storage_for_health = node_config.storage_path();
    let rpc_enabled_for_health = node_config.rpc.enabled;

    tokio::spawn(async move {
        if let Err(e) = health::serve_health_with_state(
            health_port,
            max_lag,
            storage_for_health,
            rpc_enabled_for_health,
            health_state,
        )
        .await
        {
            error!(target: "neo", error = %e, "health endpoint failed");
        }
    });
    info!(target: "neo", port = health_port, "health endpoint started");
}

fn build_state_service_settings(
    cli: &NodeCli,
    storage_path: Option<&str>,
) -> Option<StateServiceSettings> {
    if !cli.state_root {
        return None;
    }

    let default_state_dir = PathBuf::from("StateRoot");
    let requested_state_dir = cli
        .state_root_path
        .clone()
        .unwrap_or_else(|| default_state_dir.clone());
    let resolved_state_dir = if requested_state_dir.is_absolute() {
        requested_state_dir
    } else if let Some(storage_root) = storage_path {
        PathBuf::from(storage_root).join(requested_state_dir)
    } else {
        requested_state_dir
    };
    let state_path = resolved_state_dir.to_string_lossy().to_string();
    info!(
        target: "neo",
        path = %state_path,
        full_state = cli.state_root_full_state,
        "state root calculation enabled"
    );
    Some(StateServiceSettings {
        full_state: cli.state_root_full_state,
        path: state_path,
    })
}

/// Applies CLI argument overrides to the node configuration.
fn apply_cli_overrides(cli: &NodeCli, node_config: &mut NodeConfig) {
    if let Some(magic) = cli.network_magic {
        node_config.network.network_magic = Some(magic);
    }
    if let Some(port) = cli.listen_port {
        node_config.p2p.listen_port = Some(port);
    }
    if !cli.seed_nodes.is_empty() {
        node_config.p2p.seed_nodes = cli.seed_nodes.clone();
    }
    if let Some(max_conn) = cli.max_connections {
        node_config.p2p.max_connections = Some(max_conn);
    }
    if let Some(min_conn) = cli.min_connections {
        node_config.p2p.min_desired_connections = Some(min_conn);
    }
    if let Some(max_per_address) = cli.max_connections_per_address {
        node_config.p2p.max_connections_per_address = Some(max_per_address);
    }
    if let Some(limit) = cli.broadcast_history_limit {
        node_config.p2p.broadcast_history_limit = Some(limit);
    }
    if cli.disable_compression {
        node_config.p2p.enable_compression = Some(false);
    }
    if let Some(seconds) = cli.block_time {
        node_config.blockchain.block_time = Some(seconds);
    }
    if let Some(backend) = &cli.backend {
        node_config.storage.backend = Some(backend.clone());
    }
    if let Some(bind) = &cli.rpc_bind {
        node_config.rpc.bind_address = Some(bind.clone());
    }
    if let Some(port) = cli.rpc_port {
        node_config.rpc.port = Some(port);
    }
    if cli.rpc_disable_cors {
        node_config.rpc.cors_enabled = Some(false);
    }
    if let Some(user) = &cli.rpc_user {
        node_config.rpc.rpc_user = Some(user.clone());
    }
    if let Some(pass) = &cli.rpc_pass {
        node_config.rpc.rpc_pass = Some(pass.clone());
    }
    if let Some(cert) = &cli.rpc_tls_cert {
        node_config.rpc.tls_cert_file = Some(cert.clone());
    }
    if let Some(cert_pass) = &cli.rpc_tls_cert_password {
        node_config.rpc.tls_cert_password = Some(cert_pass.clone());
    }
    if let Some(path) = &cli.logging_path {
        node_config.logging.file_path = Some(path.clone());
    }
    if let Some(level) = &cli.logging_level {
        node_config.logging.level = Some(level.clone());
    }
    if let Some(format) = &cli.logging_format {
        node_config.logging.format = Some(format.clone());
    }
    if cli.storage_read_only {
        node_config.storage.read_only = Some(true);
    }
    if !cli.rpc_allow_origins.is_empty() {
        node_config.rpc.allow_origins = cli.rpc_allow_origins.clone();
    }
    if !cli.rpc_disabled_methods.is_empty() {
        node_config.rpc.disabled_methods = cli.rpc_disabled_methods.clone();
    }
    if cli.rpc_hardened {
        node_config.rpc.cors_enabled = Some(false);
        node_config.rpc.auth_enabled = true;
        node_config.rpc.allow_origins.clear();
        let mut disabled = node_config.rpc.disabled_methods.clone();
        if !disabled
            .iter()
            .any(|m| m.eq_ignore_ascii_case("openwallet"))
        {
            disabled.push("openwallet".to_string());
        }
        if !disabled
            .iter()
            .any(|m| m.eq_ignore_ascii_case("listplugins"))
        {
            disabled.push("listplugins".to_string());
        }
        node_config.rpc.disabled_methods = disabled;
    }
}
