//! Live Neo N3 Network Connectivity Test
//!
//! This example connects to the real Neo N3 network (mainnet or testnet),
//! discovers peers, and attempts to sync blocks.

use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{info, warn, error};
use std::net::{SocketAddr, ToSocketAddrs};

use neo_network::{NetworkServerBuilder, NetworkServerConfig};
use neo_network::server::NetworkServerEvent;
use neo_core::UInt160;
use neo_ledger::{Blockchain, Storage};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    println!("🌐 Neo-RS Live Network Connectivity Test");
    println!("=========================================");
    println!();

    // Test mainnet connectivity
    info!("🔵 Testing MainNet connectivity...");
    match test_mainnet_connectivity().await {
        Ok(_) => info!("✅ MainNet connectivity test passed"),
        Err(e) => warn!("MainNet test failed: {}", e),
    }

    info!("");
    info!("🟠 Testing TestNet connectivity...");
    match test_testnet_connectivity().await {
        Ok(_) => info!("✅ TestNet connectivity test passed"),
        Err(e) => warn!("TestNet test failed: {}", e),
    }

    println!();
    println!("✅ Network connectivity tests completed!");

    Ok(())
}

async fn test_mainnet_connectivity() -> Result<(), Box<dyn std::error::Error>> {
    // Real Neo N3 MainNet seed nodes
    let seed_nodes = resolve_seed_nodes(&[
        "seed1.neo.org:10333",
        "seed2.neo.org:10333", 
        "seed3.neo.org:10333",
        "seed4.neo.org:10333",
        "seed5.neo.org:10333",
        "seed6.neo.org:10333",
        "seed7.neo.org:10333",
        "seed8.neo.org:10333",
        "seed9.neo.org:10333",
        "seed10.neo.org:10333",
    ]).await;

    if seed_nodes.is_empty() {
        return Err("No MainNet seed nodes could be resolved".into());
    }

    info!("🌱 Resolved {} MainNet seed nodes", seed_nodes.len());
    for (i, node) in seed_nodes.iter().take(5).enumerate() {
        info!("   {}. {}", i + 1, node);
    }

    let config = NetworkServerConfig {
        node_id: UInt160::zero(),
        magic: 0x334f454e, // Neo N3 MainNet magic
        seed_nodes: seed_nodes.clone(),
        enable_auto_sync: true,
        sync_check_interval: 15,
        p2p_config: neo_network::P2PConfig {
            listen_address: "0.0.0.0:10334".parse().unwrap(), // Use different port to avoid conflicts
            ..Default::default()
        },
        ..Default::default()
    };

    test_network_connection("MainNet", config, seed_nodes).await
}

async fn test_testnet_connectivity() -> Result<(), Box<dyn std::error::Error>> {
    // Real Neo N3 TestNet seed nodes
    let seed_nodes = resolve_seed_nodes(&[
        "seed1t5.neo.org:20333",
        "seed2t5.neo.org:20333",
        "seed3t5.neo.org:20333", 
        "seed4t5.neo.org:20333",
        "seed5t5.neo.org:20333",
    ]).await;

    if seed_nodes.is_empty() {
        return Err("No TestNet seed nodes could be resolved".into());
    }

    info!("🌱 Resolved {} TestNet seed nodes", seed_nodes.len());
    for (i, node) in seed_nodes.iter().enumerate() {
        info!("   {}. {}", i + 1, node);
    }

    let mut config = NetworkServerConfig::testnet();
    config.seed_nodes = seed_nodes.clone();
    config.enable_auto_sync = true;
    config.sync_check_interval = 15;
    config.p2p_config.listen_address = "0.0.0.0:20334".parse().unwrap();

    test_network_connection("TestNet", config, seed_nodes).await
}

async fn resolve_seed_nodes(hostnames: &[&str]) -> Vec<SocketAddr> {
    let mut resolved = Vec::new();
    
    for hostname in hostnames {
        match timeout(Duration::from_secs(5), resolve_hostname(hostname)).await {
            Ok(Ok(addr)) => {
                info!("✅ Resolved {}: {}", hostname, addr);
                resolved.push(addr);
            }
            Ok(Err(e)) => {
                warn!("❌ Failed to resolve {}: {}", hostname, e);
            }
            Err(_) => {
                warn!("⏰ Timeout resolving {}", hostname);
            }
        }
    }
    
    resolved
}

