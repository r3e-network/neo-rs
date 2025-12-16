//! Neo Node - Neo N3 node daemon (server)
//!
//! `neo-node` is a long-running daemon: it runs the Neo N3 protocol, syncs the chain over P2P,
//! and (optionally) exposes a JSON-RPC server for external clients.
//!
//! It is intentionally *daemon-only* and does not provide an interactive "query the node" UI.
//! For RPC queries and user-facing commands, use `neo-cli`, which connects to a running node
//! via JSON-RPC.
//!
//! ## Usage
//! `neo-node --config neo_mainnet_node.toml`
//!
//! The node will:
//! - Start the P2P network and sync with peers
//! - Start the RPC server (if enabled in config)
//! - Optionally run with TEE protection (`--tee`, feature-gated)
//! - Run until Ctrl+C is received

mod config;
mod executor;
mod genesis;
mod health;
mod metrics;
mod p2p_service;
mod rpc_service;
mod runtime;
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
use chrono::Local;
use clap::Parser;
use config::{infer_magic_from_type, NodeConfig};
// NOTE: NeoSystem moved out of neo-core during Phase 2 refactoring
// The node runtime will be reimplemented using the new architecture:
// - neo_state::WorldState for state management
// - neo_p2p::P2PService traits for networking
// - neo_consensus::ConsensusService for dBFT
// Validates configuration and initializes the node runtime.
use neo_core::{
    persistence::{providers::RocksDBStoreProvider, storage::StorageConfig, IStoreProvider},
    protocol_settings::ProtocolSettings,
    state_service::state_store::StateServiceSettings,
};
// NOTE: neo-plugins temporarily disabled during refactoring
// #[allow(unused_imports)]
// use neo_plugins as _;
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::signal;
use tracing::{error, info, warn};
use tracing_appender::{non_blocking, non_blocking::WorkerGuard};
use tracing_subscriber::{fmt, EnvFilter};

