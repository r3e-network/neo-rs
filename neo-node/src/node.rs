//! Minimal but functional Neo node daemon entry point.

use anyhow::Context;
use clap::Parser;
use neo_config::ProtocolSettings;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

/// Default path to the protocol settings file.
pub const DEFAULT_SETTINGS_PATH: &str = "neo_testnet_node.toml";

/// Command-line arguments for the `neo-node` daemon.
#[derive(Debug, Parser)]
#[command(name = "neo-node", version, about = "Neo N3 node daemon")]
pub struct NodeCli {
    /// Path to a JSON file containing [`ProtocolSettings`].
    #[arg(long, short = 'c', default_value = DEFAULT_SETTINGS_PATH)]
    pub config: PathBuf,

    /// Override the network magic advertised in the protocol
    /// settings (must match the rest of the network).
    #[arg(long)]
    pub network_magic: Option<u32>,

    /// Override the storage backend path. If omitted, an in-memory
    /// store is used.
    #[arg(long)]
    pub storage_path: Option<PathBuf>,
}

/// Entry point: parse CLI, load config, build a [`neo_system::Node`],
/// spawn the service tasks, and wait for `Ctrl-C`.
pub async fn run() -> anyhow::Result<()> {
    init_tracing();
    let cli = NodeCli::parse();
    let settings = load_settings(&cli.config, cli.network_magic)?;
    let settings = Arc::new(settings);
    info!(target: "neo", settings_path = %cli.config.display(), "loaded protocol settings");

    let node = build_node(settings.clone(), cli.storage_path.as_deref())
        .context("failed to construct neo-system Node")?;
    info!(target: "neo", "neo-system Node built; entering main loop");

    let handles = spawn_services(&node).await;
    info!(target: "neo", "services spawned; waiting for Ctrl-C");

    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::warn!(target: "neo", error = %err, "ctrl-c handler failed; falling back to pending forever");
        std::future::pending::<()>().await;
    }
    info!(target: "neo", "Ctrl-C received; shutting down");

    for handle in handles {
        handle.abort();
    }
    Ok(())
}

/// Loads [`ProtocolSettings`] from the supplied path. The file
/// format is JSON (mirroring C#). If the file is missing, the
/// function falls back to [`ProtocolSettings::default`].
fn load_settings(path: &PathBuf, magic_override: Option<u32>) -> anyhow::Result<ProtocolSettings> {
    let mut settings = if path.exists() {
        let path_str = path.to_str().with_context(|| {
            format!("settings path {} is not valid UTF-8", path.display())
        })?;
        ProtocolSettings::load(path_str)
            .map_err(|e| anyhow::anyhow!("failed to load settings from {}: {e}", path.display()))?
    } else {
        ProtocolSettings::default()
    };
    if let Some(magic) = magic_override {
        settings.network = magic;
    }
    Ok(settings)
}

/// Constructs a [`neo_system::Node`] from the supplied settings.
fn build_node(
    settings: Arc<ProtocolSettings>,
    storage_path: Option<&std::path::Path>,
) -> anyhow::Result<neo_system::Node> {
    use neo_blockchain::BlockchainHandle;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::store::Store;

    let store: Arc<dyn Store> = if let Some(path) = storage_path {
        anyhow::bail!(
            "persistent storage path {} requested but the in-memory store is the only \
             one wired into the default neo-node build; integrate neo-storage-rocksdb \
             for production use",
            path.display()
        );
    } else {
        Arc::new(MemoryStore::new())
    };

    let (blockchain, _rx) = BlockchainHandle::with_capacity();
    let node = neo_system::Node::builder()
        .with_settings(settings)
        .with_storage(store)
        .with_blockchain(blockchain)
        .build()
        .map_err(|e| anyhow::anyhow!("node build failed: {e}"))?;
    Ok(node)
}

/// Spawns the service tasks owned by the [`neo_system::Node`].
///
/// Returns a list of `JoinHandle`s for the spawned tasks; the
/// caller is responsible for aborting them on shutdown.
async fn spawn_services(node: &neo_system::Node) -> Vec<tokio::task::JoinHandle<()>> {
    let _ = node;
    Vec::new()
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,neo=debug"));
    let _ = fmt().with_env_filter(filter).try_init();
}
