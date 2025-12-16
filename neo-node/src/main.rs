//! Neo Node - Neo N3 node daemon (server)
//!
//! `neo-node` is a long-running daemon: it runs the Neo N3 protocol, syncs the chain over P2P,
//! and (optionally) exposes a JSON-RPC server for external clients.

mod cli;
mod config;
mod executor;
mod genesis;
mod health;
mod logging;
mod metrics;
mod p2p_service;
mod rpc_service;
mod runtime;
mod startup;
mod state_validator;
mod sync_service;
#[cfg(feature = "tee")]
mod tee_integration;
mod validator_service;

pub use p2p_service::{P2PService, P2PServiceState};
pub use rpc_service::{RpcService, RpcServiceConfig, RpcServiceState};
pub use runtime::{NodeRuntime, RuntimeConfig, RuntimeEvent};
pub use sync_service::{SyncService, SyncState, SyncStats};
pub use validator_service::{ValidatorConfig, ValidatorService, ValidatorState};

use anyhow::{bail, Context, Result};
use clap::Parser;
use cli::NodeCli;
use config::NodeConfig;
use neo_core::{
    protocol_settings::ProtocolSettings,
    state_service::state_store::StateServiceSettings,
};
use std::{path::PathBuf, sync::Arc};
use tokio::signal;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = NodeCli::parse();
    let mut node_config = NodeConfig::load(&cli.config)?;

    // Initialize logging
    let logging_handles = logging::init_tracing(&node_config.logging, cli.daemon)?;
    let _log_guard = logging_handles.guard;

    // Apply CLI overrides
    apply_cli_overrides(&cli, &mut node_config);

    // Build protocol settings
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

    // Handle check modes
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

    // Write RPC server plugin configuration
    if let Some(path) = node_config.write_rpc_server_plugin_config(&protocol_settings)? {
        info!(
            target: "neo",
            path = %path.display(),
            "rpc server configuration emitted"
        );
    }

    if read_only_storage && !(cli.check_all || cli.check_config || cli.check_storage) {
        bail!("read-only storage mode is only supported with --check-* flags; cannot start the node in read-only mode");
    }

    // Initialize storage provider
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

    // Build state service settings if enabled
    let state_service_settings = build_state_service_settings(&cli, storage_path.as_deref());

    // Build validators list from protocol settings
    let validators = build_validators_list(&protocol_settings);

    info!(
        target: "neo",
        validator_count = validators.len(),
        "loaded validators from protocol settings"
    );

    // Load validator configuration from wallet if provided
    let (validator_index, private_key) = load_validator_config(&cli, &protocol_settings)?;

    // Build RuntimeConfig
    let runtime_config = RuntimeConfig {
        network_magic: protocol_settings.network,
        protocol_version: 0,
        validator_index,
        validators,
        private_key,
        p2p: neo_p2p::P2PConfig {
            listen_address: format!("0.0.0.0:{}", node_config.p2p.listen_port.unwrap_or(10333))
                .parse()
                .unwrap_or_else(|_| "0.0.0.0:10333".parse().unwrap()),
            max_inbound: node_config.p2p.max_connections.unwrap_or(10),
            max_outbound: node_config.p2p.min_desired_connections.unwrap_or(10),
            seed_nodes: startup::resolve_seed_nodes(&node_config.p2p.seed_nodes).await,
            network_magic: protocol_settings.network,
            ..Default::default()
        },
        mempool: neo_mempool::MempoolConfig::default(),
        state_service: state_service_settings,
        protocol_settings: protocol_settings.clone(),
    };

    info!(
        target: "neo",
        network = format!("{:#X}", protocol_settings.network),
        backend = backend_name.as_deref().unwrap_or("memory"),
        storage = storage_path.as_deref().unwrap_or("<in-memory>"),
        rpc_enabled = node_config.rpc.enabled,
        "configuration validated successfully"
    );

    // Start services and run main loop
    run_node(runtime_config, &node_config, &cli).await
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