pub(crate) const STORAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(name = "neo-node", about = "Neo N3 blockchain node daemon", version)]
struct NodeCli {
    /// Path to the TOML configuration file.
    #[arg(
        long,
        short = 'c',
        default_value = "neo_mainnet_node.toml",
        env = "NEO_CONFIG",
        value_name = "PATH"
    )]
    config: PathBuf,

    /// Overrides the configured storage path.
    #[arg(long, value_name = "PATH", env = "NEO_STORAGE")]
    storage: Option<PathBuf>,

    /// Overrides the storage backend (memory, rocksdb).
    #[arg(long, value_name = "BACKEND", env = "NEO_BACKEND")]
    backend: Option<String>,

    /// Open storage read-only (offline checks only).
    #[arg(long, env = "NEO_STORAGE_READONLY")]
    storage_read_only: bool,

    /// Overrides the network magic used during the P2P handshake.
    #[arg(long, value_name = "MAGIC", env = "NEO_NETWORK_MAGIC")]
    network_magic: Option<u32>,

    /// Overrides the P2P listening port.
    #[arg(long, value_name = "PORT", env = "NEO_LISTEN_PORT")]
    listen_port: Option<u16>,

    /// Replaces the configured seed nodes (comma separated).
    #[arg(
        long = "seed",
        value_delimiter = ',',
        value_name = "HOST:PORT",
        env = "NEO_SEED_NODES"
    )]
    seed_nodes: Vec<String>,

    /// Overrides the maximum number of concurrent connections.
    #[arg(long, value_name = "N", env = "NEO_MAX_CONNECTIONS")]
    max_connections: Option<usize>,

    /// Overrides the minimum desired number of peers.
    #[arg(long, value_name = "N", env = "NEO_MIN_CONNECTIONS")]
    min_connections: Option<usize>,

    /// Overrides the per-address connection cap.
    #[arg(long, value_name = "N", env = "NEO_MAX_CONNECTIONS_PER_ADDRESS")]
    max_connections_per_address: Option<usize>,

    /// Maximum broadcast history entries to retain in memory.
    #[arg(long, value_name = "N", env = "NEO_BROADCAST_HISTORY_LIMIT")]
    broadcast_history_limit: Option<usize>,

    /// Disables compression for outbound connections.
    #[arg(long, env = "NEO_DISABLE_COMPRESSION")]
    disable_compression: bool,

    /// Overrides the block time in seconds.
    #[arg(long, value_name = "SECONDS", env = "NEO_BLOCK_TIME")]
    block_time: Option<u64>,

    /// Run in daemon mode (no console output except errors).
    #[arg(long, short = 'd', env = "NEO_DAEMON")]
    daemon: bool,

    /// Override RPC bind address.
    #[arg(long, value_name = "ADDR", env = "NEO_RPC_BIND")]
    rpc_bind: Option<String>,

    /// Override RPC port.
    #[arg(long, value_name = "PORT", env = "NEO_RPC_PORT")]
    rpc_port: Option<u16>,

    /// Disable RPC CORS.
    #[arg(long, env = "NEO_RPC_DISABLE_CORS")]
    rpc_disable_cors: bool,

    /// Override RPC basic auth username.
    #[arg(long, value_name = "USER", env = "NEO_RPC_USER")]
    rpc_user: Option<String>,

    /// Override RPC basic auth password.
    #[arg(long, value_name = "PASS", env = "NEO_RPC_PASS")]
    rpc_pass: Option<String>,

    /// Override RPC TLS certificate path.
    #[arg(long, value_name = "PATH", env = "NEO_RPC_TLS_CERT")]
    rpc_tls_cert: Option<String>,

    /// Override RPC TLS certificate password.
    #[arg(long, value_name = "PASS", env = "NEO_RPC_TLS_PASS")]
    rpc_tls_cert_password: Option<String>,

    /// Override logging path.
    #[arg(long, value_name = "PATH", env = "NEO_LOG_PATH")]
    logging_path: Option<String>,

    /// Override logging level.
    #[arg(long, value_name = "LEVEL", env = "NEO_LOG_LEVEL")]
    logging_level: Option<String>,

    /// Override logging format.
    #[arg(long, value_name = "FORMAT", env = "NEO_LOG_FORMAT")]
    logging_format: Option<String>,

    /// Override RPC allowed CORS origins (comma-separated).
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "ORIGIN",
        env = "NEO_RPC_ALLOW_ORIGINS"
    )]
    rpc_allow_origins: Vec<String>,

    /// Override RPC disabled methods (comma-separated).
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "METHOD",
        env = "NEO_RPC_DISABLED_METHODS"
    )]
    rpc_disabled_methods: Vec<String>,

    /// Apply hardened RPC defaults (auth required, CORS disabled, common risky methods disabled).
    #[arg(long)]
    rpc_hardened: bool,

    /// Enable TEE (Trusted Execution Environment) mode for wallet and mempool protection.
    #[cfg(feature = "tee")]
    #[arg(long)]
    tee: bool,

    /// Path to store TEE sealed data.
    #[cfg(feature = "tee")]
    #[arg(long, value_name = "PATH", default_value = "./tee_data")]
    tee_data_path: PathBuf,

    /// TEE fair ordering policy (fcfs, batched, commit-reveal).
    #[cfg(feature = "tee")]
    #[arg(long, value_name = "POLICY", default_value = "batched")]
    tee_ordering_policy: String,

    /// Validate configuration and exit without starting the node.
    #[arg(long)]
    check_config: bool,

    /// Validate storage backend connectivity and exit without starting the node.
    #[arg(long)]
    check_storage: bool,

    /// Run both config and storage checks, then exit.
    #[arg(long)]
    check_all: bool,

    /// Enable a lightweight health check server (HTTP on localhost) reporting readiness.
    #[arg(long, value_name = "PORT", env = "NEO_HEALTH_PORT")]
    health_port: Option<u16>,

    /// Fail healthz when header lag exceeds this value (blocks). Defaults to 20 when not set; use 0 to disable the check.
    #[arg(long, value_name = "BLOCKS", env = "NEO_HEALTH_MAX_HEADER_LAG")]
    health_max_header_lag: Option<u32>,

    /// Enable state root calculation and validation.
    /// When enabled, the node will:
    /// - Calculate local state roots after each block is persisted
    /// - Validate state roots received from the network against designated state validators
    /// - Track both local and validated state root indices
    #[arg(long, env = "NEO_STATE_ROOT", alias = "stateroot")]
    state_root: bool,

    /// Path to store state root data (defaults to StateRoot subdirectory of storage path).
    #[arg(long, value_name = "PATH", env = "NEO_STATE_ROOT_PATH")]
    state_root_path: Option<PathBuf>,

    /// Enable full state history tracking (keeps all historical state, increases storage usage).
    #[arg(long, env = "NEO_STATE_ROOT_FULL_STATE")]
    state_root_full_state: bool,

    /// Path to NEP-6 wallet file for validator mode.
    /// When provided with --wallet-password, the node will attempt to run as a validator
    /// if the wallet's default account is in the standby committee.
    #[arg(long, value_name = "PATH", env = "NEO_WALLET")]
    wallet: Option<PathBuf>,

    /// Password for the NEP-6 wallet file.
    /// Required when --wallet is specified.
    #[arg(long, value_name = "PASSWORD", env = "NEO_WALLET_PASSWORD")]
    wallet_password: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = NodeCli::parse();
    let mut node_config = NodeConfig::load(&cli.config)?;

    // Initialize logging
    let logging_handles = init_tracing(&node_config.logging, cli.daemon)?;
    let _log_guard = logging_handles.guard;

    // Apply CLI overrides
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
        // Ensure risky methods are disabled
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

    validate_node_config(
        &node_config,
        storage_path.as_deref(),
        backend_name.as_deref(),
        &protocol_settings,
        cli.rpc_hardened,
    )?;

    if cli.check_all {
        check_storage_access(
            backend_name.as_deref(),
            storage_path.as_deref(),
            storage_config.clone(),
        )?;
        info!(target: "neo", "configuration validated; exiting due to --check-all");
        return Ok(());
    }

    if cli.check_storage {
        check_storage_access(
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
        build = %build_feature_summary(),
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
    let store_provider = select_store_provider(backend_name.as_deref(), storage_config)?;
    if let (Some(_provider), Some(path)) = (&store_provider, &storage_path) {
        check_storage_network(path, protocol_settings.network, read_only_storage)?;
    }

    if store_provider.is_some() && storage_path.is_none() {
        let backend = backend_name.unwrap_or_else(|| "unknown".to_string());
        bail!(
            "storage backend '{}' requires a data path (--storage or [storage.path])",
            backend
        );
    }

    // Build state service settings if enabled
    let state_service_settings = if cli.state_root {
        let default_state_dir = PathBuf::from("StateRoot");
        let requested_state_dir = cli
            .state_root_path
            .clone()
            .unwrap_or_else(|| default_state_dir.clone());
        let resolved_state_dir = if requested_state_dir.is_absolute() {
            requested_state_dir
        } else if let Some(storage_root) = storage_path.as_ref() {
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
    } else {
        None
    };

    // Build validators list from protocol settings standby_committee
    let validators: Vec<neo_consensus::ValidatorInfo> = protocol_settings
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
        .collect();

    info!(
        target: "neo",
        validator_count = validators.len(),
        "loaded validators from protocol settings"
    );

    // Load validator configuration from wallet if provided
    let (validator_index, private_key) = if let (Some(wallet_path), Some(password)) =
        (&cli.wallet, &cli.wallet_password)
    {
        info!(
            target: "neo",
            wallet = %wallet_path.display(),
            "loading validator wallet"
        );

        match validator_service::load_validator_from_wallet(
            wallet_path.to_str().unwrap_or(""),
            password,
            std::sync::Arc::new(protocol_settings.clone()),
        ) {
            Ok(Some(config)) => {
                info!(
                    target: "neo",
                    validator_index = config.validator_index,
                    script_hash = %config.script_hash,
                    "validator mode enabled"
                );
                (Some(config.validator_index), config.private_key)
            }
            Ok(None) => {
                warn!(
                    target: "neo",
                    "wallet account is not in standby committee - running in non-validator mode"
                );
                (None, Vec::new())
            }
            Err(e) => {
                warn!(
                    target: "neo",
                    error = %e,
                    "failed to load validator wallet - running in non-validator mode"
                );
                (None, Vec::new())
            }
        }
    } else if cli.wallet.is_some() {
        bail!("--wallet-password is required when --wallet is specified");
    } else {
        (None, Vec::new())
    };

    // Build RuntimeConfig from NodeConfig
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
            seed_nodes: resolve_seed_nodes(&node_config.p2p.seed_nodes).await,
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

    // Create and start the node runtime
    let mut node_runtime = NodeRuntime::new(runtime_config.clone());

    info!(target: "neo", "starting neo-node runtime...");

    // Start the runtime
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
    let rpc_service = if node_config.rpc.enabled {
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
        service.set_network_magic(protocol_settings.network).await;

        if let Err(e) = service.start().await {
            error!(target: "neo", error = %e, "failed to start RPC service");
        } else {
            info!(target: "neo", address = %rpc_addr, "RPC service started");
        }

        Some(service)
    } else {
        None
    };

    // Optionally start health endpoint
    if let Some(health_port) = cli.health_port {
        let max_lag = cli
            .health_max_header_lag
            .unwrap_or(health::DEFAULT_MAX_HEADER_LAG);
        let storage_for_health = storage_path.clone();
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

struct LoggingHandles {
    guard: Option<WorkerGuard>,
}

fn init_tracing(logging: &config::LoggingSection, daemon_mode: bool) -> Result<LoggingHandles> {
    use tracing_subscriber::fmt::writer::{BoxMakeWriter, MakeWriterExt};

    if !logging.active {
        return Ok(LoggingHandles { guard: None });
    }

    let level = logging.level.as_deref().unwrap_or("info");
    let filter_spec = format!("{level},neo={level}");
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter_spec));

    let mut guard = None;

    let path_value = logging
        .file_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let file_requested = logging.file_enabled;
    let file_writer = if file_requested {
        let path = path_value.unwrap_or("Logs");
        let (writer, file_guard) = create_file_writer(path)?;
        guard = Some(file_guard);
        Some(writer)
    } else {
        None
    };

    let has_file = file_writer.is_some();
    let console_enabled = logging.console_output && !daemon_mode;

    let writer: BoxMakeWriter = match (file_writer, console_enabled) {
        (Some(file), true) => BoxMakeWriter::new(io::stderr.and(file)),
        (Some(file), false) => BoxMakeWriter::new(file),
        (None, true) => BoxMakeWriter::new(io::stderr),
        (None, false) => BoxMakeWriter::new(io::sink),
    };

    let builder = fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .with_ansi(console_enabled && !has_file);

    let normalized = logging
        .format
        .as_deref()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "text".to_string());

    match normalized.as_str() {
        "json" => {
            let _ = builder.json().try_init();
        }
        "pretty" => {
            let _ = builder.pretty().try_init();
        }
        _ => {
            let _ = builder.try_init();
        }
    }
    Ok(LoggingHandles { guard })
}

fn create_file_writer(path: &str) -> Result<(non_blocking::NonBlocking, WorkerGuard)> {
    let provided = Path::new(path);
    let file_path = if provided.is_file() || provided.extension().is_some() {
        provided.to_path_buf()
    } else {
        fs::create_dir_all(provided)
            .with_context(|| format!("failed to create log directory {}", provided.display()))?;
        provided.join(default_log_name())
    };

    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log directory {}", parent.display()))?;
        }
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
        .with_context(|| format!("failed to open log file {}", file_path.display()))?;
    Ok(non_blocking(file))
}

