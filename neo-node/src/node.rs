//! Neo N3 node daemon composition root.
//!
//! Wires the workspace's subsystems into a runnable node:
//!
//! 1. **Config** — parses the shipped TOML node configuration
//!    (`[network] [storage] [p2p] [rpc] [blockchain] [mempool]` …)
//!    and derives the consensus [`ProtocolSettings`] from the configured
//!    network type (TestNet / MainNet presets, or a custom magic).
//! 2. **Storage** — opens a persistent RocksDB store when
//!    `[storage].backend = "rocksdb"` (or `--storage-path` is given),
//!    otherwise an in-memory store.
//! 3. **Ledger** — one shared store snapshot, one shared
//!    [`neo_mempool::MemoryPool`], and a live
//!    [`neo_blockchain::BlockchainService`] driving the C#
//!    `Blockchain.Persist` pipeline (genesis bootstrap on an empty
//!    store, native OnPersist/PostPersist, per-tx execution).
//! 4. **P2P** — spawns the [`neo_network::LocalNodeService`], binds the
//!    configured TCP listener, and dials the configured seed nodes.
//! 5. **RPC** — when `[rpc].enabled`, starts the JSON-RPC server with
//!    the full provider handler set over the shared [`neo_system::Node`].
//!
//! Post-handshake P2P inventory dispatch is wired into the blockchain
//! service, and dBFT consensus participation can be enabled through the
//! `[consensus]` section for validator nodes.

use anyhow::Context;
use clap::Parser;
use neo_config::ProtocolSettings;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

/// Default path to the node configuration file.
pub const DEFAULT_SETTINGS_PATH: &str = "neo_testnet_node.toml";

/// Command-line arguments for the `neo-node` daemon.
#[derive(Debug, Parser)]
#[command(name = "neo-node", version, about = "Neo N3 node daemon")]
pub struct NodeCli {
    /// Path to the TOML node configuration file.
    #[arg(long, short = 'c', default_value = DEFAULT_SETTINGS_PATH)]
    pub config: PathBuf,

    /// Override the network magic advertised in the protocol settings
    /// (must match the rest of the network).
    #[arg(long)]
    pub network_magic: Option<u32>,

    /// Override the persistent storage directory. Implies the RocksDB
    /// backend regardless of the configured `[storage].backend`.
    #[arg(long)]
    pub storage_path: Option<PathBuf>,

    /// Validate the node configuration and exit without starting services.
    #[arg(long)]
    pub check_config: bool,

    /// Validate the configured storage backend can be opened and exit.
    #[arg(long)]
    pub check_storage: bool,

    /// Run all preflight checks and exit.
    #[arg(long)]
    pub check_all: bool,
}

// ===================== TOML node configuration =====================
//
// Every field is optional (`#[serde(default)]`); unknown sections and
// keys are ignored, so a config carrying extra blocks the daemon does
// not consume yet (`[consensus] [telemetry] [logging] …`) still parses.

/// The daemon's TOML configuration surface.
#[derive(Debug, Default, Deserialize)]
struct NodeConfig {
    #[serde(default)]
    network: NetworkSection,
    #[serde(default)]
    storage: StorageSection,
    #[serde(default)]
    p2p: P2pSection,
    #[serde(default)]
    rpc: RpcSection,
    #[serde(default, alias = "dbft")]
    consensus: ConsensusSection,
    #[serde(default)]
    blockchain: BlockchainSection,
    #[serde(default)]
    mempool: MempoolSection,
}

/// `[network]`: which Neo network the node joins.
#[derive(Debug, Default, Deserialize)]
struct NetworkSection {
    /// `"TestNet"` / `"MainNet"` — selects the built-in protocol preset.
    #[serde(default)]
    network_type: Option<String>,
    /// Explicit network magic override (wins over the preset).
    #[serde(default)]
    network_magic: Option<u32>,
}

/// `[storage]`: persistence backend.
#[derive(Debug, Default, Deserialize)]
struct StorageSection {
    /// `"rocksdb"` for a persistent store, anything else for in-memory.
    #[serde(default, alias = "Engine")]
    backend: Option<String>,
    /// Database directory for the RocksDB backend.
    #[serde(default)]
    data_dir: Option<PathBuf>,
    /// Alias for `data_dir` accepted by the shipped mainnet/production presets
    /// (which use `[storage] path = "..."`).
    #[serde(default)]
    path: Option<PathBuf>,
}

impl StorageSection {
    /// The configured RocksDB directory, accepting either `data_dir` or `path`.
    fn data_directory(&self) -> Option<PathBuf> {
        self.data_dir.clone().or_else(|| self.path.clone())
    }
}

/// `[p2p]`: peer-to-peer networking.
#[derive(Debug, Default, Deserialize)]
struct P2pSection {
    /// TCP port the node listens on for inbound peers.
    #[serde(default, alias = "listen_port", alias = "Port")]
    port: Option<u16>,
    /// Address to bind the listener to (default `0.0.0.0`).
    #[serde(default)]
    bind_address: Option<String>,
    /// Seed node endpoints (`host:port`) to dial on startup. Falls back
    /// to the protocol preset's seed list when empty.
    #[serde(default)]
    seed_nodes: Vec<String>,
    /// Whether P2P message compression is advertised/enabled.
    #[serde(default, alias = "EnableCompression")]
    enable_compression: Option<bool>,
    /// Minimum desired outbound peer count.
    #[serde(default, alias = "MinDesiredConnections")]
    min_desired_connections: Option<usize>,
    /// Maximum simultaneous peer count. `-1` matches C# "unlimited".
    #[serde(default, alias = "MaxConnections")]
    max_connections: Option<i64>,
    /// Maximum simultaneous peers accepted from one remote IP.
    #[serde(default, alias = "MaxConnectionsPerAddress")]
    max_connections_per_address: Option<usize>,
    /// Maximum known inventory hashes retained for duplicate suppression.
    #[serde(default, alias = "MaxKnownHashes")]
    max_known_hashes: Option<usize>,
    /// Maximum recent broadcasts retained for diagnostics.
    #[serde(default)]
    broadcast_history_limit: Option<usize>,
}

impl P2pSection {
    /// Build the live channel configuration consumed by `LocalNodeService`.
    fn channels_config(&self) -> anyhow::Result<neo_network::ChannelsConfig> {
        let mut config = neo_network::ChannelsConfig::default();

        if let Some(enable_compression) = self.enable_compression {
            config.enable_compression = enable_compression;
        }
        if let Some(min_desired_connections) = self.min_desired_connections {
            config.min_desired_connections = min_desired_connections;
        }
        if let Some(max_connections) = self.max_connections {
            config.max_connections = match max_connections {
                -1 => usize::MAX,
                value if value >= 0 => usize::try_from(value)
                    .context("invalid [p2p].max_connections: value is too large")?,
                _ => {
                    anyhow::bail!(
                        "invalid [p2p].max_connections: use -1 for unlimited or a non-negative integer"
                    )
                }
            };
        }
        if let Some(max_connections_per_address) = self.max_connections_per_address {
            config.max_connections_per_address = max_connections_per_address;
        }
        if let Some(max_known_hashes) = self.max_known_hashes {
            config.max_known_hashes = max_known_hashes;
        }
        if let Some(broadcast_history_limit) = self.broadcast_history_limit {
            config.broadcast_history_limit = broadcast_history_limit;
        }

        Ok(config)
    }
}

