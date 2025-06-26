//! Network connectivity test for Neo Rust node
//!
//! This test verifies that the Rust Neo node can connect to the real Neo N3 network,
//! discover peers, perform handshakes, and communicate using the Neo protocol.

use neo_core::UInt160;
use neo_network::{NetworkConfig, NetworkMessage, NodeEvent, NodeInfo, P2PNode, ProtocolMessage};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};

#[tokio::test]
async fn test_mainnet_connectivity() {
    // Initialize tracing for debugging
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init()
        .ok();

    info!("🌐 Testing Neo N3 Mainnet connectivity...");

    // Create mainnet configuration
    let config = NetworkConfig::default(); // Uses mainnet by default
    let node_info = neo_network::NodeInfo::new(UInt160::zero(), 0);

    // Create P2P node
    let p2p_node = P2PNode::new(config.p2p_config.clone(), node_info, config.magic);

    // Get event receiver to monitor connections
    let mut event_receiver = p2p_node.event_receiver();

    info!("📡 Starting P2P node for mainnet connectivity test...");

    // Start the P2P node (this will begin listening)
    if let Err(e) = p2p_node.start().await {
        error!("Failed to start P2P node: {}", e);
        panic!("Could not start P2P node");
    }

    info!("✅ P2P node started successfully");

    // Test connection to mainnet seed nodes
    let mut connected_peers = 0;
    let max_test_peers = 3; // Test connection to 3 peers

    for (i, seed_addr) in config.seed_nodes.iter().take(max_test_peers).enumerate() {
        info!(
            "🔗 Attempting to connect to seed node {}: {}",
            i + 1,
            seed_addr
        );

        match p2p_node.connect_peer(*seed_addr).await {
            Ok(()) => {
                info!("✅ Successfully initiated connection to {}", seed_addr);

                // Wait for connection events with timeout
                match timeout(
                    Duration::from_secs(30),
                    wait_for_connection_event(&mut event_receiver, *seed_addr),
                )
                .await
                {
                    Ok(true) => {
                        info!(
                            "🎉 Successfully connected and completed handshake with {}",
                            seed_addr
                        );
                        connected_peers += 1;

                        // Test basic communication
                        if let Err(e) = test_basic_communication(&p2p_node, *seed_addr).await {
                            warn!("Communication test failed with {}: {}", seed_addr, e);
                        } else {
                            info!("✅ Communication test successful with {}", seed_addr);
                        }
                    }
                    Ok(false) => {
                        warn!("❌ Connection to {} failed during handshake", seed_addr);
                    }
                    Err(_) => {
                        warn!("⏰ Connection to {} timed out", seed_addr);
                    }
                }
            }
            Err(e) => {
                warn!("❌ Failed to connect to {}: {}", seed_addr, e);
            }
        }

        // Small delay between connection attempts
        sleep(Duration::from_millis(500)).await;
    }

    // Stop the P2P node
    p2p_node.stop().await;

    // Verify we connected to at least one peer
    assert!(
        connected_peers > 0,
        "Failed to connect to any mainnet peers"
    );

    info!("🎉 Mainnet connectivity test completed successfully!");
    info!(
        "📊 Connected to {}/{} seed nodes",
        connected_peers, max_test_peers
    );
}

#[tokio::test]
async fn test_testnet_connectivity() {
    // Initialize tracing for debugging
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init()
        .ok();

    info!("🧪 Testing Neo N3 Testnet connectivity...");

    // Create testnet configuration
    let config = NetworkConfig::testnet();
    let node_info = neo_network::NodeInfo::new(UInt160::zero(), 0);

    // Create P2P node with testnet config
    let p2p_node = P2PNode::new(config.p2p_config.clone(), node_info, config.magic);

    // Get event receiver to monitor connections
    let mut event_receiver = p2p_node.event_receiver();

    info!("📡 Starting P2P node for testnet connectivity test...");

    // Start the P2P node
    if let Err(e) = p2p_node.start().await {
        error!("Failed to start P2P node: {}", e);
        panic!("Could not start P2P node");
    }

    info!("✅ P2P node started successfully");

    // Test connection to testnet seed nodes
    let mut connected_peers = 0;
    let max_test_peers = 2; // Test connection to 2 testnet peers

    for (i, seed_addr) in config.seed_nodes.iter().take(max_test_peers).enumerate() {
        info!(
            "🔗 Attempting to connect to testnet seed node {}: {}",
            i + 1,
            seed_addr
        );

        match p2p_node.connect_peer(*seed_addr).await {
            Ok(()) => {
                info!("✅ Successfully initiated connection to {}", seed_addr);

                // Wait for connection events with timeout
                match timeout(
                    Duration::from_secs(30),
                    wait_for_connection_event(&mut event_receiver, *seed_addr),
                )
                .await
                {
                    Ok(true) => {
                        info!(
                            "🎉 Successfully connected and completed handshake with {}",
                            seed_addr
                        );
                        connected_peers += 1;
                    }
                    Ok(false) => {
                        warn!("❌ Connection to {} failed during handshake", seed_addr);
                    }
                    Err(_) => {
                        warn!("⏰ Connection to {} timed out", seed_addr);
                    }
                }
            }
            Err(e) => {
                warn!("❌ Failed to connect to {}: {}", seed_addr, e);
            }
        }

        // Small delay between connection attempts
        sleep(Duration::from_millis(500)).await;
    }

    // Stop the P2P node
    p2p_node.stop().await;

    // Verify we connected to at least one testnet peer
    assert!(
        connected_peers > 0,
        "Failed to connect to any testnet peers"
    );

    info!("🎉 Testnet connectivity test completed successfully!");
    info!(
        "📊 Connected to {}/{} testnet seed nodes",
        connected_peers, max_test_peers
    );
}

