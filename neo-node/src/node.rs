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

use clap::Parser;
use neo_config::ProtocolSettings;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

mod chain_acc;
mod config;
mod context;
mod indexer_runtime;
mod sync_metrics;
mod ledger_source;
mod logging;
mod observability;
mod rpc_runtime;
mod seeds;
mod services;
mod tasks;
mod telemetry;

use config::{
    NodeConfig, default_p2p_port, load_config, open_store, validate_config, validate_storage,
};
use context::DaemonContext;
use ledger_source::LedgerBlockSource;
use rpc_runtime::start_rpc_server;
use services::OperationalServices;
use tasks::{spawn_daemon_task, spawn_daemon_task_result};

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

    /// Import blocks from a chain.acc dump file before starting the node.
    /// The file is the C# Neo block-dump format (u32 count, then repeated
    /// i32-size + serialized-Block). Blocks are imported with verify=false
    /// (trusted source, like C# Neo's chain.acc import). After import, the
    /// node starts normally and continues syncing from the network.
    #[arg(long, value_name = "PATH")]
    pub import_chain: Option<PathBuf>,
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
    let cli = NodeCli::parse();
    let (settings, config) = load_config(&cli.config, cli.network_magic)?;
    let _logging_guards = logging::init_tracing(&config.logging)?;
    let settings = Arc::new(settings);
    info!(
        target: "neo",
        network = format_args!("0x{:08X}", settings.network),
        config = %cli.config.display(),
        "loaded protocol settings"
    );
    validate_config(&config, settings.network)?;

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

    let observability =
        observability::ObservabilityRuntime::from_config(&config.observability, settings.network)?;
    if let Some(observability) = &observability {
        observability.install_panic_hook();
    }

    let running_node = match build_node(
        Arc::clone(&settings),
        &config,
        cli.storage_path.as_deref(),
        observability.clone(),
    )
    .await
    {
        Ok(running_node) => running_node,
        Err(err) => {
            let err = err.context("failed to construct neo-system Node");
            if let Some(observability) = &observability {
                observability.report_startup_error(&err);
            }
            return Err(err);
        }
    };
    let RunningNode {
        node,
        network,
        mut handles,
    } = running_node;
    info!(target: "neo", "neo-system Node built; blockchain service running");

    // Optional: import blocks from a chain.acc file before starting live sync.
    if let Some(import_path) = &cli.import_chain {
        let blockchain = node.blockchain();
        match chain_acc::import_chain_acc(&blockchain, import_path, false).await {
            Ok(count) => {
                info!(
                    target: "neo",
                    imported = count,
                    "chain.acc import completed successfully; continuing with network sync"
                );
            }
            Err(err) => {
                warn!(
                    target: "neo",
                    error = %err,
                    "chain.acc import failed; continuing with network sync"
                );
            }
        }
    }

    match telemetry::metrics_server_task(&config.telemetry.metrics, Arc::clone(&node)) {
        Ok(Some(task)) => spawn_daemon_task_result(
            &mut handles,
            observability.as_ref(),
            "telemetry_metrics",
            task,
        ),
        Ok(None) => {}
        Err(err) => {
            let err = err.context("failed to start metrics endpoint");
            if let Some(observability) = &observability {
                observability.report_startup_error(&err);
            }
            for handle in handles {
                handle.abort();
            }
            return Err(err);
        }
    }

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
                warn!(target: "neo", %bind_addr, error = %err, "failed to start P2P listener");
                if let Some(observability) = &observability {
                    observability.report_runtime_error("p2p_listener", &err);
                }
            }
        },
        Err(err) => {
            warn!(target: "neo", addr = %format!("{p2p_bind}:{p2p_port}"), error = %err, "invalid P2P bind address");
            if let Some(observability) = &observability {
                observability.report_runtime_error("p2p_bind_address", &err);
            }
        }
    }

    let seed_nodes = if config.p2p.seed_nodes.is_empty() {
        settings.seed_list.clone()
    } else {
        config.p2p.seed_nodes.clone()
    };
    if let Some(handle) =
        seeds::spawn_seed_dialing(seed_nodes, network.clone(), observability.clone())
    {
        handles.push(handle);
    }

    // ----- RPC server -----
    let _rpc_keepalive = if config.rpc.enabled {
        match start_rpc_server(&node, &config, settings.network) {
            Ok(server) => Some(server),
            Err(err) => {
                let err = err.context("failed to start RPC server");
                if let Some(observability) = &observability {
                    observability.report_startup_error(&err);
                }
                for handle in handles {
                    handle.abort();
                }
                return Err(err);
            }
        }
    } else {
        info!(target: "neo", "RPC server disabled in config");
        None
    };
    if let Some(observability) = &observability {
        handles.extend(observability.spawn_heartbeat_tasks(Arc::clone(&node)));
    }

    // Wait for a shutdown signal, handling SIGTERM as well as Ctrl-C (SIGINT)
    // so `kill`/`pkill`, Docker, and systemd all stop the node gracefully. The
    // graceful path drops the RocksDB handle, which flushes the memtable to
    // disk; an ungraceful SIGTERM kill would otherwise leave recent blocks only
    // in the un-fsync'd memtable, so the next start would resume well behind the
    // last in-memory height.
    #[cfg(unix)]
    let shutdown_signalled: std::io::Result<&str> = {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => tokio::select! {
                res = tokio::signal::ctrl_c() => res.map(|()| "Ctrl-C"),
                _ = sigterm.recv() => Ok("SIGTERM"),
            },
            Err(_) => tokio::signal::ctrl_c().await.map(|()| "Ctrl-C"),
        }
    };
    #[cfg(not(unix))]
    let shutdown_signalled: std::io::Result<&str> =
        tokio::signal::ctrl_c().await.map(|()| "Ctrl-C");

    match shutdown_signalled {
        Ok(signal_name) => {
            info!(target: "neo", signal = signal_name, "shutdown signal received; shutting down")
        }
        Err(err) => {
            warn!(target: "neo", error = %err, "shutdown-signal handler failed; falling back to pending forever");
            if let Some(observability) = &observability {
                observability.report_runtime_error("shutdown_signal", &err);
            }
            std::future::pending::<()>().await;
        }
    }

    for handle in handles {
        handle.abort();
    }
    Ok(())
}

