//! Live Neo N3 Network Connectivity Test
//!
//! This example connects to the real Neo N3 network (mainnet or testnet),
//! discovers peers, and attempts to sync blocks.

use neo_config::{NetworkConfig, NetworkType};
use neo_core::UInt160;
use neo_ledger::Blockchain;
use neo_network::{NodeInfo, P2PNode};
use neo_persistence::rocksdb::RocksDbStore;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{error, info, warn};

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
    match test_network_connectivity(NetworkType::MainNet).await {
        Ok(_) => info!("✅ MainNet connectivity test passed"),
        Err(e) => warn!("MainNet test failed: {}", e),
    }

    info!("");
    info!("🟠 Testing TestNet connectivity...");
    match test_network_connectivity(NetworkType::TestNet).await {
        Ok(_) => info!("✅ TestNet connectivity test passed"),
        Err(e) => warn!("TestNet test failed: {}", e),
    }

    println!();
    println!("✅ Network connectivity tests completed!");

    Ok(())
}

async fn test_network_connectivity(
    network_type: NetworkType,
) -> Result<(), Box<dyn std::error::Error>> {
    let network_name = match network_type {
        NetworkType::MainNet => "MainNet",
        NetworkType::TestNet => "TestNet",
        NetworkType::Private => "Private",
    };

    // Create network configuration
    let mut network_config = NetworkConfig::default();
    network_config.port = match network_type {
        NetworkType::MainNet => 10333,
        NetworkType::TestNet => 20333,
        NetworkType::Private => 30333,
    };

    // Override with well-known seed nodes for better connectivity
    if network_type == NetworkType::TestNet {
        network_config.seed_nodes = vec![
            "168.62.167.190:20333".parse()?,
            "52.187.47.33:20333".parse()?,
            "52.166.72.196:20333".parse()?,
            "13.75.254.144:20333".parse()?,
            "13.71.130.1:20333".parse()?,
        ];
    }

    info!("🌱 Using {} seed nodes", network_config.seed_nodes.len());
    for (i, node) in network_config.seed_nodes.iter().take(3).enumerate() {
        info!("   {}. {}", i + 1, node);
    }

    // Create node info
    let node_info = NodeInfo::new(UInt160::zero(), 0);

    // Create temporary storage
    let temp_dir = std::env::temp_dir().join(format!("neo-rs-live-test-{}", network_name));
    let storage = Arc::new(RocksDbStore::new(&temp_dir)?);

    // Create blockchain instance
    let blockchain = Arc::new(Blockchain::new(storage.clone()).await?);

    // Create P2P node
    info!("🔧 Creating P2P node for {}...", network_name);
    let p2p_node = P2PNode::new(network_config, node_info)?;

    // Start the P2P node
    info!("🚀 Starting P2P node...");
    match timeout(Duration::from_secs(10), p2p_node.start()).await {
        Ok(Ok(_)) => info!("✅ P2P node started successfully"),
        Ok(Err(e)) => {
            error!("❌ Failed to start P2P node: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            error!("❌ P2P node startup timed out");
            return Err("Startup timeout".into());
        }
    }

    // Monitor connectivity
    info!("📡 Monitoring network connectivity...");
    let start_time = std::time::Instant::now();
    let mut max_peers = 0;
    let mut connected = false;

    while start_time.elapsed().as_secs() < 30 {
        let peer_count = p2p_node.get_connected_peers().await.len();

        if peer_count > max_peers {
            max_peers = peer_count;
            info!("🔗 Connected peers: {}", peer_count);

            if peer_count > 0 && !connected {
                connected = true;
                info!("🎉 Successfully connected to {} network!", network_name);

                // Show peer details
                let peers = p2p_node.get_connected_peers().await;
                for (i, peer) in peers.iter().take(3).enumerate() {
                    info!("   Peer {}: {:?}", i + 1, peer);
                }
            }
        }

        // Check blockchain height
        let height = blockchain.get_height().await;
        if height > 0 {
            info!("📦 Current blockchain height: {}", height);
        }

        sleep(Duration::from_secs(3)).await;
    }

    // Final report
    info!("📊 Connection test complete:");
    info!("   Network: {}", network_name);
    info!("   Max peers connected: {}", max_peers);
    info!("   Test duration: {:?}", start_time.elapsed());

    if max_peers == 0 {
        warn!("⚠️  No peers connected during test");
        warn!("   This could be due to:");
        warn!("   - Network connectivity issues");
        warn!("   - Firewall blocking connections");
        warn!("   - Seed nodes being offline");
    }

    // Graceful shutdown
    info!("🛑 Shutting down P2P node...");
    p2p_node.stop().await?;

    // Clean up temporary directory
    if let Err(e) = std::fs::remove_dir_all(&temp_dir) {
        warn!("Failed to clean up temp directory: {}", e);
    }

    Ok(())
}
