//! Neo Node - Standalone blockchain node daemon
//!
//! This is the Neo N3 blockchain node daemon. It runs the NeoSystem with
//! RPC server enabled, allowing external clients (like neo-cli) to interact
//! with the node via JSON-RPC.
//!
//! Usage:
//!   neo-node --config neo_mainnet_node.toml
//!
//! The node will:
//! - Start the P2P network and sync with peers
//! - Start the RPC server (if enabled in config)
//! - Optionally run with TEE protection (--tee flag)
//! - Run until Ctrl+C is received

mod config;
#[cfg(feature = "tee")]
mod tee_integration;

use anyhow::{bail, Context, Result};
use chrono::Local;
use clap::Parser;
use config::{infer_magic_from_type, NodeConfig};
use neo_core::{
    neo_system::NeoSystem,
    persistence::{providers::RocksDBStoreProvider, storage::StorageConfig, IStoreProvider},
    protocol_settings::ProtocolSettings,
};
#[allow(unused_imports)]
use neo_plugins as _;
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

#[derive(Parser, Debug)]
#[command(name = "neo-node", about = "Neo N3 blockchain node daemon", version)]
struct Cli {
    /// Path to the TOML configuration file.
    #[arg(long, short = 'c', default_value = "neo_mainnet_node.toml", value_name = "PATH")]
    config: PathBuf,

    /// Overrides the configured storage path.
    #[arg(long, value_name = "PATH")]
    storage: Option<PathBuf>,

    /// Overrides the storage backend (memory, rocksdb).
    #[arg(long, value_name = "BACKEND")]
    backend: Option<String>,

    /// Overrides the network magic used during the P2P handshake.
    #[arg(long, value_name = "MAGIC")]
    network_magic: Option<u32>,

    /// Overrides the P2P listening port.
    #[arg(long, value_name = "PORT")]
    listen_port: Option<u16>,

    /// Replaces the configured seed nodes (comma separated).
    #[arg(long = "seed", value_delimiter = ',', value_name = "HOST:PORT")]
    seed_nodes: Vec<String>,

    /// Overrides the maximum number of concurrent connections.
    #[arg(long, value_name = "N")]
    max_connections: Option<usize>,

    /// Overrides the minimum desired number of peers.
    #[arg(long, value_name = "N")]
    min_connections: Option<usize>,

    /// Overrides the per-address connection cap.
    #[arg(long, value_name = "N")]
    max_connections_per_address: Option<usize>,

    /// Maximum broadcast history entries to retain in memory.
    #[arg(long, value_name = "N")]
    broadcast_history_limit: Option<usize>,

    /// Disables compression for outbound connections.
    #[arg(long)]
    disable_compression: bool,

    /// Overrides the block time in seconds.
    #[arg(long, value_name = "SECONDS")]
    block_time: Option<u64>,

    /// Run in daemon mode (no console output except errors).
    #[arg(long, short = 'd')]
    daemon: bool,

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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
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

    // Build protocol settings
    let storage_config = node_config.storage_config();
    let storage_path = cli
        .storage
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| node_config.storage_path());
    let backend_name = node_config.storage_backend().map(|name| name.to_string());

    let protocol_settings: ProtocolSettings = node_config.protocol_settings();

    // Warn if network type and magic differ
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

    // Initialize storage provider
    let store_provider = select_store_provider(backend_name.as_deref(), storage_config)?;
    if let (Some(_provider), Some(path)) = (&store_provider, &storage_path) {
        check_storage_network(path, protocol_settings.network)?;
    }

    if store_provider.is_some() && storage_path.is_none() {
        let backend = backend_name.unwrap_or_else(|| "unknown".to_string());
        bail!(
            "storage backend '{}' requires a data path (--storage or [storage.path])",
            backend
        );
    }

    // Create and start NeoSystem
    let system: Arc<NeoSystem> = NeoSystem::new(
        protocol_settings,
        store_provider.clone(),
        storage_path.clone(),
    )
    .map_err(anyhow::Error::new)?;

    log_registered_plugins().await;

    system
        .start_node(node_config.channels_config())
        .map_err(anyhow::Error::new)?;

    info!(
        target: "neo",
        network = format!("{:#X}", system.settings().network),
        backend = backend_name.as_deref().unwrap_or("memory"),
        storage = storage_path.as_deref().unwrap_or("<in-memory>"),
        rpc_enabled = node_config.rpc.enabled,
        rpc_port = node_config.rpc.port.unwrap_or(10332),
        "neo-node started; press Ctrl+C to stop"
    );

    // Wait for shutdown signal
    if let Err(err) = signal::ctrl_c().await {
        error!(target: "neo", error = %err, "failed to wait for shutdown signal");
    } else {
        info!(target: "neo", "shutdown signal received (Ctrl+C)");
    }

    info!(target: "neo", "stopping neo system");
    system.shutdown().await.map_err(anyhow::Error::new)?;
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

fn build_feature_summary() -> String {
    let mut features = vec![
        "plugins: dbft,rpc-server,rocksdb-store,tokens-tracker,application-logs,sqlite-wallet",
    ];

    #[cfg(feature = "tee")]
    features.push("tee: enabled");

    #[cfg(feature = "tee-sgx")]
    features.push("tee-sgx: hardware");

    features.join("; ")
}

async fn log_registered_plugins() {
    let plugins = neo_extensions::plugin::global_plugin_infos().await;
    if plugins.is_empty() {
        info!(target: "neo", "no plugins registered");
    } else {
        let summary: Vec<String> = plugins
            .iter()
            .map(|p| format!("{} v{} ({:?})", p.name, p.version, p.category))
            .collect();
        info!(
            target: "neo",
            count = plugins.len(),
            plugins = %summary.join(", "),
            "plugins registered"
        );
    }
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

fn check_storage_network(path: &str, magic: u32) -> Result<()> {
    let storage_path = Path::new(path);
    if !storage_path.exists() {
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
        fs::write(&marker, format!("0x{magic:08x}"))
            .with_context(|| format!("failed to write network marker {}", marker.display()))?;
    }
    Ok(())
}