fn default_log_name() -> String {
    format!("neo-node-{}.log", Local::now().format("%Y-%m-%d"))
}

/// Resolves seed node hostnames to socket addresses via DNS lookup
async fn resolve_seed_nodes(seeds: &[String]) -> Vec<std::net::SocketAddr> {
    use tokio::net::lookup_host;

    let mut resolved = Vec::new();
    for seed in seeds {
        // Try direct parse first (for IP:port format)
        if let Ok(addr) = seed.parse() {
            resolved.push(addr);
            continue;
        }

        // DNS lookup for hostname:port format
        match lookup_host(seed).await {
            Ok(addrs) => {
                // Take the first resolved address
                if let Some(addr) = addrs.into_iter().next() {
                    info!(target: "neo", seed = %seed, resolved = %addr, "resolved seed node");
                    resolved.push(addr);
                } else {
                    warn!(target: "neo", seed = %seed, "DNS lookup returned no addresses");
                }
            }
            Err(e) => {
                warn!(target: "neo", seed = %seed, error = %e, "failed to resolve seed node");
            }
        }
    }
    resolved
}

#[allow(clippy::useless_vec)]
fn build_feature_summary() -> String {
    #[allow(unused_mut)]
    let mut features = vec![
        "plugins: dbft,rpc-server,rocksdb-store,tokens-tracker,application-logs,sqlite-wallet",
    ];

    #[cfg(feature = "tee")]
    features.push("tee: enabled");

    #[cfg(feature = "tee-sgx")]
    features.push("tee-sgx: hardware");

    features.join("; ")
}