/// `[rpc]`: JSON-RPC server.
#[derive(Debug, Default, Deserialize)]
struct RpcSection {
    /// Whether to start the RPC server.
    #[serde(default)]
    enabled: bool,
    /// RPC listen port (default `10332`).
    #[serde(default)]
    port: Option<u16>,
    /// RPC bind address (default `127.0.0.1`).
    #[serde(default)]
    bind_address: Option<String>,
}

/// `[consensus]`: dBFT consensus participation.
#[derive(Debug, Default, Deserialize)]
struct ConsensusSection {
    /// Whether this node participates in dBFT consensus. When `true`, the
    /// node decodes inbound dBFT extensible payloads and — if its key is in
    /// the validator set — drives the round lifecycle and produces blocks.
    #[serde(default)]
    enabled: bool,
    /// This node's 32-byte secp256r1 private key, hex-encoded. Required when
    /// `enabled = true`. The node is a validator only if the derived public
    /// key is in the protocol's validator set; otherwise it relays only.
    #[serde(default)]
    private_key_hex: Option<String>,
    /// Optional HSM-backed consensus signing (PKCS#11). When set, the node signs
    /// consensus messages via the HSM instead of `private_key_hex`. Requires the
    /// node to be built with `--features hsm`.
    #[serde(default)]
    hsm: Option<crate::consensus::HsmKeyConfig>,
}

/// `[blockchain]`: protocol settings that affect validation / production.
#[derive(Debug, Default, Deserialize)]
struct BlockchainSection {
    /// Block interval in milliseconds (`ProtocolSettings.MillisecondsPerBlock`).
    #[serde(
        default,
        alias = "milliseconds_per_block",
        alias = "MillisecondsPerBlock"
    )]
    block_time: Option<u32>,
    /// Maximum transactions accepted in one block.
    #[serde(default, alias = "MaxTransactionsPerBlock")]
    max_transactions_per_block: Option<u32>,
    /// Maximum `ValidUntilBlock` increment for transactions.
    #[serde(
        default,
        alias = "max_valid_until_block_increment",
        alias = "MaxValidUntilBlockIncrement"
    )]
    max_valid_until_block_increment: Option<u32>,
    /// Maximum number of traceable blocks exposed to contracts.
    #[serde(default, alias = "MaxTraceableBlocks")]
    max_traceable_blocks: Option<u32>,
}

/// `[mempool]`: transaction pool sizing.
#[derive(Debug, Default, Deserialize)]
struct MempoolSection {
    /// Maximum number of transactions retained in the memory pool.
    #[serde(
        default,
        alias = "memory_pool_max_transactions",
        alias = "MemoryPoolMaxTransactions"
    )]
    max_transactions: Option<i32>,
}

/// [`neo_blockchain::service_context::SystemContext`] for the daemon:
/// protocol settings plus the canonical store snapshot the blockchain
/// service persists blocks into (and verifies transactions against).
struct DaemonContext {
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<neo_storage::persistence::DataCache>,
    /// The store-backed cache whose `DataCache` shares state with `snapshot`
    /// (cloned from it). `commit()` flushes the block writes accumulated in the
    /// snapshot through to the durable backing store — the write-through the
    /// blockchain service triggers via `commit_to_store()` after each block.
    store_cache: parking_lot::Mutex<neo_storage::persistence::StoreCache>,
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
        neo_native_contracts::LedgerContract::new()
            .current_index(&self.snapshot)
            .unwrap_or(0)
    }

    fn store_snapshot(&self) -> Option<Arc<neo_storage::persistence::DataCache>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn commit_to_store(&self) {
        // The StoreCache's DataCache shares state with `snapshot` (it was cloned
        // from it), so its tracked block writes are flushed through to the store.
        self.store_cache.lock().commit();
    }
}

/// Read-only ledger view that serves peers' block requests
/// ([`neo_network::BlockSource`]) by reconstructing a full block from the
/// persistent store: `index → hash → TrimmedBlock → transactions`
/// (the C# `NativeContract.Ledger.GetBlock(snapshot, index)` path).
struct LedgerBlockSource {
    snapshot: Arc<neo_storage::persistence::DataCache>,
    /// Blockchain relay cache for accepted extensible payloads (dBFT and
    /// state-service messages).
    ledger: Arc<neo_blockchain::LedgerContext>,
    /// The shared mempool, so `Inv`/`Mempool` gossip can answer for
    /// unconfirmed transactions (which are not yet in the ledger snapshot).
    mempool: Arc<neo_mempool::MemoryPool>,
}

impl LedgerBlockSource {
    /// Reconstructs the full block stored under `hash`: header + the
    /// transactions referenced by its `TrimmedBlock`.
    fn full_block(
        &self,
        ledger: &neo_native_contracts::LedgerContract,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Block> {
        let trimmed = ledger.get_trimmed_block(&self.snapshot, hash).ok()??;
        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for tx_hash in &trimmed.hashes {
            let state = ledger
                .get_transaction_state(&self.snapshot, tx_hash)
                .ok()??;
            transactions.push(state.transaction?);
        }
        Some(neo_payloads::Block::from_parts(
            trimmed.header,
            transactions,
        ))
    }
}

impl neo_network::BlockSource for LedgerBlockSource {
    fn block_by_index(&self, index: u32) -> Option<neo_payloads::Block> {
        let ledger = neo_native_contracts::LedgerContract::new();
        let hash = ledger.get_block_hash(&self.snapshot, index).ok()??;
        self.full_block(&ledger, &hash)
    }

    fn header_by_index(&self, index: u32) -> Option<neo_payloads::Header> {
        let ledger = neo_native_contracts::LedgerContract::new();
        let hash = ledger.get_block_hash(&self.snapshot, index).ok()??;
        let trimmed = ledger.get_trimmed_block(&self.snapshot, &hash).ok()??;
        Some(trimmed.header)
    }

    fn block_hash_by_index(&self, index: u32) -> Option<neo_primitives::UInt256> {
        neo_native_contracts::LedgerContract::new()
            .get_block_hash(&self.snapshot, index)
            .ok()
            .flatten()
    }

    fn block_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<neo_payloads::Block> {
        self.full_block(&neo_native_contracts::LedgerContract::new(), hash)
    }

    fn block_index_by_hash(&self, hash: &neo_primitives::UInt256) -> Option<u32> {
        neo_native_contracts::LedgerContract::new()
            .get_trimmed_block(&self.snapshot, hash)
            .ok()
            .flatten()
            .map(|trimmed| trimmed.header.index())
    }

