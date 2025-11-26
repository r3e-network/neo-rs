#[allow(dead_code)]
mod commands;
mod config;
#[allow(dead_code)]
mod console;
#[allow(dead_code)]
mod console_service;

use anyhow::{bail, Context, Result};
use chrono::Local;
use clap::Parser;
use commands::{
    block::BlockCommands, blockchain::BlockchainCommands, command_line::CommandLine,
    contracts::ContractCommands, logger::LoggerCommands, native::NativeCommands,
    nep17::Nep17Commands, network::NetworkCommands, node::NodeCommands, plugins::PluginCommands,
    tools::ToolCommands, vote::VoteCommands, wallet::WalletCommands,
};
use config::NodeConfig;
use neo_core::{
    neo_system::NeoSystem,
    persistence::{providers::RocksDBStoreProvider, storage::StorageConfig, IStoreProvider},
    protocol_settings::ProtocolSettings,
    wallets::IWalletProvider,
};
#[allow(unused_imports)]
use neo_plugins as _;
use std::{
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{signal, task};
use tracing::{error, info, warn};
use tracing_appender::{non_blocking, non_blocking::WorkerGuard};
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

    /// Maximum broadcast history entries to retain in memory (0 disables capture).
    #[arg(long, value_name = "N")]
    broadcast_history_limit: Option<usize>,

    /// Disables compression for outbound connections.
    #[arg(long)]
    disable_compression: bool,

    /// Overrides the block time in seconds.
    #[arg(long, value_name = "SECONDS")]
    block_time: Option<u64>,

    /// Disables verification when importing `chain*.acc` bootstrap files.
    #[arg(long)]
    no_verify_import: bool,

    /// Unlocks the specified NEP-6 wallet at startup.
    #[arg(long, value_name = "PATH")]
    wallet: Option<PathBuf>,

    /// Password used to decrypt the unlocked wallet (requires `--wallet`).
    #[arg(long, value_name = "PASSWORD")]
    password: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut node_config = NodeConfig::load(&cli.config)?;
    let logging_handles = init_tracing(&node_config.logging)?;
    let console_log_state = logging_handles.console_enabled.clone();
    let _log_guard = logging_handles.guard;

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

    let storage_config = node_config.storage_config();
    let storage_path = cli
        .storage
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| node_config.storage_path());
    let backend_name = node_config.storage_backend().map(|name| name.to_string());

    let protocol_settings: ProtocolSettings = node_config.protocol_settings();
    if let Some(canonical) = node_config
        .network
        .network_type
        .as_deref()
        .and_then(config::infer_magic_from_type)
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
    let wallet_settings = Arc::new(protocol_settings.clone());
    if let Some(path) = node_config.write_rpc_server_plugin_config(&protocol_settings)? {
        info!(
            target: "neo",
            path = %path.display(),
            "rpc server configuration emitted"
        );
    }

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

    let system: Arc<NeoSystem> = NeoSystem::new(
        protocol_settings,
        store_provider.clone(),
        storage_path.clone(),
    )
    .map_err(anyhow::Error::new)?;
    log_registered_plugins().await;

    let wallet_commands = Arc::new(WalletCommands::new(wallet_settings, Arc::clone(&system)));
    let wallet_provider: Arc<dyn IWalletProvider + Send + Sync> =
        Arc::clone(&wallet_commands) as Arc<dyn IWalletProvider + Send + Sync>;
    system
        .attach_wallet_provider(wallet_provider)
        .map_err(anyhow::Error::new)?;
    let plugin_commands = Arc::new(PluginCommands::new(&node_config.plugins));
    let block_commands = Arc::new(BlockCommands::new(Arc::clone(&system)));
    let blockchain_commands = Arc::new(BlockchainCommands::new(Arc::clone(&system)));
    let native_commands = Arc::new(NativeCommands::new(Arc::clone(&system)));
    let node_commands = Arc::new(NodeCommands::new(Arc::clone(&system)));
    let network_commands = Arc::new(NetworkCommands::new(Arc::clone(&system)));
    let nep17_commands = Arc::new(Nep17Commands::new(
        Arc::clone(&system),
        Arc::clone(&wallet_commands),
    ));
    let logger_commands = Arc::new(LoggerCommands::new(console_log_state));
    let tool_commands = Arc::new(ToolCommands::new(Arc::new(system.settings().clone())));
    let contract_commands = Arc::new(ContractCommands::new(
        Arc::clone(&system),
        Arc::clone(&wallet_commands),
    ));
    let vote_commands = Arc::new(VoteCommands::new(
        Arc::clone(&system),
        Arc::clone(&wallet_commands),
        Arc::clone(&contract_commands),
    ));
    let verify_import = !cli.no_verify_import;
    if let Err(err) = block_commands.import_default_chain_files(verify_import) {
        warn!(
            target: "neo",
            error = %err,
            "failed to import bootstrap blocks; continuing without import"
        );
    }

    let mut wallet_path = cli.wallet.clone();
    let mut wallet_password = cli.password.clone();

    if wallet_path.is_none() && node_config.unlock_wallet.is_active {
        match &node_config.unlock_wallet.path {
            Some(path) if !path.is_empty() => {
                wallet_path = Some(PathBuf::from(path));
            }
            _ => error!(
                target: "neo",
                "unlock_wallet.is_active is true but no wallet path was provided"
            ),
        }

        if wallet_password.is_none() {
            match &node_config.unlock_wallet.password {
                Some(pass) => wallet_password = Some(pass.clone()),
                None => error!(
                    target: "neo",
                    "unlock_wallet.is_active is true but no wallet password was provided"
                ),
            }
        }
    }

    if let Some(wallet_path) = wallet_path.as_deref() {
        let password = wallet_password.as_deref().unwrap_or("");
        wallet_commands
            .open_wallet(wallet_path, password)
            .map_err(|err| anyhow::anyhow!(err))?;
    }

    system
        .start_node(node_config.channels_config())
        .map_err(anyhow::Error::new)?;

    info!(
        target: "neo",
        network = format!("{:#X}", system.settings().network),
        backend = backend_name.as_deref().unwrap_or("memory"),
        storage = storage_path
            .as_deref()
            .unwrap_or("<in-memory>"),
        "neo-rs node started; press Ctrl+C to exit"
    );

    let stdin_is_tty = io::stdin().is_terminal();

    let command_line = Arc::new(CommandLine::new(
        wallet_commands,
        plugin_commands,
        logger_commands,
        block_commands,
        blockchain_commands,
        native_commands,
        node_commands,
        network_commands,
        nep17_commands,
        tool_commands,
        contract_commands,
        vote_commands,
    ));

    let mut shell_handle = if stdin_is_tty {
        Some(task::spawn_blocking({
            let command_line = Arc::clone(&command_line);
            move || command_line.run_shell()
        }))
    } else {
        None
    };

    if let Some(mut handle) = shell_handle.take() {
        tokio::select! {
            result = signal::ctrl_c() => {
                if let Err(err) = result {
                    error!(target: "neo", error = %err, "failed to wait for shutdown signal");
                } else {
                    info!(target: "neo", "shutdown signal received (Ctrl+C)");
                }
                handle.abort();
                if let Err(err) = handle.await {
                    warn!(target: "neo", error = %err, "console task did not shut down cleanly");
                }
            }
            shell = &mut handle => {
                match shell {
                    Ok(Ok(())) => info!(target: "neo", "console session ended"),
                    Ok(Err(err)) => error!(target: "neo", error = %err, "console session failed"),
                    Err(err) => error!(target: "neo", error = %err, "console task error"),
                }
            }
        }
    } else if let Err(err) = signal::ctrl_c().await {
        error!(target: "neo", error = %err, "failed to wait for shutdown signal");
    } else {
        info!(target: "neo", "shutdown signal received (Ctrl+C)");
    }

    info!(target: "neo", "stopping neo system");
    system.shutdown().await.map_err(anyhow::Error::new)?;
    info!(target: "neo", "shutdown complete");
    Ok(())
}