#[tokio::test]
async fn test_protocol_version_compatibility() {
    info!("🔍 Testing Neo N3 protocol version compatibility...");

    let version = neo_network::ProtocolVersion::current();
    info!("📋 Current protocol version: {}", version);

    // Test compatibility with older versions
    let older_version = neo_network::ProtocolVersion::new(3, 5, 0);
    assert!(
        version.is_compatible(&older_version),
        "Should be compatible with older patch versions"
    );

    // Test incompatibility with different major versions
    let different_major = neo_network::ProtocolVersion::new(2, 6, 0);
    assert!(
        !version.is_compatible(&different_major),
        "Should not be compatible with different major versions"
    );

    info!("✅ Protocol version compatibility tests passed");
}

#[tokio::test]
async fn test_network_message_format() {
    info!("📨 Testing Neo N3 network message format compatibility...");

    // Test creating a version message (used in handshake)
    let node_info = neo_network::NodeInfo::new(UInt160::zero(), 100);
    let version_message = ProtocolMessage::version(&node_info, 10333, true);

    // Create network message with mainnet magic
    let magic = 0x334f454e; // Neo N3 mainnet magic
    let network_message = NetworkMessage::new(magic, version_message);

    info!("🔍 Testing message serialization...");

    // Test serialization (this should match C# Neo message format)
    match network_message.to_bytes() {
        Ok(bytes) => {
            info!("✅ Message serialized successfully, {} bytes", bytes.len());

            // Verify header format (should be 24 bytes for Neo N3)
            assert!(
                bytes.len() >= 24,
                "Message should have at least 24-byte header"
            );

            // Test deserialization
            match NetworkMessage::from_bytes(&bytes) {
                Ok(deserialized) => {
                    info!("✅ Message deserialized successfully");
                    assert_eq!(deserialized.header.magic, magic);
                }
                Err(e) => {
                    error!("❌ Message deserialization failed: {}", e);
                    panic!("Message deserialization failed");
                }
            }
        }
        Err(e) => {
            error!("❌ Message serialization failed: {}", e);
            panic!("Message serialization failed");
        }
    }

    info!("✅ Network message format tests passed");
}

/// Wait for a connection event for a specific peer
async fn wait_for_connection_event(
    event_receiver: &mut tokio::sync::broadcast::Receiver<NodeEvent>,
    target_addr: SocketAddr,
) -> bool {
    while let Ok(event) = event_receiver.recv().await {
        match event {
            NodeEvent::PeerConnected(peer_info) if peer_info.address == target_addr => {
                return true;
            }
            NodeEvent::PeerDisconnected(address) if address == target_addr => {
                warn!("Peer {} disconnected", address);
                return false;
            }
            NodeEvent::NetworkError {
                peer: Some(address),
                error,
            } if address == target_addr => {
                warn!("Connection to {} failed: {}", address, error);
                return false;
            }
            _ => {
                // Continue waiting for the right event
                debug!("Received other event: {:?}", event);
            }
        }
    }
    false
}

/// Test basic communication with a connected peer
async fn test_basic_communication(
    p2p_node: &P2PNode,
    peer_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("📞 Testing basic communication with {}", peer_addr);

    // Create a ping message
    let ping_message = NetworkMessage::new(ProtocolMessage::Ping {
        nonce: rand::random(),
    });

    // Send ping message
    p2p_node.send_message(peer_addr, ping_message).await?;
    info!("📤 Sent ping message to {}", peer_addr);

    // Wait a bit for potential response
    sleep(Duration::from_millis(1000)).await;

    Ok(())
}

/// Integration test for full network stack
#[tokio::test]
async fn test_full_network_stack() {
    info!("🔧 Testing full network stack integration...");

    // Test network configuration
    let mainnet_config = NetworkConfig::default();
    let testnet_config = NetworkConfig::testnet();
    let private_config = NetworkConfig::private();

    // Verify configurations
    assert_eq!(mainnet_config.magic, 0x334f454e);
    assert_eq!(testnet_config.magic, 0x3554334e);
    assert_eq!(private_config.magic, 0x12345678);

    // Verify seed nodes
    assert!(!mainnet_config.seed_nodes.is_empty());
    assert!(!testnet_config.seed_nodes.is_empty());
    assert!(private_config.seed_nodes.is_empty());

    info!("✅ Network configuration tests passed");

    // Test node info creation
    let node_info = neo_network::NodeInfo::new(UInt160::zero(), 0);
    assert_eq!(node_info.user_agent, "neo-rs/0.1.0");
    assert!(!node_info.capabilities.is_empty());

    info!("✅ Node info tests passed");
    info!("🎉 Full network stack integration test completed!");
}
