//! Neo TestNet P2P Node with Real Network Connectivity
//!
//! This implementation demonstrates the Neo Rust node connecting to actual
//! TestNet infrastructure and synchronizing blocks from the network.

use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    println!("🚀 Neo TestNet P2P Node - Real Network Integration");
    println!("==================================================");
    
    // Initialize Neo P2P configuration
    let network_config = neo_network::NetworkConfig {
        network_type: neo_config::NetworkType::TestNet,
        listen_port: 20333,
        max_connections: 50,
        connect_timeout: Duration::from_secs(10),
        seed_nodes: vec![
            "seed1t.neo.org:20333".parse()?,
            "seed2t.neo.org:20333".parse()?,
            "seed3t.neo.org:20333".parse()?,
            "seed4t.neo.org:20333".parse()?,
            "seed5t.neo.org:20333".parse()?,
        ],
        ..Default::default()
    };

    println!("🌐 Network: TestNet (Magic: 0x{:08X})", 0x3554334E);
    println!("📡 P2P Port: {}", network_config.listen_port);
    println!("🎯 Seed Nodes: {} configured", network_config.seed_nodes.len());

    // Create network event channel
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(1000);

    // Initialize P2P node
    println!("🔧 Initializing P2P node...");
    let mut p2p_node = neo_network::P2pNode::new(network_config, event_tx).await?;

    // Start P2P networking
    println!("🚀 Starting P2P networking...");
    p2p_node.start().await?;

    println!("✅ P2P node started successfully!");
    println!("🔍 Attempting connections to seed nodes...");

    // Connect to seed nodes
    let mut connected_peers = 0;
    for seed in &p2p_node.config().seed_nodes {
        println!("🔌 Connecting to seed: {}", seed);
        match p2p_node.connect_to_peer(*seed).await {
            Ok(_) => {
                connected_peers += 1;
                println!("✅ Connected to seed: {}", seed);
            }
            Err(e) => {
                println!("❌ Failed to connect to {}: {}", seed, e);
            }
        }
        
        // Brief delay between connections
        sleep(Duration::from_millis(500)).await;
    }

    println!("📊 Connection Summary:");
    println!("   ✅ Connected peers: {}", connected_peers);
    println!("   📡 Listening on: 0.0.0.0:{}", p2p_node.config().listen_port);

    if connected_peers > 0 {
        println!("🎉 P2P connectivity established!");
        println!("🔄 Starting block synchronization...");

        // Monitor network events and sync blocks
        let mut sync_height = 0u32;
        let mut last_peer_count = 0;

        for iteration in 0..20 { // Run for 20 iterations (10 minutes)
            println!("\n📊 === Sync Iteration {} ===", iteration + 1);
            
            // Check network status
            let peer_count = p2p_node.get_peer_count().await;
            if peer_count != last_peer_count {
                println!("👥 Peer count: {} ({})", peer_count, 
                    if peer_count > last_peer_count { "↗" } else { "↘" });
                last_peer_count = peer_count;
            }

            // Process network events
            while let Ok(event) = tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await {
                if let Some(event) = event {
                    match event {
                        neo_network::NetworkEvent::PeerConnected { address } => {
                            println!("✅ New peer connected: {}", address);
                        }
                        neo_network::NetworkEvent::PeerDisconnected { address } => {
                            println!("❌ Peer disconnected: {}", address);
                        }
                        neo_network::NetworkEvent::BlockReceived { height, hash } => {
                            println!("📦 Block received - Height: {}, Hash: {}", height, hash);
                            sync_height = height;
                        }
                        neo_network::NetworkEvent::TransactionReceived { hash } => {
                            println!("💰 Transaction received: {}", hash);
                        }
                        _ => {
                            println!("📢 Network event: {:?}", event);
                        }
                    }
                }
            }

            // Request blocks if connected
            if peer_count > 0 {
                println!("📥 Requesting blocks from height {}...", sync_height);
                
                // Request next batch of blocks
                match p2p_node.request_blocks(sync_height, sync_height + 10).await {
                    Ok(block_count) => {
                        if block_count > 0 {
                            println!("📦 Requested {} blocks from network", block_count);
                        }
                    }
                    Err(e) => {
                        println!("❌ Block request failed: {}", e);
                    }
                }
            }

            // Wait before next iteration
            sleep(Duration::from_secs(30)).await;
        }

        println!("\n🏁 P2P test completed successfully!");
        println!("📊 Final Statistics:");
        println!("   📦 Highest synced block: {}", sync_height);
        println!("   👥 Connected peers: {}", p2p_node.get_peer_count().await);
        
    } else {
        println!("⚠️ No peer connections established");
        println!("   This may be due to:");
        println!("   • Network firewall blocking outbound connections");
        println!("   • TestNet seed nodes temporarily down");
        println!("   • Network configuration issues");
        println!("   • Port 20333 not accessible");
    }

    // Shutdown gracefully
    println!("🛑 Shutting down P2P node...");
    p2p_node.stop().await?;
    println!("✅ Node shutdown complete");

    Ok(())
}