/// Constructs the [`neo_system::Node`] with a live blockchain service
/// and a spawned [`neo_network::LocalNodeService`].
async fn build_node(
    settings: Arc<ProtocolSettings>,
    config: &NodeConfig,
    storage_override: Option<&Path>,
    observability: Option<observability::ObservabilityRuntime>,
) -> anyhow::Result<RunningNode> {
    use neo_blockchain::service::{BlockchainService, MempoolLike};
    use neo_blockchain::service_context::SystemContext;
    use neo_blockchain::{HeaderCache, LedgerContext};
    use neo_storage::persistence::store::Store;
    use neo_storage::persistence::StoreCache;
    use parking_lot::Mutex;

    // ----- storage backend -----
    let store = open_store(config, storage_override)?;

    // Enable fast-sync optimizations during initial catch-up (disable WAL,
    // fsync, and auto-compaction) for dramatically higher write throughput.
    // The node will re-enable balanced mode once it approaches the live tip.
    // This mirrors C# Neo's behaviour during chain.acc import / bulk sync.
    let durable_tip_height = {
        let probe = StoreCache::new_from_store(Arc::clone(&store), false);
        neo_native_contracts::LedgerContract::new()
            .current_index(probe.data_cache())
            .unwrap_or(0)
    };
    if durable_tip_height == 0 {
        info!(
            target: "neo::sync",
            "enabling fast-sync store mode for initial catch-up (WAL disabled, auto-compaction off)"
        );
        store.enable_fast_sync_mode();
    }

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

    let OperationalServices {
        state_store,
        state_service,
        indexer_service,
        application_logs_service,
        tokens_tracker_service,
        tokens_tracker_runtime,
    } = services::build_operational_services(config, settings.network)?;

    // A second handle on the shared snapshot serves peers' block requests, and
    // the shared mempool answers `Inv`/`Mempool`/`GetData` for unconfirmed txs.
    let block_source: Arc<dyn neo_network::BlockSource> = Arc::new(LedgerBlockSource::new(
        Arc::clone(&snapshot),
        Arc::clone(&ledger_ctx),
        Arc::clone(&mempool),
    ));
    let daemon_ctx = Arc::new(DaemonContext::new(
        Arc::clone(&settings),
        snapshot,
        store_cache,
        state_service.clone(),
        indexer_service.clone(),
        application_logs_service.clone(),
    ));
    let system_ctx: Arc<dyn SystemContext> = daemon_ctx.clone();
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
    spawn_daemon_task(
        &mut handles,
        observability.as_ref(),
        "blockchain_service",
        service.run(),
    );

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
    let consensus_enabled = config.consensus.enabled || config.consensus.auto_start;
    let consensus_setup = crate::consensus::build_consensus_setup(
        &settings,
        consensus_enabled,
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
    spawn_daemon_task(
        &mut handles,
        observability.as_ref(),
        "p2p_service",
        net_service.run(),
    );

    {
        let blockchain = blockchain.clone();
        let relay = network.clone();
        let consensus_decode = consensus_decode.clone();
        let consensus_inbound_tx = consensus_inbound_tx.clone();
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            "inventory_relay",
            async move {
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
                                            neo_network::InventoryType::Transaction,
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
            },
        );
    }

    // ----- dBFT consensus driver -----
    // Spawn the round-driving task now that the network relay handle exists.
    // A configured key that is not in the current validator set stays idle but
    // keeps tracking imports so it can participate after a committee change.
    if let (Some(setup), Some(inbound_rx)) = (consensus_setup, consensus_inbound_rx) {
        if let Some(task) = crate::consensus::consensus_driver_task(
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
            spawn_daemon_task(
                &mut handles,
                observability.as_ref(),
                "consensus_driver",
                task,
            );
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
        spawn_daemon_task(
            &mut handles,
            observability.as_ref(),
            "network_height_advertiser",
            async move {
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
            },
        );
    }

    let service_registry = neo_system::ServiceRegistry::new();
    if let Some(state_store) = &state_store {
        service_registry.register(Arc::clone(state_store));
    }
    if let Some(indexer) = &indexer_service {
        service_registry.register(Arc::clone(indexer));
    }
    if let Some(application_logs) = &application_logs_service {
        service_registry.register(Arc::clone(application_logs));
    }
    if let Some(tokens_tracker) = &tokens_tracker_service {
        service_registry.register(Arc::clone(tokens_tracker));
    }

    let node = Arc::new(
        neo_system::Node::builder()
            .with_settings(settings)
            .with_storage(store)
            .with_blockchain(blockchain.clone())
            .with_network(network.clone())
            .with_mempool(mempool)
            .with_header_cache(header_cache)
            .with_services(service_registry)
            .build()
            .map_err(|e| anyhow::anyhow!("node build failed: {e}"))?,
    );
    daemon_ctx.set_node(Arc::clone(&node));
    if let Some((tracker_settings, tracker_store)) = tokens_tracker_runtime {
        daemon_ctx.set_tokens_tracker(Some(Arc::new(
            neo_rpc::plugins::tokens_tracker::TokensTracker::new(
                tracker_settings,
                tracker_store,
                Arc::clone(&node),
            ),
        )));
    }

    if let Some(indexer) = indexer_service {
        // The indexer runtime processes every block commit and is expensive
        // (indexes all transaction execution results). During catch-up it
        // dominates sync time (measured: 25 blocks/min WITH vs 200+ WITHOUT).
        // Gate it: only spawn when the node starts near the live tip.
        let peer_tip = neo_runtime::sync_metrics::peer_live_tip();
        // Start the indexer if the node is resuming from a non-zero height
        // (already synced, just catching up a small gap). On a cold store
        // (durable_tip=0) the node will sync millions of blocks; defer the
        // indexer to prioritize sync throughput.
        let indexer_should_start = durable_tip_height > 0 && (peer_tip == 0 || durable_tip_height as u64 + 10000 >= peer_tip);
        if indexer_should_start {
            spawn_daemon_task(
                &mut handles,
                observability.as_ref(),
                "indexer_runtime",
                indexer_runtime::run_live_indexer(
                    blockchain.clone(),
                    indexer,
                    application_logs_service,
                    config.indexer.backfill_on_startup,
                ),
            );
            info!(target: "neo", "indexer runtime started (durable_tip={}, peer_tip={})", durable_tip_height, peer_tip);
        } else {
            info!(
                target: "neo",
                durable_tip = durable_tip_height,
                peer_tip,
                "indexer runtime DEFERRED during catch-up (will index once near tip); sync throughput prioritized"
            );
        }
    }

    Ok(RunningNode {
        node,
        network,
        handles,
    })
}

#[cfg(test)]
#[path = "tests/node.rs"]
mod tests;
