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
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use neo_core::ShutdownCoordinator;
use neo_ledger::Blockchain;
use neo_network::{P2pNode, SyncManager, NetworkCommand};
use neo_config::NetworkType;

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
        .get_matches();

    let is_testnet = matches.get_flag("testnet");
    let is_mainnet = matches.get_flag("mainnet");
    let rpc_port: u16 = matches.get_one::<String>("rpc-port").unwrap().parse()?;
    let p2p_port: u16 = matches.get_one::<String>("p2p-port").unwrap().parse()?;

    info!("🚀 Starting Neo-Rust Node");
    info!("=========================");
    info!("Network: {}", if is_testnet { "TestNet" } else if is_mainnet { "MainNet" } else { "Private" });
    info!("RPC Port: {}", rpc_port);
    info!("P2P Port: {}", p2p_port);

    // Run the node
    if let Err(e) = run_node(is_testnet, is_mainnet, rpc_port, p2p_port).await {
        error!("❌ Node failed: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

async fn run_node(is_testnet: bool, is_mainnet: bool, _rpc_port: u16, p2p_port: u16) -> Result<()> {
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
            .context("Failed to initialize P2P node")?
    );

    // Initialize sync manager
    let sync_manager = Arc::new(SyncManager::new(blockchain.clone(), p2p_node.clone()));

    // Register shutdown components
    shutdown_coordinator.register_component(sync_manager.clone()).await;
    shutdown_coordinator.register_component(p2p_node.clone()).await;

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

    // Setup monitoring and status reporting
    let monitoring_handle = start_monitoring(
        blockchain.clone(),
        p2p_node.clone(),
        sync_manager.clone(),
    );

    info!("✅ Neo-Rust node started successfully!");
    info!("📊 Connecting to Neo N3 network...");
    if is_testnet {
        info!("🔗 Connecting to Neo N3 TestNet seed nodes...");
    } else if is_mainnet {
        info!("🔗 Connecting to Neo N3 MainNet seed nodes...");
    }

    // Wait for shutdown signal
    let mut term_signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
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

    // Initiate graceful shutdown
    if let Err(e) = shutdown_coordinator.initiate_shutdown("User requested shutdown".to_string()).await {
        error!("Error during shutdown: {}", e);
    }

    // Wait for handles to complete
    let _ = tokio::join!(p2p_handle, sync_handle);

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
            info!("│  ├─ Height: {} (+{} in last 30s)", block_height, blocks_gained);
            info!("│  └─ Sync Rate: {} blocks/min", blocks_per_minute);
            info!("├─ Network:");
            info!("│  ├─ Peers: {} (↑{} ↓{})", 
                  p2p_stats.peer_count, 
                  p2p_stats.outbound_connections, 
                  p2p_stats.inbound_connections);
            info!("│  ├─ Messages: {} sent, {} received", 
                  p2p_stats.messages_sent, 
                  p2p_stats.messages_received);
            info!("│  └─ Data: {:.1} MB sent, {:.1} MB received", 
                  p2p_stats.bytes_sent as f64 / 1_000_000.0,
                  p2p_stats.bytes_received as f64 / 1_000_000.0);
            info!("├─ Synchronization:");
            info!("│  ├─ State: {}", sync_stats.state);
            info!("│  ├─ Progress: {:.1}% ({}/{})", 
                  sync_stats.progress_percentage,
                  sync_stats.current_height,
                  sync_stats.best_known_height);
            info!("│  ├─ Speed: {:.1} blocks/sec", sync_stats.sync_speed);
            info!("│  ├─ Health: {:.1}% {}", 
                  sync_health.health_score,
                  if sync_health.is_healthy { "✅" } else { "⚠️" });
            info!("│  └─ Pending: {} requests", sync_stats.pending_requests);
            info!("└─ Uptime: {}s", p2p_stats.uptime_seconds);

            // Check for potential issues
            if p2p_stats.peer_count < 3 {
                warn!("⚠️  Low peer count: {} peers. Node may have connectivity issues.", p2p_stats.peer_count);
            }

            if sync_health.health_score < 50.0 {
                warn!("⚠️  Sync health low: {:.1}%. Check network connectivity.", sync_health.health_score);
            }

            if sync_stats.current_height > 0 && blocks_gained == 0 && sync_stats.state != neo_network::sync::SyncState::Synchronized {
                warn!("⚠️  No new blocks in 30 seconds. Sync may be stalled.");
            }

            last_height = block_height;
        }
    })
}