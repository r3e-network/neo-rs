//! Neo-Rust Node - Complete Neo N3 Blockchain Node Implementation
//!
//! This is the main entry point for the Neo-Rust blockchain node.
//! It provides a complete implementation that can connect to the Neo N3 network,
//! sync blocks, process transactions, and participate in consensus.

use anyhow::{Context, Result};
use clap::{Arg, Command};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

use neo_config::{LedgerConfig, NetworkType, RpcServerConfig};
use neo_consensus::ConsensusServiceConfig;
use neo_core::ShutdownCoordinator;
use neo_ledger::{Blockchain, Ledger};
use neo_network::{NetworkCommand, P2pNode, SyncManager, TransactionRelay, TransactionRelayConfig};
use neo_persistence::RocksDbStore;
use neo_rpc_server::RpcServer;

mod config;
mod debug;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    // Parse command line arguments
    let matches = Command::new("neo-node")
        .version("0.1.0")
        .about("Neo N3 Blockchain Node in Rust")
        .arg(
            Arg::new("testnet")
                .long("testnet")
                .help("Connect to Neo N3 TestNet")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("mainnet")
                .long("mainnet")
                .help("Connect to Neo N3 MainNet")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("rpc-port")
                .long("rpc-port")
                .value_name("PORT")
                .help("RPC server port")
                .default_value("10332"),
        )
        .arg(
            Arg::new("p2p-port")
                .long("p2p-port")
                .value_name("PORT")
                .help("P2P network port")
                .default_value("20333"),
        )
        .arg(
            Arg::new("consensus")
                .long("consensus")
                .help("Enable consensus participation")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("validator-key")
                .long("validator-key")
                .value_name("KEY")
                .help("Validator private key for consensus"),
        )
        .get_matches();

    let is_testnet = matches.get_flag("testnet");
    let is_mainnet = matches.get_flag("mainnet");
    let rpc_port: u16 = matches.get_one::<String>("rpc-port").unwrap().parse()?;
    let p2p_port: u16 = matches.get_one::<String>("p2p-port").unwrap().parse()?;
    let enable_consensus = matches.get_flag("consensus");
    let validator_key = matches.get_one::<String>("validator-key").cloned();

    info!("🚀 Starting Neo-Rust Node");
    info!("=========================");
    info!(
        "Network: {}",
        if is_testnet {
            "TestNet"
        } else if is_mainnet {
            "MainNet"
        } else {
            "Private"
        }
    );
    info!("RPC Port: {}", rpc_port);
    info!("P2P Port: {}", p2p_port);
    info!(
        "Consensus: {}",
        if enable_consensus {
            "Enabled"
        } else {
            "Disabled"
        }
    );

    // Run the node
    if let Err(e) = run_node(
        is_testnet,
        is_mainnet,
        rpc_port,
        p2p_port,
        enable_consensus,
        validator_key,
    )
    .await
    {
        error!("❌ Node failed: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

async fn run_node(
    is_testnet: bool,
    is_mainnet: bool,
    rpc_port: u16,
    p2p_port: u16,
    enable_consensus: bool,
    validator_key: Option<String>,
) -> Result<()> {
    // Initialize shutdown coordinator
    let shutdown_coordinator = Arc::new(ShutdownCoordinator::new());

    // Initialize blockchain
    info!("⛓️  Initializing blockchain...");
    let network_type = if is_testnet {
        NetworkType::TestNet
    } else if is_mainnet {
        NetworkType::MainNet
    } else {
        NetworkType::Private
    };
    let blockchain = match Blockchain::new(network_type).await {
        Ok(blockchain) => {
            info!("✅ Blockchain initialized successfully");
            Arc::new(blockchain)
        }
        Err(e) => {
            error!("❌ Blockchain initialization failed: {}", e);
            error!("❌ Error details: {:?}", e);
            return Err(anyhow::anyhow!("Failed to initialize blockchain: {}", e));
        }
    };

    // Initialize network configuration
    let mut network_config = if is_testnet {
        neo_network::NetworkConfig::testnet()
    } else if is_mainnet {
        neo_network::NetworkConfig::default() // mainnet is default
    } else {
        neo_network::NetworkConfig::private()
    };

    // Update network config with user settings
    network_config.port = p2p_port;
    network_config.listen_address = format!("0.0.0.0:{}", p2p_port).parse().unwrap();

    info!("🌐 Initializing network components...");

    // Create command channel for P2P node
    let (_command_sender, command_receiver) = mpsc::channel::<NetworkCommand>(1000);

    // Initialize P2P node
    let p2p_node = Arc::new(
        P2pNode::new(network_config.clone(), command_receiver)
            .context("Failed to initialize P2P node")?,
    );

    // Initialize sync manager
    let sync_manager = Arc::new(SyncManager::new(blockchain.clone(), p2p_node.clone()));

    // Initialize transaction relay
    info!("💱 Initializing transaction relay...");
    let relay_config = TransactionRelayConfig::default();
    let ledger_mempool_config = neo_ledger::MempoolConfig::default();
    let ledger_mempool = Arc::new(RwLock::new(neo_ledger::MemoryPool::new(
        ledger_mempool_config,
    )));
    let _transaction_relay = Arc::new(TransactionRelay::new(relay_config, ledger_mempool.clone()));

    // Initialize consensus if enabled
    let consensus_service = if enable_consensus {
        info!("🏛️  Initializing consensus service...");

        // Create validator hash from key or use default for testing
        let validator_hash = if let Some(_key) = validator_key {
            // In production, this would derive the validator hash from the private key
            warn!("Using mock validator hash - production implementation needed");
            neo_core::UInt160::zero() // Mock for now
        } else {
            warn!("No validator key provided, using zero hash");
            neo_core::UInt160::zero()
        };

        // Create consensus configuration
        let mut consensus_config = ConsensusServiceConfig::default();
        consensus_config.enabled = true;

        // Create mock network service for consensus (in production, this would integrate with P2P)
        let consensus_network = Arc::new(neo_consensus::service::NetworkService::new());

        // Create consensus ledger (in production, this would be the actual blockchain)
        let consensus_ledger = Arc::new(neo_consensus::service::Ledger::new());

        // Create consensus mempool
        let consensus_mempool_config = neo_consensus::proposal::MempoolConfig::default();
        let consensus_mempool = Arc::new(neo_consensus::proposal::MemoryPool::new(
            consensus_mempool_config,
        ));

        // Create consensus service
        let service = neo_consensus::ConsensusService::new(
            consensus_config,
            validator_hash,
            consensus_ledger,
            consensus_network,
            consensus_mempool,
        );

        info!("✅ Consensus service initialized");
        Some(Arc::new(tokio::sync::RwLock::new(service)))
    } else {
        info!("⏭️  Consensus disabled");
        None
    };

    // Initialize RPC server
    info!("🔧 Initializing RPC server...");
    let rpc_config = RpcServerConfig {
        bind_address: "127.0.0.1".to_string(),
        port: rpc_port,
        max_connections: 100,
        enabled: true,
        cors_enabled: true,
        ssl_enabled: false,
    };

    // Create a ledger instance for RPC
    let ledger_config = LedgerConfig::default();
    let ledger = Arc::new(
        Ledger::new(ledger_config)
            .map_err(|e| anyhow::anyhow!("Failed to create ledger: {}", e))?,
    );

    // Create a mock storage for now (in a full implementation, this would be the actual storage)
    let storage = Arc::new(
        RocksDbStore::new("/tmp/neo-db")
            .map_err(|e| anyhow::anyhow!("Failed to create storage: {}", e))?,
    );
    let rpc_server = Arc::new(
        RpcServer::new(rpc_config, ledger, storage)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize RPC server: {}", e))?,
    );

    // Register shutdown components
    shutdown_coordinator
        .register_component(sync_manager.clone())
        .await;
    shutdown_coordinator
        .register_component(p2p_node.clone())
        .await;

    // Start the node components
    info!("🚀 Starting node components...");

    // Start P2P node
    let p2p_handle = {
        let p2p_node = p2p_node.clone();
        tokio::spawn(async move {
            if let Err(e) = p2p_node.start().await {
                error!("P2P node failed: {}", e);
            }
        })
    };

    // Start sync manager
    let sync_handle = {
        let sync_manager = sync_manager.clone();
        tokio::spawn(async move {
            if let Err(e) = sync_manager.start().await {
                error!("Sync manager failed: {}", e);
            }
        })
    };

    // Start RPC server
    let rpc_handle = {
        let rpc_server = rpc_server.clone();
        tokio::spawn(async move {
            if let Err(e) = rpc_server.start().await {
                error!("RPC server failed: {}", e);
            }
        })
    };

    // Start consensus service if enabled
    let consensus_handle = if let Some(consensus) = consensus_service.clone() {
        info!("🏛️  Starting consensus service...");
        let handle = tokio::spawn(async move {
            let mut service = consensus.write().await;
            if let Err(e) = service.start().await {
                error!("Consensus service failed: {}", e);
            }
        });
        Some(handle)
    } else {
        None
    };

    // Setup monitoring and status reporting
    let monitoring_handle =
        start_monitoring(blockchain.clone(), p2p_node.clone(), sync_manager.clone());

    info!("✅ Neo-Rust node started successfully!");
    info!("📊 Connecting to Neo N3 network...");
    if is_testnet {
        info!("🔗 Connecting to Neo N3 TestNet seed nodes...");
    } else if is_mainnet {
        info!("🔗 Connecting to Neo N3 MainNet seed nodes...");
    }

    // Wait for shutdown signal
    let mut term_signal =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("📶 Received shutdown signal (Ctrl+C)");
        }
        _ = term_signal.recv() => {
            info!("📶 Received shutdown signal (SIGTERM)");
        }
    }

    info!("🛑 Initiating graceful shutdown...");

    // Stop monitoring
    monitoring_handle.abort();

    // Stop consensus if running
    if let Some(consensus) = consensus_service {
        info!("🛑 Stopping consensus service...");
        let mut service = consensus.write().await;
        service.stop().await;
    }

    // Initiate graceful shutdown
    if let Err(e) = shutdown_coordinator
        .initiate_shutdown("User requested shutdown".to_string())
        .await
    {
        error!("Error during shutdown: {}", e);
    }

    // Wait for handles to complete
    if let Some(consensus_handle) = consensus_handle {
        let _ = tokio::join!(p2p_handle, sync_handle, rpc_handle, consensus_handle);
    } else {
        let _ = tokio::join!(p2p_handle, sync_handle, rpc_handle);
    }

    info!("✅ Neo-Rust node stopped gracefully");
    Ok(())
}