    fn transaction_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::Transaction> {
        // Serve unconfirmed transactions from the mempool first (C# `GetData`
        // serves `MemoryPool` entries), then fall back to the ledger.
        if let Some(item) = self.mempool.get(hash) {
            return Some((*item.transaction).clone());
        }
        neo_native_contracts::LedgerContract::new()
            .get_transaction_state(&self.snapshot, hash)
            .ok()?
            .and_then(|state| state.transaction)
    }

    fn extensible_by_hash(
        &self,
        hash: &neo_primitives::UInt256,
    ) -> Option<neo_payloads::ExtensiblePayload> {
        self.ledger.get_extensible(hash)
    }

    fn contains_transaction(&self, hash: &neo_primitives::UInt256) -> bool {
        self.mempool.contains(hash)
            || neo_native_contracts::LedgerContract::new()
                .get_transaction_state(&self.snapshot, hash)
                .ok()
                .flatten()
                .is_some()
    }

    fn mempool_transaction_hashes(&self) -> Vec<neo_primitives::UInt256> {
        self.mempool
            .verified_snapshot()
            .iter()
            .map(|item| item.hash())
            .collect()
    }
}

/// The composed, running node and the handles that keep it alive.
struct RunningNode {
    node: Arc<neo_system::Node>,
    network: neo_network::NetworkHandle,
    handles: Vec<tokio::task::JoinHandle<()>>,
}

/// Entry point: parse CLI, load config, build the node, start P2P +
/// RPC, and wait for `Ctrl-C`.
pub async fn run() -> anyhow::Result<()> {
    init_tracing();
    let cli = NodeCli::parse();
    let (settings, config) = load_config(&cli.config, cli.network_magic)?;
    let settings = Arc::new(settings);
    info!(
        target: "neo",
        network = format_args!("0x{:08X}", settings.network),
        config = %cli.config.display(),
        "loaded protocol settings"
    );
    validate_config(&config)?;

    let check_config = cli.check_config || cli.check_all;
    let check_storage = cli.check_storage || cli.check_all;
    if check_config && !check_storage {
        info!(target: "neo", config = %cli.config.display(), "configuration preflight passed");
        println!("configuration OK: {}", cli.config.display());
        return Ok(());
    }
    if check_storage {
        validate_storage(&config, cli.storage_path.as_deref())?;
        info!(target: "neo", config = %cli.config.display(), "storage preflight passed");
        println!("storage OK: {}", cli.config.display());
        return Ok(());
    }

    let RunningNode {
        node,
        network,
        mut handles,
    } = build_node(Arc::clone(&settings), &config, cli.storage_path.as_deref())
        .await
        .context("failed to construct neo-system Node")?;
    info!(target: "neo", "neo-system Node built; blockchain service running");

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
                warn!(target: "neo", %bind_addr, error = %err, "failed to start P2P listener")
            }
        },
        Err(err) => {
            warn!(target: "neo", addr = %format!("{p2p_bind}:{p2p_port}"), error = %err, "invalid P2P bind address")
        }
    }

    // ----- seed dialing (non-blocking) -----
    let seeds = if config.p2p.seed_nodes.is_empty() {
        settings.seed_list.clone()
    } else {
        config.p2p.seed_nodes.clone()
    };
    if !seeds.is_empty() {
        let dialer = network.clone();
        handles.push(tokio::spawn(async move {
            for seed in seeds {
                match tokio::net::lookup_host(&seed).await {
                    Ok(addrs) => {
                        // Dial the first resolved endpoint; the peer
                        // registry enforces the connection caps.
                        if let Some(addr) = addrs.into_iter().next() {
                            match dialer.connect_peer(addr).await {
                                Ok(id) => info!(target: "neo", %addr, ?id, "connected to seed"),
                                Err(err) => {
                                    warn!(target: "neo", %addr, error = %err, "seed dial failed")
                                }
                            }
                        }
                    }
                    Err(err) => warn!(target: "neo", seed = %seed, error = %err, "seed DNS resolution failed"),
                }
            }
        }));
    }

    // ----- RPC server -----
    let _rpc_keepalive = if config.rpc.enabled {
        Some(start_rpc_server(&node, &config, settings.network)?)
    } else {
        info!(target: "neo", "RPC server disabled in config");
        None
    };

    if let Err(err) = tokio::signal::ctrl_c().await {
        warn!(target: "neo", error = %err, "ctrl-c handler failed; falling back to pending forever");
        std::future::pending::<()>().await;
    }
    info!(target: "neo", "Ctrl-C received; shutting down");

    for handle in handles {
        handle.abort();
    }
    Ok(())
}

/// Default P2P port for a network magic (TestNet `20333`, MainNet
/// `10333`); `0` (ephemeral) for unknown networks.
fn default_p2p_port(network: u32) -> u16 {
    match network {
        0x3554_334E => 20333, // TestNet
        0x334F_454E => 10333, // MainNet
        _ => 0,
    }
}

/// Loads the TOML node configuration and derives [`ProtocolSettings`]
/// from the configured network type. A missing file yields the built-in
/// defaults (MainNet preset).
fn load_config(
    path: &PathBuf,
    magic_override: Option<u32>,
) -> anyhow::Result<(ProtocolSettings, NodeConfig)> {
    let config: NodeConfig = if path.exists() {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading node config {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("parsing TOML node config {}", path.display()))?
    } else {
        info!(target: "neo", path = %path.display(), "config not found; using built-in defaults");
        NodeConfig::default()
    };

    let mut settings = match config.network.network_type.as_deref() {
        Some(t) if t.eq_ignore_ascii_case("testnet") => ProtocolSettings::testnet(),
        Some(t) if t.eq_ignore_ascii_case("mainnet") => ProtocolSettings::mainnet(),
        Some(other) => {
            warn!(target: "neo", network_type = other, "unknown network_type; using default (MainNet) settings");
            ProtocolSettings::default()
        }
        None => ProtocolSettings::default(),
    };
    if let Some(magic) = magic_override.or(config.network.network_magic) {
        settings.network = magic;
    }
    if let Some(block_time) = config.blockchain.block_time {
        settings.milliseconds_per_block = block_time;
    }
    if let Some(max_transactions) = config.blockchain.max_transactions_per_block {
        settings.max_transactions_per_block = max_transactions;
    }
    if let Some(max_valid_until_block_increment) = config.blockchain.max_valid_until_block_increment
    {
        settings.max_valid_until_block_increment = max_valid_until_block_increment;
    }
    if let Some(max_traceable_blocks) = config.blockchain.max_traceable_blocks {
        settings.max_traceable_blocks = max_traceable_blocks;
    }
    if let Some(max_transactions) = config.mempool.max_transactions {
        settings.memory_pool_max_transactions = max_transactions;
    }
    Ok((settings, config))
}

