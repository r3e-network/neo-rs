#[allow(dead_code)]
mod commands;
mod config;
#[allow(dead_code)]
mod console;
#[allow(dead_code)]
mod console_service;

use anyhow::{bail, Result};
use clap::Parser;
use commands::command_line::CommandLine;
use commands::wallet::WalletCommands;
use config::NodeConfig;
use neo_core::{
    neo_system::NeoSystem,
    persistence::{providers::RocksDBStoreProvider, storage::StorageConfig, IStoreProvider},
    protocol_settings::ProtocolSettings,
};
#[allow(unused_imports)]
use neo_plugins as _;
use std::{path::PathBuf, sync::Arc};
use tokio::{signal, task};
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "neo-cli", about = "Neo N3 node (Rust) command-line interface")]
struct Cli {
    /// Path to the TOML configuration file (matches the Neo CLI format).
    #[arg(long, default_value = "neo_mainnet_node.toml", value_name = "PATH")]
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

    /// Disables compression for outbound connections.
    #[arg(long)]
    disable_compression: bool,

    /// Overrides the block time in seconds.
    #[arg(long, value_name = "SECONDS")]
    block_time: Option<u64>,

    /// Unlocks the specified NEP-6 wallet at startup.
    #[arg(long, value_name = "PATH")]
    wallet: Option<PathBuf>,

    /// Password used to decrypt the unlocked wallet (requires `--wallet`).
    #[arg(long, value_name = "PASSWORD")]
    password: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let mut node_config = NodeConfig::load(&cli.config)?;

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

    if cli.disable_compression {
        node_config.p2p.enable_compression = Some(false);
    }

    if let Some(seconds) = cli.block_time {
        node_config.blockchain.block_time = Some(seconds);
    }

    if let Some(backend) = &cli.backend {
        node_config.storage.backend = Some(backend.clone());
    }

    let storage_config = node_config.storage_config();
    let storage_path = cli
        .storage
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| node_config.storage_path());

    let protocol_settings: ProtocolSettings = node_config.protocol_settings();
    let wallet_settings = Arc::new(protocol_settings.clone());
    if let Some(path) = node_config.write_rpc_server_plugin_config(&protocol_settings)? {
        info!(
            target: "neo",
            path = %path.display(),
            "rpc server configuration emitted"
        );
    }

    let backend_name = node_config.storage_backend().map(|name| name.to_string());
    let store_provider = select_store_provider(backend_name.as_deref(), storage_config)?;

    if store_provider.is_some() && storage_path.is_none() {
        let backend = backend_name.unwrap_or_else(|| "unknown".to_string());
        bail!(
            "storage backend '{}' requires a data path (--storage or [storage.path])",
            backend
        );
    }

    let system: Arc<NeoSystem> = NeoSystem::new(
        protocol_settings,
        store_provider.clone(),
        storage_path.clone(),
    )
    .map_err(|err| anyhow::Error::new(err))?;

    let wallet_commands = Arc::new(WalletCommands::new(wallet_settings));

    if let Some(wallet_path) = cli.wallet.as_deref() {
        let password = cli.password.as_deref().unwrap_or("");
        wallet_commands
            .open_wallet(wallet_path, password)
            .map_err(|err| anyhow::anyhow!(err))?;
    }

    system
        .start_node(node_config.channels_config())
        .map_err(|err| anyhow::Error::new(err))?;

    info!(
        target: "neo",
        network = format!("{:#X}", system.settings().network),
        backend = backend_name.as_deref().unwrap_or("memory"),
        storage = storage_path
            .as_deref()
            .unwrap_or("<in-memory>"),
        "neo-rs node started; press Ctrl+C to exit"
    );

    let command_line = Arc::new(CommandLine::new(wallet_commands));

    let shell_handle = task::spawn_blocking({
        let command_line = Arc::clone(&command_line);
        move || command_line.run_shell()
    });

    tokio::select! {
        result = signal::ctrl_c() => {
            if let Err(err) = result {
                error!(target: "neo", error = %err, "failed to wait for shutdown signal");
            } else {
                info!(target: "neo", "shutdown signal received (Ctrl+C)");
            }
        }
        shell = shell_handle => {
            match shell {
                Ok(Ok(())) => info!(target: "neo", "console session ended"),
                Ok(Err(err)) => error!(target: "neo", error = %err, "console session failed"),
                Err(err) => error!(target: "neo", error = %err, "console task error"),
            }
        }
    }

    info!(target: "neo", "shutdown signal received, exiting");
    Ok(())
}

fn init_tracing() {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,neo=info"));
    let _ = fmt().with_env_filter(env_filter).try_init();
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
