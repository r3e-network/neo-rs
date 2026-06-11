//! Neo N3 node daemon composition root.
//!
//! Wires the workspace's subsystems into a runnable node:
//!
//! 1. **Config** — parses the shipped TOML node configuration
//!    (`[network] [storage] [p2p] [rpc]` …) and derives the consensus
//!    [`ProtocolSettings`] from the configured network type (TestNet /
//!    MainNet presets, or a custom magic).
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
//! Post-handshake P2P message dispatch (block/header sync) and dBFT
//! consensus participation are not yet wired here; a node started today
//! establishes peer connections and serves RPC over its local state.

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
    #[serde(default)]
    consensus: ConsensusSection,
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
    #[serde(default)]
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
    #[serde(default)]
    port: Option<u16>,
    /// Address to bind the listener to (default `0.0.0.0`).
    #[serde(default)]
    bind_address: Option<String>,
    /// Seed node endpoints (`host:port`) to dial on startup. Falls back
    /// to the protocol preset's seed list when empty.
    #[serde(default)]
    seed_nodes: Vec<String>,
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
            let state = ledger.get_transaction_state(&self.snapshot, tx_hash).ok()??;
            transactions.push(state.transaction?);
        }
        Some(neo_payloads::Block::from_parts(trimmed.header, transactions))
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

    let RunningNode {
        node,
        network,
        mut handles,
    } = build_node(Arc::clone(&settings), &config, cli.storage_path.as_deref())
        .await
        .context("failed to construct neo-system Node")?;
    info!(target: "neo", "neo-system Node built; blockchain service running");

    // ----- P2P listener -----
    let p2p_port = config.p2p.port.unwrap_or(default_p2p_port(settings.network));
    let p2p_bind = config.p2p.bind_address.as_deref().unwrap_or("0.0.0.0");
    match format!("{p2p_bind}:{p2p_port}").parse::<SocketAddr>() {
        Ok(bind_addr) => match network.start(bind_addr).await {
            Ok(()) => info!(target: "neo", %bind_addr, "P2P listener started"),
            Err(err) => warn!(target: "neo", %bind_addr, error = %err, "failed to start P2P listener"),
        },
        Err(err) => warn!(target: "neo", addr = %format!("{p2p_bind}:{p2p_port}"), error = %err, "invalid P2P bind address"),
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
    Ok((settings, config))
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
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::store::Store;
    use neo_storage::persistence::{StoreCache, StoreProvider};
    use neo_storage_rocksdb::RocksDBStoreProvider;
    use parking_lot::Mutex;

    // ----- storage backend -----
    let backend = config.storage.backend.as_deref().unwrap_or("memory");
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

    // Natives are dispatched through the global provider.
    neo_native_contracts::install();

    let store_cache = StoreCache::new_from_store(Arc::clone(&store), false);
    let snapshot = Arc::new(store_cache.data_cache().clone());
    // The consensus driver reads the ledger tip from this startup snapshot for
    // its first round only; subsequent rounds restart off RuntimeEvent::Imported.
    let consensus_snapshot = Arc::clone(&snapshot);

    let mempool = Arc::new(neo_mempool::MemoryPool::new(&settings));
    let header_cache = Arc::new(HeaderCache::default());

    // A second handle on the shared snapshot serves peers' block requests, and
    // the shared mempool answers `Inv`/`Mempool`/`GetData` for unconfirmed txs.
    let block_source: Arc<dyn neo_network::BlockSource> = Arc::new(LedgerBlockSource {
        snapshot: Arc::clone(&snapshot),
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
        Arc::new(LedgerContext::default()),
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
    )?;
    let consensus_active = consensus_setup
        .as_ref()
        .is_some_and(|s| s.my_index.is_some());
    // Validators + network magic the forwarder uses to decode/authenticate
    // inbound dBFT extensible payloads.
    let consensus_decode = consensus_setup
        .as_ref()
        .filter(|_| consensus_active)
        .map(|s| (s.validators.clone(), s.network));
    let (consensus_inbound_tx, consensus_inbound_rx) = if consensus_active {
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
    let (inv_tx, mut inv_rx) =
        tokio::sync::mpsc::channel::<neo_network::InboundInventory>(1024);

    // ----- P2P service -----
    let (net_service, network) = neo_network::LocalNodeService::new(Arc::clone(&settings));
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
                            if let Some(cp) = crate::consensus::extensible_to_consensus(
                                &payload,
                                *network_magic,
                                validators,
                            ) {
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
    // Non-validator nodes return `None` here (they still decode + relay dBFT
    // payloads via the forwarder above, but produce no blocks).
    if let (Some(setup), Some(inbound_rx)) = (consensus_setup, consensus_inbound_rx) {
        if let Some(handle) = crate::consensus::spawn_consensus_driver(
            setup,
            blockchain.clone(),
            Arc::clone(&mempool),
            network.clone(),
            consensus_snapshot,
            inbound_rx,
        ) {
            info!(target: "neo", "dBFT consensus driver started (validator node)");
            handles.push(handle);
        }
    }

    // ----- ledger height -> network advertisement -----
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
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,neo=debug"));
    let _ = fmt().with_env_filter(filter).try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

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
max_connections = 64
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
        assert!(!settings.standby_committee.is_empty(), "preset seeds a committee");

        // Operational sections the daemon wires.
        assert_eq!(config.storage.backend.as_deref(), Some("rocksdb"));
        assert_eq!(
            config.storage.data_dir.as_deref(),
            Some(std::path::Path::new("./data/testnet"))
        );
        assert_eq!(config.p2p.port, Some(20333));
        assert_eq!(config.p2p.seed_nodes.len(), 2);
        assert!(config.rpc.enabled);
        assert_eq!(config.rpc.port, Some(20332));
        assert_eq!(config.rpc.bind_address.as_deref(), Some("127.0.0.1"));
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
        use neo_storage::persistence::{store::Store, StoreCache};
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
}