fn select_store_provider(
    backend: Option<&str>,
    storage_config: StorageConfig,
) -> Result<Option<Arc<dyn IStoreProvider>>> {
    let Some(name) = backend else {
        return Ok(None);
    };

    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "memory" | "mem" | "inmemory" => Ok(None),
        "rocksdb" | "rocksdbstore" | "rocksdb-store" => {
            let provider: Arc<dyn IStoreProvider> =
                Arc::new(RocksDBStoreProvider::new(storage_config));
            Ok(Some(provider))
        }
        other => bail!("unsupported storage backend '{}'", other),
    }
}

fn check_storage_network(path: &str, magic: u32, read_only: bool) -> Result<()> {
    let storage_path = Path::new(path);
    if !storage_path.exists() {
        if read_only {
            bail!("storage path {} does not exist (read-only mode)", path);
        }
        fs::create_dir_all(storage_path)
            .with_context(|| format!("failed to create storage path {}", path))?;
    }

    let marker = storage_path.join("NETWORK_MAGIC");
    if marker.exists() {
        let contents = fs::read_to_string(&marker)
            .with_context(|| format!("failed to read network marker {}", marker.display()))?;
        let parsed = contents.trim_start_matches("0x").trim().to_string();
        let stored_magic = u32::from_str_radix(&parsed, 16)
            .or_else(|_| parsed.parse::<u32>())
            .with_context(|| format!("invalid magic in {}: {}", marker.display(), contents))?;
        if stored_magic != magic {
            bail!(
                "storage at {} was initialized for network magic 0x{:08x}, but current config is 0x{:08x}. Use a fresh storage path or clear the directory.",
                path,
                stored_magic,
                magic
            );
        }
    } else {
        if read_only {
            bail!(
                "storage path {} missing NETWORK_MAGIC marker (read-only mode)",
                path
            );
        }
        fs::write(&marker, format!("0x{magic:08x}"))
            .with_context(|| format!("failed to write network marker {}", marker.display()))?;
    }

    let version_marker = storage_path.join("VERSION");
    if version_marker.exists() {
        let contents = fs::read_to_string(&version_marker).with_context(|| {
            format!(
                "failed to read storage version marker {}",
                version_marker.display()
            )
        })?;
        let stored_version = contents.trim();
        if stored_version != STORAGE_VERSION {
            bail!(
                "storage at {} was created with version '{}', current binary is '{}'. Use a fresh storage path or migrate data.",
                path,
                stored_version,
                STORAGE_VERSION
            );
        }
    } else {
        if read_only {
            bail!(
                "storage path {} missing VERSION marker (read-only mode)",
                path
            );
        }
        fs::write(&version_marker, STORAGE_VERSION).with_context(|| {
            format!(
                "failed to write storage version marker {}",
                version_marker.display()
            )
        })?;
    }
    Ok(())
}