fn validate_config(config: &NodeConfig) -> anyhow::Result<()> {
    let _ = storage_backend_name(config)?;
    let _ = config.p2p.channels_config()?;

    if let Some(bind_address) = config.p2p.bind_address.as_deref() {
        bind_address
            .parse::<std::net::IpAddr>()
            .context("invalid [p2p].bind_address")?;
    }

    if config.rpc.enabled {
        config
            .rpc
            .bind_address
            .as_deref()
            .unwrap_or("127.0.0.1")
            .parse::<std::net::IpAddr>()
            .context("invalid [rpc].bind_address")?;
    }

    Ok(())
}

fn validate_storage(config: &NodeConfig, storage_override: Option<&Path>) -> anyhow::Result<()> {
    let _store = open_store(config, storage_override)?;
    Ok(())
}

fn storage_backend_name(config: &NodeConfig) -> anyhow::Result<&str> {
    let backend = config.storage.backend.as_deref().unwrap_or("memory");
    if backend.eq_ignore_ascii_case("memory") || backend.eq_ignore_ascii_case("rocksdb") {
        Ok(backend)
    } else {
        anyhow::bail!(
            "unsupported [storage].backend {backend:?}; expected \"memory\" or \"rocksdb\""
        );
    }
}

fn open_store(
    config: &NodeConfig,
    storage_override: Option<&Path>,
) -> anyhow::Result<Arc<dyn neo_storage::persistence::store::Store>> {
    use neo_storage::persistence::StoreProvider;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::store::Store;
    use neo_storage::rocksdb::RocksDBStoreProvider;

    let backend = storage_backend_name(config)?;
    let use_rocksdb = storage_override.is_some() || backend.eq_ignore_ascii_case("rocksdb");
    let store: Arc<dyn Store> = if use_rocksdb {
        let path = storage_override
            .map(Path::to_path_buf)
            .or_else(|| config.storage.data_directory())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "storage backend 'rocksdb' requires a data directory \
                     (set [storage].data_dir or [storage].path, or pass --storage-path)"
                )
            })?;
        info!(target: "neo", path = %path.display(), "opening RocksDB store");
        let cfg = StorageConfig {
            path,
            ..Default::default()
        };
        RocksDBStoreProvider::new(cfg)
            .get_store("")
            .map_err(|e| anyhow::anyhow!("failed to open RocksDB store: {e}"))?
    } else {
        info!(target: "neo", "using in-memory store (state is not persisted across restarts)");
        Arc::new(MemoryStore::new())
    };

    Ok(store)
}

