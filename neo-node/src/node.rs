//! Minimal but functional Neo node daemon entry point.
//!
//! The composition mirrors the production wiring proven by the RPC
//! server fixture (`neo-rpc/src/server/test_support.rs`): one shared
//! store snapshot, one shared [`neo_mempool::MemoryPool`], a live
//! [`neo_blockchain::BlockchainService`] loop driving the C#
//! `Blockchain.Persist` pipeline (genesis bootstrap on an empty store,
//! native OnPersist/PostPersist, per-transaction execution, ledger
//! records), and a [`neo_system::Node`] composed over the same pieces.

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

/// [`neo_blockchain::service_context::SystemContext`] for the daemon:
/// protocol settings plus the canonical store snapshot the blockchain
/// service persists blocks into (and verifies transactions against).
struct DaemonContext {
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<neo_storage::persistence::DataCache>,
}

impl std::fmt::Debug for DaemonContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonContext")
            .field("network", &self.settings.network)
            .finish_non_exhaustive()
    }
}

impl neo_blockchain::service_context::SystemContext for DaemonContext {
    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        // The Ledger current-block pointer written by the persist
        // pipeline (zero before genesis lands).
        neo_native_contracts::LedgerContract::new()
            .current_index(&self.snapshot)
            .unwrap_or(0)
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::persistence::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }
}

/// Entry point: parse CLI, load config, build a [`neo_system::Node`]
/// with a live blockchain service, bootstrap genesis on an empty
/// store, and wait for `Ctrl-C`.
pub async fn run() -> anyhow::Result<()> {
    init_tracing();
    let cli = NodeCli::parse();
    let settings = load_settings(&cli.config, cli.network_magic)?;
    let settings = Arc::new(settings);
    info!(target: "neo", settings_path = %cli.config.display(), "loaded protocol settings");

    let (node, handles) = build_node(settings.clone(), cli.storage_path.as_deref())
        .await
        .context("failed to construct neo-system Node")?;
    let _ = &node;
    info!(target: "neo", "neo-system Node built; blockchain service running");

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

/// Constructs a [`neo_system::Node`] with a live
/// [`neo_blockchain::BlockchainService`] loop, mirroring the
/// production composition proven by the RPC server fixture:
///
/// 1. one store + one canonical [`neo_storage::persistence::DataCache`] snapshot;
/// 2. one shared [`neo_mempool::MemoryPool`] (admission runs the full
///    C# `Transaction.Verify` pipeline against the snapshot);
/// 3. the blockchain service spawned on its command loop;
/// 4. `BlockchainCommand::Initialize` queued so an empty store gets
///    the genesis block persisted (C# `Blockchain.OnInitialize`).
async fn build_node(
    settings: Arc<ProtocolSettings>,
    storage_path: Option<&std::path::Path>,
) -> anyhow::Result<(neo_system::Node, Vec<tokio::task::JoinHandle<()>>)> {
    use neo_blockchain::service::{BlockchainService, MempoolLike};
    use neo_blockchain::service_context::SystemContext;
    use neo_blockchain::{HeaderCache, LedgerContext};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::store::Store;
    use neo_storage::persistence::StoreCache;
    use parking_lot::Mutex;

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

    // The engine-based persist pipeline dispatches natives through the
    // global provider.
    neo_native_contracts::install();

    // Canonical store snapshot: a write-through DataCache over the
    // store. `DataCache` clones share inner state, so the blockchain
    // service, the verification seam, and RPC reads all observe the
    // same view.
    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());

    let mempool = Arc::new(neo_mempool::MemoryPool::new(&settings));
    let header_cache = Arc::new(HeaderCache::default());

    let system_ctx: Arc<dyn SystemContext> = Arc::new(DaemonContext {
        settings: Arc::clone(&settings),
        snapshot,
    });
    // The same Arc<MemoryPool> serves blockchain admission (via the
    // SharedMempool adapter) and node/RPC reads.
    let mempool_like: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(
        neo_blockchain::service::SharedMempool(Arc::clone(&mempool)),
    ));
    let (service, blockchain) = BlockchainService::with_defaults(
        system_ctx,
        Arc::new(LedgerContext::default()),
        Arc::clone(&header_cache),
        mempool_like,
    );

    let mut handles = Vec::new();
    handles.push(tokio::spawn(service.run()));

    // C# Blockchain.OnInitialize: persist the genesis block when the
    // chain state is uninitialized (native deploy seeds + mints + the
    // ledger genesis records).
    blockchain
        .tell(neo_blockchain::BlockchainCommand::Initialize)
        .await
        .map_err(|_| anyhow::anyhow!("blockchain service command loop closed during init"))?;

    // Network handle: the command/event channels exist so the node
    // composes; starting listeners + seed dialing is the next
    // build-out step (the receivers are parked until then).
    let (network, _net_cmd_rx, _net_event_tx) = neo_network::NetworkHandle::channel(64, 64);

    let node = neo_system::Node::builder()
        .with_settings(settings)
        .with_storage(store)
        .with_blockchain(blockchain)
        .with_network(network)
        .with_mempool(mempool)
        .with_header_cache(header_cache)
        .build()
        .map_err(|e| anyhow::anyhow!("node build failed: {e}"))?;
    Ok((node, handles))
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,neo=debug"));
    let _ = fmt().with_env_filter(filter).try_init();
}