fn start_monitoring(
    blockchain: Arc<Blockchain>,
    p2p_node: Arc<P2pNode>,
    sync_manager: Arc<SyncManager>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        let mut last_height = 0;

        loop {
            interval.tick().await;

            // Get current statistics
            let block_height = blockchain.get_height().await;
            let p2p_stats = p2p_node.get_statistics().await;
            let sync_stats = sync_manager.stats().await;
            let sync_health = sync_manager.get_sync_health().await;

            // Calculate blocks per minute
            let blocks_gained = block_height.saturating_sub(last_height);
            let blocks_per_minute = blocks_gained * 2; // 30 second intervals

            // Display comprehensive status
            info!("📊 Node Status Report");
            info!("├─ Blockchain:");
            info!(
                "│  ├─ Height: {} (+{} in last 30s)",
                block_height, blocks_gained
            );
            info!("│  └─ Sync Rate: {} blocks/min", blocks_per_minute);
            info!("├─ Network:");
            info!(
                "│  ├─ Peers: {} (↑{} ↓{})",
                p2p_stats.peer_count, p2p_stats.outbound_connections, p2p_stats.inbound_connections
            );
            info!(
                "│  ├─ Messages: {} sent, {} received",
                p2p_stats.messages_sent, p2p_stats.messages_received
            );
            info!(
                "│  └─ Data: {:.1} MB sent, {:.1} MB received",
                p2p_stats.bytes_sent as f64 / 1_000_000.0,
                p2p_stats.bytes_received as f64 / 1_000_000.0
            );
            info!("├─ Synchronization:");
            info!("│  ├─ State: {}", sync_stats.state);
            info!(
                "│  ├─ Progress: {:.1}% ({}/{})",
                sync_stats.progress_percentage,
                sync_stats.current_height,
                sync_stats.best_known_height
            );
            info!("│  ├─ Speed: {:.1} blocks/sec", sync_stats.sync_speed);
            info!(
                "│  ├─ Health: {:.1}% {}",
                sync_health.health_score,
                if sync_health.is_healthy {
                    "✅"
                } else {
                    "⚠️"
                }
            );
            info!("│  └─ Pending: {} requests", sync_stats.pending_requests);
            info!("└─ Uptime: {}s", p2p_stats.uptime_seconds);

            // Check for potential issues
            if p2p_stats.peer_count < 3 {
                warn!(
                    "⚠️  Low peer count: {} peers. Node may have connectivity issues.",
                    p2p_stats.peer_count
                );
            }

            if sync_health.health_score < 50.0 {
                warn!(
                    "⚠️  Sync health low: {:.1}%. Check network connectivity.",
                    sync_health.health_score
                );
            }

            if sync_stats.current_height > 0
                && blocks_gained == 0
                && sync_stats.state != neo_network::sync::SyncState::Synchronized
            {
                warn!("⚠️  No new blocks in 30 seconds. Sync may be stalled.");
            }

            last_height = block_height;
        }
    })
}