fn is_public_bind(bind: &str) -> bool {
    bind.parse::<std::net::IpAddr>()
        .map(|ip| !ip.is_loopback() && !ip.is_unspecified())
        .unwrap_or(true)
}

fn validate_node_config(
    node_config: &NodeConfig,
    storage_path: Option<&str>,
    backend_name: Option<&str>,
    protocol_settings: &ProtocolSettings,
    rpc_hardened: bool,
) -> Result<()> {
    if node_config.rpc.auth_enabled
        && (node_config.rpc.rpc_user.is_none() || node_config.rpc.rpc_pass.is_none())
    {
        bail!("rpc.auth_enabled requires both rpc_user and rpc_pass");
    }

    if rpc_hardened && (node_config.rpc.rpc_user.is_none() || node_config.rpc.rpc_pass.is_none()) {
        bail!("--rpc-hardened requires rpc_user and rpc_pass (set via config or env)");
    }

    if node_config.rpc.enabled && !node_config.rpc.auth_enabled {
        let bind = node_config
            .rpc
            .bind_address
            .as_deref()
            .unwrap_or("127.0.0.1");
        if is_public_bind(bind) {
            warn!(
                target: "neo",
                bind_address = bind,
                "RPC is enabled on a non-loopback address without auth; enable auth or front with a proxy"
            );
        }
    }

    if let Some(name) = backend_name {
        let normalized = name.trim().to_ascii_lowercase();
        let requires_path = matches!(
            normalized.as_str(),
            "rocksdb" | "rocksdbstore" | "rocksdb-store"
        );
        if requires_path && storage_path.map(|p| p.trim().is_empty()).unwrap_or(true) {
            bail!(
                "storage backend '{}' requires a data path (--storage or [storage.path])",
                name
            );
        }
    }

    if let Some(path) = storage_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = Path::new(trimmed);
            if candidate.is_file() {
                bail!(
                    "storage path '{}' is a file; provide a directory path",
                    trimmed
                );
            }
        }
    }

    if let Some(canonical) = node_config
        .network
        .network_type
        .as_deref()
        .and_then(infer_magic_from_type)
    {
        if canonical != protocol_settings.network {
            warn!(
                target: "neo",
                network_type = ?node_config.network.network_type,
                configured_magic = format_args!("0x{:08x}", protocol_settings.network),
                canonical_magic = format_args!("0x{:08x}", canonical),
                "network type and magic differ; ensure this is intentional"
            );
        }
    }

    Ok(())
}