/// Constructs the [`neo_system::Node`] with a live blockchain service
/// and a spawned [`neo_network::LocalNodeService`].
async fn build_node(
    settings: Arc<ProtocolSettings>,
    config: &NodeConfig,
    storage_override: Option<&Path>,
) -> anyhow::Result<RunningNode> {
    use neo_blockchain::service::{BlockchainService, MempoolLike};
    use neo_blockchain::service_context::SystemContext;
    use neo_blockchain::{HeaderCache, LedgerContext};
    use neo_storage::persistence::StoreCache;
    use parking_lot::Mutex;

    // ----- storage backend -----
    let store = open_store(config, storage_override)?;

    // Natives are dispatched through the global provider.
    neo_native_contracts::install();

    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    // The consensus driver reads the ledger tip from this startup snapshot for
    // its first round only; subsequent rounds restart off RuntimeEvent::Imported.
    let consensus_snapshot = Arc::clone(&snapshot);
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

    // A second handle on the shared snapshot serves peers' block requests, and
    // the shared mempool answers `Inv`/`Mempool`/`GetData` for unconfirmed txs.
    let block_source: Arc<dyn neo_network::BlockSource> = Arc::new(LedgerBlockSource {
        snapshot: Arc::clone(&snapshot),
        ledger: Arc::clone(&ledger_ctx),
        mempool: Arc::clone(&mempool),
    });
    let system_ctx: Arc<dyn SystemContext> = Arc::new(DaemonContext {
        settings: Arc::clone(&settings),
        snapshot,
        store_cache: parking_lot::Mutex::new(store_cache),
    });
    let mempool_like: Arc<Mutex<dyn MempoolLike + Send + Sync>> = Arc::new(Mutex::new(
        neo_blockchain::service::SharedMempool(Arc::clone(&mempool)),
    ));
    let (service, blockchain) = BlockchainService::with_defaults(
        system_ctx,
        Arc::clone(&ledger_ctx),
        Arc::clone(&header_cache),
        mempool_like,
    );

    let mut handles = Vec::new();
    handles.push(tokio::spawn(service.run()));

    // C# Blockchain.OnInitialize: persist genesis on an empty store.
    blockchain
        .tell(neo_blockchain::BlockchainCommand::Initialize)
        .await
        .map_err(|_| anyhow::anyhow!("blockchain service command loop closed during init"))?;

    // ----- dBFT consensus participation -----
    // Build the validator set + this node's role from the protocol settings and
    // the `[consensus]` config. The driver itself is spawned after the network
    // exists (it needs the outbound relay handle); the inbound channel is set up
    // here so the forwarder can feed it decoded dBFT payloads.
    let consensus_setup = crate::consensus::build_consensus_setup(
        &settings,
        config.consensus.enabled,
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

    // ----- inbound inventory relay: peer blocks/transactions -> ledger -----
    // The network layer is decoupled from the blockchain (C# `NeoSystem`
    // mediator), so each per-peer task forwards decoded inventory over this
    // channel; the forwarder hands blocks to the blockchain service, which
    // applies the C# `Blockchain.OnNewBlock` sequencing. The forwarder is
    // spawned *after* the network exists so it can re-announce accepted
    // transactions to peers via `Inv` (C# `LocalNode.RelayDirectly`).
    let (inv_tx, mut inv_rx) = tokio::sync::mpsc::channel::<neo_network::InboundInventory>(1024);

    // ----- P2P service -----
    let channels_config = config.p2p.channels_config()?;
    let (net_service, network) =
        neo_network::LocalNodeService::with_config(Arc::clone(&settings), channels_config);
    let net_service = net_service
        .with_inventory_sink(inv_tx)
        .with_block_source(block_source);
    handles.push(tokio::spawn(net_service.run()));

    {
        let blockchain = blockchain.clone();
        let relay = network.clone();
        let consensus_decode = consensus_decode.clone();
        let consensus_inbound_tx = consensus_inbound_tx.clone();
        handles.push(tokio::spawn(async move {
            use neo_network::InboundInventory;
            while let Some(item) = inv_rx.recv().await {
                match item {
                    InboundInventory::Block(block) => {
                        let _ = blockchain
                            .tell(neo_blockchain::BlockchainCommand::InventoryBlock {
                                block,
                                relay: true,
                                pre_verified: false,
                            })
                            .await;
                    }
                    InboundInventory::Transaction(tx) => {
                        // Admit the peer's transaction to the mempool; the
                        // C# `Transaction.Verify` pipeline runs inside the
                        // blockchain service. On a fresh accept (Succeed),
                        // re-announce it to peers via `Inv` so it propagates.
                        if let Ok(reply) = blockchain.add_transaction((*tx).clone()).await {
                            if reply.result.is_success() {
                                let _ = relay
                                    .broadcast_inv(
                                        neo_p2p::InventoryType::Transaction,
                                        vec![reply.hash],
                                    )
                                    .await;
                            }
                        }
                    }
                    InboundInventory::Extensible(payload) => {
                        // dBFT consensus messages: when this node is a validator,
                        // decode + authenticate the payload and feed it to the
                        // consensus driver. (`extensible_to_consensus` returns
                        // `None` for non-dBFT or spoofed payloads.)
                        if let (Some((validators, network_magic)), Some(tx)) =
                            (&consensus_decode, &consensus_inbound_tx)
                        {
                            let cp = {
                                let validators = validators.read();
                                crate::consensus::extensible_to_consensus(
                                    &payload,
                                    *network_magic,
                                    &validators,
                                )
                            };
                            if let Some(cp) = cp {
                                let _ = tx.send(cp).await;
                            }
                        }
                        // Cache + relay through the blockchain service regardless
                        // (peers that are validators consume it; we relay it on).
                        let _ = blockchain
                            .tell(neo_blockchain::BlockchainCommand::InventoryExtensible {
                                payload: (*payload).clone(),
                                relay: true,
                            })
                            .await;
                    }
                }
            }
        }));
    }

    // ----- dBFT consensus driver -----
    // Spawn the round-driving task now that the network relay handle exists.
    // A configured key that is not in the current validator set stays idle but
    // keeps tracking imports so it can participate after a committee change.
    if let (Some(setup), Some(inbound_rx)) = (consensus_setup, consensus_inbound_rx) {
        if let Some(handle) = crate::consensus::spawn_consensus_driver(
            setup,
            blockchain.clone(),
            Arc::clone(&mempool),
            network.clone(),
            Arc::clone(&settings),
            consensus_validators.expect("configured consensus has validators"),
            consensus_snapshot,
            inbound_rx,
        ) {
            info!(target: "neo", "dBFT consensus driver started (validator node)");
            handles.push(handle);
        }
    }

    // ----- ledger height -> network advertisement -----
    // Seed the advertised height from the DURABLE tip before P2P sync starts,
    // so a node restarted on a populated store advertises its real height and
    // the block-sync cursor (`local_height + 1`) resumes from the persisted tip
    // instead of re-requesting the entire chain from block 1.
    let _ = network.set_block_height(durable_tip).await;
    info!(target: "neo", height = durable_tip, "advertised durable ledger tip to peers");

    // As the ledger persists blocks, advertise the new height to peers
    // (version + ping) so block-sync requests advance their cursor and
    // peers learn our progress (C# `LocalNode` reads `Ledger.CurrentIndex`).
    {
        let mut events = blockchain.subscribe();
        let network = network.clone();
        handles.push(tokio::spawn(async move {
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
        }));
    }

    let node = neo_system::Node::builder()
        .with_settings(settings)
        .with_storage(store)
        .with_blockchain(blockchain)
        .with_network(network.clone())
        .with_mempool(mempool)
        .with_header_cache(header_cache)
        .build()
        .map_err(|e| anyhow::anyhow!("node build failed: {e}"))?;

    Ok(RunningNode {
        node: Arc::new(node),
        network,
        handles,
    })
}

/// Builds the RPC server over the shared node, registers the full
/// provider handler set, and starts it on the configured endpoint.
/// Returns the server handle (kept alive for the node's lifetime).
fn start_rpc_server(
    node: &Arc<neo_system::Node>,
    config: &NodeConfig,
    network_magic: u32,
) -> anyhow::Result<Arc<parking_lot::RwLock<neo_rpc::server::RpcServer>>> {
    use neo_rpc::server::{
        RpcServer, RpcServerApplicationLogs, RpcServerBlockchain, RpcServerConfig, RpcServerNode,
        RpcServerOracle, RpcServerSmartContract, RpcServerState, RpcServerTokensTracker,
        RpcServerUtilities, RpcServerWallet,
    };

    let bind_address: std::net::IpAddr = config
        .rpc
        .bind_address
        .as_deref()
        .unwrap_or("127.0.0.1")
        .parse()
        .context("invalid [rpc].bind_address")?;
    let port = config.rpc.port.unwrap_or(10332);

    let rpc_config = RpcServerConfig {
        network: network_magic,
        bind_address,
        port,
        ..Default::default()
    };

    let mut server = RpcServer::new(Arc::clone(node), rpc_config);
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerNode::register_handlers());
    server.register_handlers(RpcServerState::register_handlers());
    server.register_handlers(RpcServerWallet::register_handlers());
    server.register_handlers(RpcServerUtilities::register_handlers());
    server.register_handlers(RpcServerSmartContract::register_handlers());
    // C#-optional plugin method groups (ApplicationLogs, TokensTracker,
    // OracleService): registered by default so the daemon serves the full
    // 55-method C# RPC surface RPC operators expect.
    server.register_handlers(RpcServerApplicationLogs::register_handlers());
    server.register_handlers(RpcServerTokensTracker::register_handlers());
    server.register_handlers(RpcServerOracle::register_handlers());

    let server = Arc::new(parking_lot::RwLock::new(server));
    let weak = Arc::downgrade(&server);
    server.write().start_rpc_server(weak, None);
    info!(target: "neo", %bind_address, port, "RPC server started");
    Ok(server)
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,neo=debug"));
    let _ = fmt().with_env_filter(filter).try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use neo_network::wire::{Message, MessageCodec};
    use neo_p2p::MessageCommand;
    use neo_payloads::p2p_payloads::{GetBlockByIndexPayload, NodeCapability, VersionPayload};
    use std::time::Duration;
    use tokio::net::TcpStream;
    use tokio_util::codec::Framed;

    const TEST_TIMEOUT: Duration = Duration::from_secs(10);

    type FakeFramed = Framed<TcpStream, MessageCodec>;

    async fn fake_dial(port: u16) -> FakeFramed {
        let stream = tokio::time::timeout(TEST_TIMEOUT, TcpStream::connect(("127.0.0.1", port)))
            .await
            .expect("dial timed out")
            .expect("dial failed");
        Framed::new(stream, MessageCodec::new())
    }

    async fn recv_frame(framed: &mut FakeFramed) -> Message {
        match tokio::time::timeout(TEST_TIMEOUT, framed.next()).await {
            Ok(Some(Ok(message))) => message,
            Ok(Some(Err(err))) => panic!("frame decode failed: {err}"),
            Ok(None) => panic!("connection closed while waiting for frame"),
            Err(_) => panic!("timed out waiting for frame"),
        }
    }

    fn decode_payload<T: neo_io::Serializable>(message: &Message) -> T {
        let mut reader = neo_io::MemoryReader::new(&message.payload_raw);
        <T as neo_io::Serializable>::deserialize(&mut reader).expect("decode payload")
    }

    fn fake_peer_version_message(network: u32, nonce: u32, height: u32) -> Message {
        let payload = VersionPayload::create(
            network,
            nonce,
            "/fake-peer:0.0.1/".to_string(),
            vec![
                NodeCapability::full_node(height),
                NodeCapability::tcp_server(20333),
            ],
        );
        Message::create(MessageCommand::Version, Some(&payload), false).expect("encode version")
    }

    fn verack_message() -> Message {
        Message::from_payload_bytes(MessageCommand::Verack, Vec::new(), false)
            .expect("encode verack")
    }

    async fn recv_getblockbyindex(fake: &mut FakeFramed) -> GetBlockByIndexPayload {
        loop {
            let frame = recv_frame(fake).await;
            if frame.command == MessageCommand::GetBlockByIndex {
                return decode_payload(&frame);
            }
        }
    }

    fn empty_child_block(parent: &neo_payloads::Block, index: u32) -> neo_payloads::Block {
        let mut header = neo_payloads::Header::new();
        header.set_index(index);
        header.set_prev_hash(parent.hash());
        header.set_timestamp(parent.header.timestamp() + 15_000);
        header.set_next_consensus(*parent.header.next_consensus());
        header.witness = neo_payloads::Witness::new_with_scripts(
            Vec::new(),
            vec![neo_vm_rs::OpCode::PUSH1.byte()],
        );
        neo_payloads::Block::from_parts(header, Vec::new())
    }

    fn seed_rocksdb_tip(path: &Path, settings: &ProtocolSettings, tip: u32) -> anyhow::Result<()> {
        use neo_storage::persistence::StoreCache;

        neo_native_contracts::install();

        let config = NodeConfig::default();
        let store = open_store(&config, Some(path))?;
        let mut store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
        let snapshot = Arc::new(store_cache.data_cache().clone());

        let mut parent = Arc::new(neo_blockchain::genesis_block(settings)?);
        neo_blockchain::persist_block_natives(
            Arc::clone(&snapshot),
            Arc::clone(&parent),
            settings,
        )?;

        for index in 1..=tip {
            let block = Arc::new(empty_child_block(parent.as_ref(), index));
            neo_blockchain::persist_block_natives(
                Arc::clone(&snapshot),
                Arc::clone(&block),
                settings,
            )?;
            parent = block;
        }

        let current_index = neo_native_contracts::LedgerContract::new()
            .current_index(&snapshot)
            .expect("seeded ledger current index");
        assert_eq!(current_index, tip);
        store_cache
            .try_commit()
            .map_err(|err| anyhow::anyhow!("commit seeded RocksDB store: {err}"))?;

        Ok(())
    }

    fn unused_local_rpc_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral RPC port");
        listener.local_addr().expect("local RPC address").port()
    }

    async fn rpc_post_json(port: u16, request: serde_json::Value) -> serde_json::Value {
        let client = reqwest::Client::new();
        let response = tokio::time::timeout(
            TEST_TIMEOUT,
            client
                .post(format!("http://127.0.0.1:{port}/"))
                .json(&request)
                .send(),
        )
        .await
        .expect("RPC request timed out")
        .expect("RPC request failed");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        response
            .json::<serde_json::Value>()
            .await
            .expect("parse RPC response JSON")
    }

    /// A representative TOML config (mirroring the shipped
    /// `neo_testnet_node.toml`) parses, derives the TestNet protocol
    /// preset, and exposes the operational sections the daemon wires.
    #[test]
    fn load_config_parses_testnet_toml_and_derives_settings() {
        let toml = r#"
[network]
network_type = "TestNet"
network_magic = 0x3554334E

[storage]
backend = "rocksdb"
data_dir = "./data/testnet"
read_only = false
cache_size = 2048

[p2p]
port = 20333
enable_compression = true
min_desired_connections = 10
max_connections = 40
max_connections_per_address = 3
max_known_hashes = 1000
seed_nodes = ["seed1t5.neo.org:20333", "seed2t5.neo.org:20333"]

[rpc]
enabled = true
port = 20332
bind_address = "127.0.0.1"

# Sections the daemon does not consume yet must still parse.
[consensus]
enabled = false

[telemetry.metrics]
enabled = false
"#;
        let dir = std::env::temp_dir();
        let path = dir.join(format!("neo_node_cfg_test_{}.toml", std::process::id()));
        std::fs::write(&path, toml).expect("write temp config");

        let (settings, config) = load_config(&path, None).expect("load config");
        std::fs::remove_file(&path).ok();

        // TestNet preset derived from network_type, magic applied.
        assert_eq!(settings.network, 0x3554_334E);
        assert!(
            !settings.standby_committee.is_empty(),
            "preset seeds a committee"
        );

        // Operational sections the daemon wires.
        assert_eq!(config.storage.backend.as_deref(), Some("rocksdb"));
        assert_eq!(
            config.storage.data_dir.as_deref(),
            Some(std::path::Path::new("./data/testnet"))
        );
        assert_eq!(config.p2p.port, Some(20333));
        assert_eq!(config.p2p.seed_nodes.len(), 2);
        let channels = config.p2p.channels_config().expect("p2p channels");
        assert!(channels.enable_compression);
        assert_eq!(channels.min_desired_connections, 10);
        assert_eq!(channels.max_connections, 40);
        assert_eq!(channels.max_connections_per_address, 3);
        assert_eq!(channels.max_known_hashes, 1_000);
        assert!(config.rpc.enabled);
        assert_eq!(config.rpc.port, Some(20332));
        assert_eq!(config.rpc.bind_address.as_deref(), Some("127.0.0.1"));
    }

    /// Node TOML protocol knobs must affect the `ProtocolSettings` used by the
    /// daemon; otherwise shipped `[blockchain]` / `[mempool]` sections look
    /// meaningful while the node silently runs different consensus limits.
    #[test]
    fn load_config_applies_blockchain_and_mempool_protocol_overrides() {
        let toml = r#"
[network]
network_type = "TestNet"

[blockchain]
block_time = 1000
max_transactions_per_block = 123
max_valid_until_block_increment = 456
max_traceable_blocks = 789

[mempool]
max_transactions = 321
"#;
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "neo_node_protocol_overrides_{}.toml",
            std::process::id()
        ));
        std::fs::write(&path, toml).expect("write temp config");

        let (settings, _) = load_config(&path, None).expect("load config");
        std::fs::remove_file(&path).ok();

        assert_eq!(settings.milliseconds_per_block, 1_000);
        assert_eq!(settings.max_transactions_per_block, 123);
        assert_eq!(settings.max_valid_until_block_increment, 456);
        assert_eq!(settings.max_traceable_blocks, 789);
        assert_eq!(settings.memory_pool_max_transactions, 321);
    }

    /// Accept C#-style / older shipped TOML aliases so operational settings are
    /// not lost when configs use names like `Engine`, `Port`, or `[dbft]`.
    #[test]
    fn node_config_accepts_csharp_style_operational_aliases() {
        let toml = r#"
[storage]
Engine = "rocksdb"
path = "./data/testnet"

[p2p]
Port = 20333
EnableCompression = false
MinDesiredConnections = 2
MaxConnections = -1
MaxConnectionsPerAddress = 1
MaxKnownHashes = 77

[dbft]
enabled = true
private_key_hex = "012345"
"#;
        let config: NodeConfig = toml::from_str(toml).expect("parses aliases");

        assert_eq!(config.storage.backend.as_deref(), Some("rocksdb"));
        assert_eq!(
            config.storage.data_directory(),
            Some(std::path::PathBuf::from("./data/testnet"))
        );
        assert_eq!(config.p2p.port, Some(20333));
        let channels = config.p2p.channels_config().expect("p2p channels");
        assert!(!channels.enable_compression);
        assert_eq!(channels.min_desired_connections, 2);
        assert_eq!(channels.max_connections, usize::MAX);
        assert_eq!(channels.max_connections_per_address, 1);
        assert_eq!(channels.max_known_hashes, 77);
        assert!(config.consensus.enabled);
        assert_eq!(config.consensus.private_key_hex.as_deref(), Some("012345"));
    }

    #[test]
    fn p2p_channels_reject_invalid_negative_max_connections() {
        let config: NodeConfig = toml::from_str(
            r#"
[p2p]
max_connections = -2
"#,
        )
        .expect("parse config");

        let err = config
            .p2p
            .channels_config()
            .expect_err("rejects invalid max");
        assert!(err.to_string().contains("max_connections"));
    }

    /// The operator-facing presets checked into this repository should carry
    /// Neo N3 v3.10.0 mainnet/testnet transaction limits explicitly.
    #[test]
    fn shipped_mainnet_and_testnet_configs_match_v3100_transaction_limits() {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("neo-node has a workspace parent");
        let cases = [
            ("config/mainnet.toml", 200),
            ("config/mainnet-stateroot.toml", 200),
            ("neo_mainnet_node.toml", 200),
            ("neo_production_node.toml", 200),
            ("config/testnet.toml", 5_000),
            ("neo_testnet_node.toml", 5_000),
        ];

        for (relative, expected) in cases {
            let path = workspace.join(relative);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
            let config: NodeConfig = toml::from_str(&text)
                .unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));

            assert_eq!(
                config.blockchain.max_transactions_per_block,
                Some(expected),
                "{} must pin v3.10.0 MaxTransactionsPerBlock",
                relative
            );
        }
    }

    /// The public-network presets should mirror the Neo v3.10.0
    /// `ApplicationConfiguration.P2P` channel defaults: compression enabled,
    /// 10 desired peers, 40 max peers, 3 peers per address, and 1000 known
    /// hashes. Local/private configs may intentionally override these.
    #[test]
    fn shipped_public_configs_match_v3100_p2p_channel_defaults() {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("neo-node has a workspace parent");
        let cases = [
            ("config/mainnet.toml", Some(10333)),
            ("config/mainnet-stateroot.toml", Some(10333)),
            ("neo_mainnet_node.toml", Some(10333)),
            ("neo_production_node.toml", Some(10333)),
            ("config/testnet.toml", Some(20333)),
            ("neo_testnet_node.toml", Some(20333)),
        ];

        for (relative, expected_port) in cases {
            let path = workspace.join(relative);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
            let config: NodeConfig = toml::from_str(&text)
                .unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));
            let channels = config
                .p2p
                .channels_config()
                .unwrap_or_else(|err| panic!("build p2p channels for {}: {err}", path.display()));

            assert_eq!(config.p2p.port, expected_port, "{relative} P2P port");
            assert!(channels.enable_compression, "{relative} compression");
            assert_eq!(channels.min_desired_connections, 10, "{relative} min");
            assert_eq!(channels.max_connections, 40, "{relative} max");
            assert_eq!(
                channels.max_connections_per_address, 3,
                "{relative} per-address cap"
            );
            assert_eq!(channels.max_known_hashes, 1_000, "{relative} known hashes");
        }
    }

    /// A missing config file falls back to the built-in defaults (the
    /// MainNet preset) rather than failing.
    #[test]
    fn load_config_missing_file_uses_defaults() {
        let path = PathBuf::from("/nonexistent/neo-node/definitely-missing.toml");
        let (settings, config) = load_config(&path, None).expect("defaults");
        assert_eq!(settings.network, ProtocolSettings::default().network);
        assert!(config.p2p.seed_nodes.is_empty());
        assert!(!config.rpc.enabled);
    }

    /// The `--network-magic` CLI override wins over the preset / config.
    #[test]
    fn load_config_magic_override_wins() {
        let path = PathBuf::from("/nonexistent/neo-node/missing.toml");
        let (settings, _) = load_config(&path, Some(0x1234_5678)).expect("override");
        assert_eq!(settings.network, 0x1234_5678);
    }

    /// Unknown / extra `[storage]` keys do not break parsing.
    #[test]
    fn node_config_ignores_unknown_keys() {
        let toml = r#"
[storage]
backend = "memory"
some_future_key = 42
"#;
        let config: NodeConfig = toml::from_str(toml).expect("tolerates unknown keys");
        assert_eq!(config.storage.backend.as_deref(), Some("memory"));
    }

    #[test]
    fn node_cli_accepts_preflight_flags() {
        let cli = NodeCli::try_parse_from([
            "neo-node",
            "--config",
            "custom.toml",
            "--storage-path",
            "./data/custom",
            "--network-magic",
            "1234",
            "--check-all",
        ])
        .expect("preflight args parse");

        assert_eq!(cli.config, PathBuf::from("custom.toml"));
        assert_eq!(cli.storage_path, Some(PathBuf::from("./data/custom")));
        assert_eq!(cli.network_magic, Some(1234));
        assert!(cli.check_all);
        assert!(!cli.check_config);
        assert!(!cli.check_storage);
    }

    #[test]
    fn validate_config_rejects_unknown_storage_backend() {
        let config: NodeConfig = toml::from_str(
            r#"
[storage]
backend = "rockdb"
"#,
        )
        .expect("parse config");

        let err = validate_config(&config).expect_err("rejects typo");
        assert!(err.to_string().contains("unsupported [storage].backend"));
    }

    #[test]
    fn validate_storage_requires_rocksdb_path() {
        let config: NodeConfig = toml::from_str(
            r#"
[storage]
backend = "rocksdb"
"#,
        )
        .expect("parse config");

        let err = validate_storage(&config, None).expect_err("missing path fails");
        assert!(err.to_string().contains("requires a data directory"));
    }

    /// Default P2P ports follow the network magic.
    #[test]
    fn default_p2p_port_matches_network() {
        assert_eq!(default_p2p_port(0x3554_334E), 20333);
        assert_eq!(default_p2p_port(0x334F_454E), 10333);
        assert_eq!(default_p2p_port(0xDEAD_BEEF), 0);
    }

    /// The shipped mainnet/production presets use `[storage] path = "..."`;
    /// the parser must accept it as an alias for `data_dir`.
    #[test]
    fn storage_section_accepts_path_alias() {
        let toml = "[storage]\nbackend = \"rocksdb\"\npath = \"./data/mainnet\"\n";
        let config: NodeConfig = toml::from_str(toml).expect("parses");
        assert_eq!(config.storage.backend.as_deref(), Some("rocksdb"));
        assert_eq!(
            config.storage.data_directory(),
            Some(std::path::PathBuf::from("./data/mainnet"))
        );
    }

    /// `commit_to_store` flushes the writes accumulated in the shared snapshot
    /// (as a block's native-persist pipeline does) through to the durable store,
    /// so a fresh cache over the same store reads them. Without this, synced
    /// blocks stay in-memory and the on-disk tip is stuck at genesis.
    #[test]
    fn commit_to_store_flushes_snapshot_writes_to_durable_store() {
        use neo_blockchain::service_context::SystemContext;
        use neo_storage::persistence::providers::memory_store::MemoryStore;
        use neo_storage::persistence::{StoreCache, store::Store};
        use neo_storage::{StorageItem, StorageKey};

        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let ctx = DaemonContext {
            settings: Arc::new(ProtocolSettings::default()),
            snapshot: Arc::clone(&snapshot),
            store_cache: parking_lot::Mutex::new(store_cache),
        };

        // Stage a write into the shared snapshot (the blockchain persist path).
        let key = StorageKey::new(-1, vec![0xAB, 0xCD]);
        snapshot.add(key.clone(), StorageItem::from_bytes(vec![0x01, 0x02, 0x03]));

        // Not durable yet: a fresh cache over the same store cannot see it.
        let before = StoreCache::new_from_store(Arc::clone(&store), false);
        assert!(
            before.data_cache().get(&key).is_none(),
            "write must not reach the store before commit_to_store"
        );

        // Flush, then a fresh cache over the same store reads the write.
        ctx.commit_to_store();
        let after = StoreCache::new_from_store(Arc::clone(&store), false);
        assert!(
            after.data_cache().get(&key).is_some(),
            "commit_to_store must flush the snapshot write through to the store"
        );
    }

    /// Full daemon restart smoke test: when the durable RocksDB store already
    /// contains a ledger tip, `build_node` must read it before P2P starts,
    /// advertise it in `version`, and request blocks from `tip + 1`.
    #[tokio::test]
    async fn build_node_restarts_from_durable_rocksdb_tip_and_resumes_sync_cursor() {
        const DURABLE_TIP: u32 = 1;
        const PEER_HEIGHT: u32 = 3;

        let temp = tempfile::tempdir().expect("temp RocksDB root");
        let storage_path = temp.path().join("chain");
        let settings = Arc::new(ProtocolSettings::default());
        seed_rocksdb_tip(&storage_path, settings.as_ref(), DURABLE_TIP)
            .expect("seed durable RocksDB tip");

        let config = NodeConfig::default();
        let running = build_node(Arc::clone(&settings), &config, Some(&storage_path))
            .await
            .expect("build node over durable store");

        running
            .network
            .start("127.0.0.1:0".parse().unwrap())
            .await
            .expect("start P2P listener");
        let port = running.network.local_node_info().port();
        assert_ne!(port, 0);

        let mut fake = fake_dial(port).await;
        let node_version = recv_frame(&mut fake).await;
        assert_eq!(node_version.command, MessageCommand::Version);
        let node_version: VersionPayload = decode_payload(&node_version);
        assert!(
            node_version.capabilities.iter().any(|capability| matches!(
                capability,
                NodeCapability::FullNode {
                    start_height: DURABLE_TIP
                }
            )),
            "restarted daemon must advertise the durable ledger tip"
        );

        fake.send(fake_peer_version_message(
            settings.network,
            0xfa4e_00d0,
            PEER_HEIGHT,
        ))
        .await
        .expect("send peer version");
        let verack = recv_frame(&mut fake).await;
        assert_eq!(verack.command, MessageCommand::Verack);
        fake.send(verack_message()).await.expect("send verack");

        let request = recv_getblockbyindex(&mut fake).await;
        assert_eq!(
            request.index_start,
            DURABLE_TIP + 1,
            "restart sync cursor resumes just after the durable tip"
        );
        assert_eq!(request.count, (PEER_HEIGHT - DURABLE_TIP) as i16);

        running.network.shutdown().await.expect("shutdown network");
        for handle in running.handles {
            handle.abort();
            let _ = handle.await;
        }
        drop(running.node);
        drop(running.network);
    }

    /// Operator-facing RPC smoke test: a daemon rebuilt over a durable RocksDB
    /// ledger must expose the recovered chain height through JSON-RPC.
    ///
    /// Runs on a multi-thread runtime to match the production daemon
    /// (`#[tokio::main]`): the JSON-RPC relay path (`sendrawtransaction` /
    /// `submitblock`) uses `block_in_place`, which requires a multi-thread
    /// runtime. `getblockcount` itself does not, but the multi-thread flavor
    /// keeps this end-to-end smoke test representative of the real daemon.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rpc_getblockcount_reads_restarted_durable_rocksdb_tip() {
        const DURABLE_TIP: u32 = 1;

        let temp = tempfile::tempdir().expect("temp RocksDB root");
        let storage_path = temp.path().join("chain");
        let settings = Arc::new(ProtocolSettings::default());
        seed_rocksdb_tip(&storage_path, settings.as_ref(), DURABLE_TIP)
            .expect("seed durable RocksDB tip");

        let rpc_port = unused_local_rpc_port();
        let mut config = NodeConfig::default();
        config.rpc.enabled = true;
        config.rpc.port = Some(rpc_port);
        config.rpc.bind_address = Some("127.0.0.1".to_string());

        let running = build_node(Arc::clone(&settings), &config, Some(&storage_path))
            .await
            .expect("build node over durable store");
        let server = start_rpc_server(&running.node, &config, settings.network)
            .expect("start JSON-RPC server");
        assert!(server.read().is_started(), "JSON-RPC server must bind");

        let response = rpc_post_json(
            rpc_port,
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "getblockcount",
                "params": [],
                "id": 1
            }),
        )
        .await;
        assert_eq!(response.get("error"), None);
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"], serde_json::json!(DURABLE_TIP + 1));

        server.write().stop_rpc_server();
        drop(server);
        for handle in running.handles {
            handle.abort();
            let _ = handle.await;
        }
        drop(running.node);
        drop(running.network);
    }
}