/// Builds state service settings if state root is enabled.
fn build_state_service_settings(cli: &NodeCli, storage_path: Option<&str>) -> Option<StateServiceSettings> {
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

/// Builds the validators list from protocol settings.
fn build_validators_list(protocol_settings: &ProtocolSettings) -> Vec<neo_consensus::ValidatorInfo> {
    protocol_settings
        .standby_committee
        .iter()
        .take(protocol_settings.validators_count as usize)
        .enumerate()
        .map(|(index, public_key)| {
            let script_hash = neo_core::smart_contract::Contract::create_signature_contract(
                public_key.clone(),
            )
            .script_hash();
            neo_consensus::ValidatorInfo {
                index: index as u8,
                public_key: public_key.clone(),
                script_hash,
            }
        })
        .collect()
}

/// Loads validator configuration from wallet if provided.
fn load_validator_config(
    cli: &NodeCli,
    protocol_settings: &ProtocolSettings,
) -> Result<(Option<u8>, Vec<u8>)> {
    if let (Some(wallet_path), Some(password)) = (&cli.wallet, &cli.wallet_password) {
        info!(
            target: "neo",
            wallet = %wallet_path.display(),
            "loading validator wallet"
        );

        match validator_service::load_validator_from_wallet(
            wallet_path.to_str().unwrap_or(""),
            password,
            Arc::new(protocol_settings.clone()),
        ) {
            Ok(Some(config)) => {
                info!(
                    target: "neo",
                    validator_index = config.validator_index,
                    script_hash = %config.script_hash,
                    "validator mode enabled"
                );
                Ok((Some(config.validator_index), config.private_key))
            }
            Ok(None) => {
                warn!(
                    target: "neo",
                    "wallet account is not in standby committee - running in non-validator mode"
                );
                Ok((None, Vec::new()))
            }
            Err(e) => {
                warn!(
                    target: "neo",
                    error = %e,
                    "failed to load validator wallet - running in non-validator mode"
                );
                Ok((None, Vec::new()))
            }
        }
    } else if cli.wallet.is_some() {
        bail!("--wallet-password is required when --wallet is specified");
    } else {
        Ok((None, Vec::new()))
    }
}

/// Runs the node with the given configuration.
async fn run_node(
    runtime_config: RuntimeConfig,
    node_config: &NodeConfig,
    cli: &NodeCli,
) -> Result<()> {
    // Create and start the node runtime
    let mut node_runtime = NodeRuntime::new(runtime_config.clone());

    info!(target: "neo", "starting neo-node runtime...");

    node_runtime
        .start()
        .await
        .context("failed to start node runtime")?;

    info!(
        target: "neo",
        height = node_runtime.height().await,
        mempool_size = node_runtime.mempool_size().await,
        "neo-node runtime started"
    );

    // Start P2P service
    let p2p_service = P2PService::new(runtime_config.p2p.clone(), node_runtime.p2p_event_sender());
    if let Err(e) = p2p_service.start().await {
        error!(target: "neo", error = %e, "failed to start P2P service");
    } else {
        info!(
            target: "neo",
            listen = %runtime_config.p2p.listen_address,
            seeds = runtime_config.p2p.seed_nodes.len(),
            "P2P service started"
        );
    }

    // Optionally start RPC service
    let rpc_service = start_rpc_service_if_enabled(node_config, &runtime_config).await;

    // Optionally start health endpoint
    start_health_endpoint_if_enabled(cli, node_config).await;

    // Wait for shutdown signal
    info!(target: "neo", "node running, press Ctrl+C to stop");

    if let Err(err) = signal::ctrl_c().await {
        error!(target: "neo", error = %err, "failed to wait for shutdown signal");
    } else {
        info!(target: "neo", "shutdown signal received (Ctrl+C)");
    }

    // Graceful shutdown
    if let Err(e) = p2p_service.stop().await {
        error!(target: "neo", error = %e, "failed to stop P2P service");
    }

    if let Some(rpc) = rpc_service {
        if let Err(e) = rpc.stop().await {
            error!(target: "neo", error = %e, "failed to stop RPC service");
        }
    }

    node_runtime
        .stop()
        .await
        .context("failed to stop node runtime")?;

    info!(target: "neo", "shutdown complete");
    Ok(())
}

/// Starts the RPC service if enabled in configuration.
async fn start_rpc_service_if_enabled(
    node_config: &NodeConfig,
    runtime_config: &RuntimeConfig,
) -> Option<RpcService> {
    if !node_config.rpc.enabled {
        return None;
    }

    let rpc_bind = node_config
        .rpc
        .bind_address
        .as_deref()
        .unwrap_or("127.0.0.1");
    let rpc_port = node_config.rpc.port.unwrap_or(10332);
    let rpc_addr = format!("{}:{}", rpc_bind, rpc_port)
        .parse()
        .unwrap_or_else(|_| "127.0.0.1:10332".parse().unwrap());

    let rpc_config = RpcServiceConfig {
        bind_address: rpc_addr,
        cors_enabled: node_config.rpc.cors_enabled.unwrap_or(true),
        allowed_origins: node_config.rpc.allow_origins.clone(),
    };

    let service = RpcService::new(rpc_config);
    service.set_network_magic(runtime_config.network_magic).await;

    if let Err(e) = service.start().await {
        error!(target: "neo", error = %e, "failed to start RPC service");
    } else {
        info!(target: "neo", address = %rpc_addr, "RPC service started");
    }

    Some(service)
}

/// Starts the health endpoint if enabled via CLI.
async fn start_health_endpoint_if_enabled(cli: &NodeCli, node_config: &NodeConfig) {
    let Some(health_port) = cli.health_port else {
        return;
    };

    let max_lag = cli
        .health_max_header_lag
        .unwrap_or(health::DEFAULT_MAX_HEADER_LAG);
    let storage_for_health = node_config.storage_path();
    let rpc_enabled_for_health = node_config.rpc.enabled;

    tokio::spawn(async move {
        if let Err(e) = health::serve_health(
            health_port,
            max_lag,
            storage_for_health,
            rpc_enabled_for_health,
        )
        .await
        {
            error!(target: "neo", error = %e, "health endpoint failed");
        }
    });
    info!(target: "neo", port = health_port, "health endpoint started");
}
