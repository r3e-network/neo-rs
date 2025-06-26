//! Neo N3 Network Connection Test
//!
//! A simple example that demonstrates how to connect the Neo Rust node
//! to the real Neo N3 MainNet or TestNet and verify connectivity.
//!
//! Usage:
//!   cargo run --example network_connection_test

use neo_config::{NetworkConfig, NetworkType};
use neo_ledger::Blockchain;
use neo_network::{NodeInfo, P2PNode};
use neo_persistence::rocksdb::RocksDbStore;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Test both MainNet and TestNet connectivity
    for network_type in [NetworkType::MainNet, NetworkType::TestNet] {
        let network_name = match network_type {
            NetworkType::MainNet => "Neo N3 MainNet",
            NetworkType::TestNet => "Neo N3 TestNet",
            NetworkType::Private => "Private Network",
        };

        info!(
            "🚀 Testing Neo Rust Node Network Connectivity for {}",
            network_name
        );
        info!("───────────────────────────────────────────");

        // Create network configuration
        let mut network_config = NetworkConfig::default();
        network_config.port = match network_type {
            NetworkType::MainNet => 10333,
            NetworkType::TestNet => 20333,
            NetworkType::Private => 30333,
        };

        // Create node info
        let node_info = NodeInfo {
            user_agent: "Neo-Rust-Test/0.1.0".to_string(),
            protocol_version: 3,
            network: network_type,
            port: network_config.port,
        };

        // Create temporary storage
        let temp_dir = std::env::temp_dir().join(format!("neo-rs-test-{}", network_type));
        let storage = Arc::new(RocksDbStore::new(&temp_dir)?);

        // Create blockchain instance
        let blockchain = Arc::new(Blockchain::new(storage.clone(), network_type).await?);

        // Create P2P node
        info!("🔧 Creating P2P node for {}...", network_name);
        let p2p_node = P2PNode::new(network_config, node_info)?;

        // Start the P2P node
        info!("🚀 Starting P2P node and connecting to {}...", network_name);
        info!("⏳ This may take a moment to connect to peers...");

        match timeout(Duration::from_secs(30), p2p_node.start()).await {
            Ok(Ok(_)) => {
                info!("✅ P2P node started successfully!");
                info!(
                    "🔗 Node is now attempting to connect to {} peers",
                    network_name
                );
            }
            Ok(Err(e)) => {
                error!("❌ Failed to start P2P node: {}", e);
                continue; // Try next network
            }
            Err(_) => {
                error!("❌ Node startup timed out after 30 seconds");
                continue; // Try next network
            }
        }

        // Monitor connectivity for a short duration
        info!("📊 Monitoring network connectivity for 20 seconds...");
        let start_time = std::time::Instant::now();
        let mut connection_established = false;

        while start_time.elapsed().as_secs() < 20 {
            sleep(Duration::from_secs(2)).await;

            // Check connectivity status
            let connected_peers = p2p_node.connected_peer_count().await;

            if connected_peers > 0 && !connection_established {
                info!(
                    "🎉 Successfully connected to {} network with {} peers!",
                    network_name, connected_peers
                );
                connection_established = true;

                // Show some peer info
                let peers = p2p_node.get_connected_peers().await;
                for (i, peer) in peers.iter().take(3).enumerate() {
                    info!("   Peer {}: {}", i + 1, peer);
                }
                if peers.len() > 3 {
                    info!("   ... and {} more peers", peers.len() - 3);
                }
                break;
            } else if connected_peers == 0 {
                debug!("No peers connected yet...");
            }
        }

        // Final status report
        info!("───────────────────────────────────────────");
        if connection_established {
            info!(
                "🎯 SUCCESS: Neo Rust node successfully connected to {}!",
                network_name
            );
        } else {
            warn!("⚠️  No peer connections established for {}", network_name);
            warn!("   This could be due to network connectivity or seed node availability");
        }

        // Graceful shutdown
        info!("🛑 Shutting down P2P node...");
        if let Err(e) = p2p_node.stop().await {
            warn!("Warning during shutdown: {}", e);
        }
        info!("✅ Shutdown complete");

        // Clean up temporary directory
        if let Err(e) = std::fs::remove_dir_all(&temp_dir) {
            debug!("Failed to clean up temp directory: {}", e);
        }

        // Wait a moment before testing next network
        sleep(Duration::from_secs(2)).await;
    }

    // Overall test result
    info!("🏁 Neo N3 Network Connectivity Test Completed!");
    Ok(())
}