async fn resolve_hostname(hostname: &str) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let addrs: Vec<SocketAddr> = tokio::task::spawn_blocking({
        let hostname = hostname.to_string();
        move || -> Result<Vec<SocketAddr>, std::io::Error> {
            let addrs: Vec<SocketAddr> = hostname.to_socket_addrs()?.collect();
            Ok(addrs)
        }
    }).await??;
    
    addrs.into_iter().find(|addr| addr.is_ipv4())
        .ok_or_else(|| "No IPv4 address found".into())
}

async fn test_network_connection(
    network_name: &str, 
    config: NetworkServerConfig,
    seed_nodes: Vec<SocketAddr>
) -> Result<(), Box<dyn std::error::Error>> {
    info!("🚀 Starting {} network test", network_name);

    // Create blockchain and storage
    let storage = Arc::new(Storage::new_memory());
    let blockchain = Arc::new(Blockchain::new(storage));
    blockchain.initialize().await?;

    info!("⛓️  Blockchain initialized - Height: {}", blockchain.height().await);

    // Create network server
    let server = NetworkServerBuilder::new()
        .magic(config.magic)
        .seed_nodes(seed_nodes.clone())
        .build(blockchain.clone());

    // Start network server
    info!("🌐 Starting network server...");
    server.start().await?;

    info!("🔌 Network server started, attempting connections...");

    // Monitor network activity for 60 seconds
    let mut event_receiver = server.event_receiver();
    let start_time = std::time::Instant::now();
    let test_duration = Duration::from_secs(60);

    info!("⏱️  Monitoring network activity for {} seconds...", test_duration.as_secs());

    let mut peer_connected = false;
    let mut blocks_synced = false;

    while start_time.elapsed() < test_duration {
        match timeout(Duration::from_secs(5), event_receiver.recv()).await {
            Ok(Ok(event)) => {
                match event {
                    NetworkServerEvent::P2P(p2p_event) => {
                        match p2p_event {
                            neo_network::P2PEvent::PeerConnected { peer_id, address } => {
                                info!("🎉 Peer connected: {} ({})", address, peer_id);
                                peer_connected = true;
                            }
                            neo_network::P2PEvent::PeerDisconnected { peer_id, address, reason } => {
                                info!("👋 Peer disconnected: {} ({}): {}", address, peer_id, reason);
                            }
                            neo_network::P2PEvent::MessageReceived { peer_id, message } => {
                                info!("📨 Message from {}: {:?}", peer_id, message.header.command);
                            }
                            _ => {}
                        }
                    }
                    NetworkServerEvent::Sync(sync_event) => {
                        match sync_event {
                            neo_network::SyncEvent::SyncStarted { target_height } => {
                                info!("🔄 Block synchronization started - target height: {}", target_height);
                            }
                            neo_network::SyncEvent::BlocksProgress { current, target } => {
                                info!("📦 Block sync progress - Height: {}/{}", current, target);
                                blocks_synced = true;
                            }
                            neo_network::SyncEvent::SyncCompleted { final_height } => {
                                info!("✅ Synchronization completed at height {}", final_height);
                                break;
                            }
                            _ => {}
                        }
                    }
                    NetworkServerEvent::StatsUpdated(stats) => {
                        info!("📊 Stats - Peers: {}, Height: {}/{}", 
                            stats.peer_count, stats.current_height, stats.best_known_height);
                    }
                    _ => {}
                }
            }
            Ok(Err(_)) => break, // Channel closed
            Err(_) => {
                // Timeout - check current status
                let stats = server.stats().await;
                info!("📊 Status - Peers: {}, Height: {}/{}", 
                    stats.peer_count, stats.current_height, stats.best_known_height);
            }
        }

        // Brief pause
        sleep(Duration::from_millis(100)).await;
    }

    // Final status report
    let final_stats = server.stats().await;
    let final_height = blockchain.height().await;

    info!("📋 {} Test Results:", network_name);
    info!("   🔗 Peer connections: {} ({})", 
        final_stats.peer_count, 
        if peer_connected { "SUCCESS" } else { "FAILED" });
    info!("   📦 Block sync: Height {} ({})", 
        final_height, 
        if blocks_synced || final_height > 0 { "SUCCESS" } else { "FAILED" });
    info!("   📊 Data transfer: ↓{}KB ↑{}KB", 
        final_stats.bytes_received / 1024, 
        final_stats.bytes_sent / 1024);

    // Stop server
    server.stop().await;
    info!("🛑 Network server stopped");

    // Determine success
    if peer_connected || final_height > 0 {
        info!("✅ {} connectivity test PASSED", network_name);
        Ok(())
    } else {
        error!("❌ {} connectivity test FAILED", network_name);
        Err(format!("{} connectivity test failed", network_name).into())
    }
} 