fn check_storage_access(
    backend: Option<&str>,
    storage_path: Option<&str>,
    storage_config: StorageConfig,
) -> Result<()> {
    let provider = select_store_provider(backend, storage_config)?;
    let Some(provider) = provider else {
        info!(target: "neo", "storage check: memory backend selected; nothing to validate");
        return Ok(());
    };

    let path = storage_path
        .ok_or_else(|| anyhow::anyhow!("storage check: no path provided for backend"))?;

    let store = provider
        .get_store(path)
        .map_err(|err| anyhow::anyhow!("storage check: failed to open store at {path}: {err}"))?;
    drop(store);
    info!(target: "neo", path, "storage check: backend opened successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_requires_storage_path_for_rocksdb() {
        let mut cfg = NodeConfig::default();
        cfg.storage.backend = Some("rocksdb".into());
        let err =
            validate_node_config(&cfg, None, Some("rocksdb"), &cfg.protocol_settings(), false)
                .expect_err("should fail without storage path");
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("storage backend"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_accepts_memory_without_path() {
        let cfg = NodeConfig::default();
        validate_node_config(&cfg, None, Some("memory"), &cfg.protocol_settings(), false)
            .expect("memory backend should not require path");
    }

    #[test]
    fn validate_enforces_rpc_auth_credentials() {
        let mut cfg = NodeConfig::default();
        cfg.rpc.enabled = true;
        cfg.rpc.auth_enabled = true;
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), false)
            .expect_err("missing rpc credentials should error");
        assert!(
            err.to_string().to_ascii_lowercase().contains("rpc_user"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_rejects_storage_path_that_is_file() {
        let tmp = tempfile::NamedTempFile::new().expect("temp file");
        let path_str = tmp.path().to_string_lossy().to_string();

        let mut cfg = NodeConfig::default();
        cfg.storage.backend = Some("rocksdb".into());
        let err = validate_node_config(
            &cfg,
            Some(&path_str),
            Some("rocksdb"),
            &cfg.protocol_settings(),
            false,
        )
        .expect_err("file path should be rejected");
        assert!(
            err.to_string().to_ascii_lowercase().contains("file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_hardened_requires_credentials() {
        let cfg = NodeConfig::default();
        let err = validate_node_config(&cfg, None, None, &cfg.protocol_settings(), true)
            .expect_err("hardened mode without credentials should fail");
        assert!(
            err.to_string().to_ascii_lowercase().contains("rpc_user"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn check_storage_allows_memory_without_path() {
        check_storage_access(Some("memory"), None, StorageConfig::default())
            .expect("memory backend should skip validation");
    }

    #[test]
    fn check_storage_succeeds_with_rocksdb_path() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let db_path = tmp.path().join("rocksdb-check");
        let cfg = StorageConfig {
            path: db_path.clone(),
            ..Default::default()
        };

        check_storage_access(
            Some("rocksdb"),
            Some(db_path.to_string_lossy().as_ref()),
            cfg,
        )
        .expect("rocksdb backend should open successfully");
    }

    #[test]
    fn check_storage_network_writes_markers() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("store");
        let path_str = path.to_string_lossy().to_string();

        check_storage_network(&path_str, 0x1234_5678, false).expect("check storage network");

        let magic = fs::read_to_string(path.join("NETWORK_MAGIC")).expect("read magic");
        assert!(magic.contains("0x12345678"));

        let version = fs::read_to_string(path.join("VERSION")).expect("read version");
        assert_eq!(version.trim(), STORAGE_VERSION);
    }

    #[test]
    fn check_storage_network_readonly_requires_markers() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let path = tmp.path().join("store");
        fs::create_dir_all(&path).expect("create dir");
        // Missing markers should fail
        let err = check_storage_network(path.to_str().unwrap(), 0x1, true)
            .expect_err("missing markers should fail in read-only");
        assert!(err.to_string().to_ascii_lowercase().contains("marker"));

        // Add markers and succeed
        fs::write(path.join("NETWORK_MAGIC"), "0x00000001").expect("write magic");
        fs::write(path.join("VERSION"), STORAGE_VERSION).expect("write version");
        check_storage_network(path.to_str().unwrap(), 0x1, true)
            .expect("markers present should pass");
    }
}