fn init_tracing(logging: &config::LoggingSection) -> Result<LoggingHandles> {
    use tracing_subscriber::fmt::writer::{BoxMakeWriter, MakeWriterExt};

    if !logging.active {
        return Ok(LoggingHandles {
            guard: None,
            console_enabled: None,
        });
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
    let console_enabled = Arc::new(AtomicBool::new(logging.console_output));
    let console_writer = ConsoleToggleWriter::new(Arc::clone(&console_enabled));
    let writer: BoxMakeWriter = match file_writer {
        Some(file) => BoxMakeWriter::new(console_writer.and(file)),
        None => BoxMakeWriter::new(console_writer),
    };

    let builder = fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .with_ansi(logging.console_output && !has_file);

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
    Ok(LoggingHandles {
        guard,
        console_enabled: Some(console_enabled),
    })
}

struct LoggingHandles {
    guard: Option<WorkerGuard>,
    console_enabled: Option<Arc<AtomicBool>>,
}

#[derive(Clone)]
struct ConsoleToggleWriter {
    enabled: Arc<AtomicBool>,
}

impl ConsoleToggleWriter {
    fn new(enabled: Arc<AtomicBool>) -> Self {
        Self { enabled }
    }
}

struct ConditionalConsoleWriter {
    enabled: Arc<AtomicBool>,
    stderr: io::Stderr,
}

impl<'a> tracing_subscriber::fmt::writer::MakeWriter<'a> for ConsoleToggleWriter {
    type Writer = ConditionalConsoleWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ConditionalConsoleWriter {
            enabled: Arc::clone(&self.enabled),
            stderr: io::stderr(),
        }
    }
}

impl Write for ConditionalConsoleWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.enabled.load(Ordering::Relaxed) {
            self.stderr.write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.enabled.load(Ordering::Relaxed) {
            self.stderr.flush()
        } else {
            Ok(())
        }
    }
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
    format!("neo-cli-{}.log", Local::now().format("%Y-%m-%d"))
}

fn build_feature_summary() -> String {
    "plugins: dbft,rpc-server,rocksdb-store,tokens-tracker,application-logs,sqlite-wallet"
        .to_string()
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
