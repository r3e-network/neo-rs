//! Comprehensive Network and P2P Integration Tests
//!
//! These tests verify network protocol implementation, P2P communication,
//! message handling, and peer management functionality.

use neo_core::{UInt160, UInt256};
use neo_network::{
    MessageType, NetworkConfig, NetworkMessage, NetworkServer, NetworkStats, P2PConfig, P2PNode,
    PeerManager, ProtocolVersion, RpcConfig,
    messages::{GetBlocksMessage, PingMessage, PongMessage, VerAckMessage, VersionMessage},
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;
use tokio_test;

/// Test network configuration and validation
#[tokio::test]
async fn test_network_configuration() {
    println!("🌐 Testing network configuration");

    // Test MainNet configuration
    let mainnet_config = NetworkConfig {
        magic: 0x334f454e, // Neo N3 MainNet magic
        listen_address: "0.0.0.0:10333".parse().unwrap(),
        seed_nodes: vec![
            "seed1.neo.org:10333".parse().unwrap(),
            "seed2.neo.org:10333".parse().unwrap(),
            "seed3.neo.org:10333".parse().unwrap(),
        ],
        p2p_config: P2PConfig {
            listen_address: "0.0.0.0:10333".parse().unwrap(),
            max_peers: 100,
            connection_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(5),
            ping_interval: Duration::from_secs(30),
            message_buffer_size: 1000,
            enable_compression: true,
        },
        rpc_config: Some(RpcConfig {
            http_address: "127.0.0.1:10332".parse().unwrap(),
            ws_address: Some("127.0.0.1:10334".parse().unwrap()),
            enable_cors: true,
            max_connections: 100,
            request_timeout: Duration::from_secs(30),
        }),
        user_agent: "neo-rs/1.0.0".to_string(),
        protocol_version: ProtocolVersion::new(3, 6, 0),
        enable_upnp: false,
        max_concurrent_connections: 200,
        connection_backlog: 100,
    };

    // Validate MainNet configuration
    assert_eq!(
        mainnet_config.magic, 0x334f454e,
        "MainNet magic should be correct"
    );
    assert!(
        !mainnet_config.seed_nodes.is_empty(),
        "Should have seed nodes"
    );
    assert!(
        mainnet_config.p2p_config.max_peers > 0,
        "Should allow peers"
    );
    assert!(
        mainnet_config.rpc_config.is_some(),
        "Should have RPC config"
    );

    // Test TestNet configuration
    let testnet_config = NetworkConfig {
        magic: 0x3554454e, // Neo N3 TestNet magic
        listen_address: "0.0.0.0:20333".parse().unwrap(),
        seed_nodes: vec![
            "seed1t.neo.org:20333".parse().unwrap(),
            "seed2t.neo.org:20333".parse().unwrap(),
        ],
        p2p_config: P2PConfig {
            listen_address: "0.0.0.0:20333".parse().unwrap(),
            max_peers: 50,
            connection_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(5),
            ping_interval: Duration::from_secs(30),
            message_buffer_size: 500,
            enable_compression: false,
        },
        rpc_config: Some(RpcConfig {
            http_address: "127.0.0.1:20332".parse().unwrap(),
            ws_address: Some("127.0.0.1:20334".parse().unwrap()),
            enable_cors: true,
            max_connections: 50,
            request_timeout: Duration::from_secs(30),
        }),
        user_agent: "neo-rs-testnet/1.0.0".to_string(),
        protocol_version: ProtocolVersion::new(3, 6, 0),
        enable_upnp: false,
        max_concurrent_connections: 100,
        connection_backlog: 50,
    };

    // Validate TestNet configuration
    assert_eq!(
        testnet_config.magic, 0x3554454e,
        "TestNet magic should be correct"
    );
    assert_ne!(
        mainnet_config.magic, testnet_config.magic,
        "MainNet and TestNet should have different magic"
    );
    assert_ne!(
        mainnet_config.listen_address.port(),
        testnet_config.listen_address.port(),
        "Should use different ports"
    );

    println!("✅ Network configuration test passed");
}

/// Test protocol version compatibility
#[tokio::test]
async fn test_protocol_version_compatibility() {
    println!("🔄 Testing protocol version compatibility");

    // Test version creation and comparison
    let version_3_6_0 = ProtocolVersion::new(3, 6, 0);
    let version_3_5_0 = ProtocolVersion::new(3, 5, 0);
    let version_3_6_1 = ProtocolVersion::new(3, 6, 1);
    let version_4_0_0 = ProtocolVersion::new(4, 0, 0);

    // Test compatibility rules
    assert!(
        version_3_6_0.is_compatible(&version_3_5_0),
        "Newer minor version should be compatible with older"
    );
    assert!(
        version_3_6_1.is_compatible(&version_3_6_0),
        "Newer patch version should be compatible"
    );
    assert!(
        !version_4_0_0.is_compatible(&version_3_6_0),
        "Different major versions should not be compatible"
    );

    // Test version string representation
    assert_eq!(
        version_3_6_0.to_string(),
        "3.6.0",
        "Version string should be formatted correctly"
    );

    // Test version parsing
    let parsed_version = ProtocolVersion::from_string("3.6.0");
    assert!(parsed_version.is_ok(), "Should parse valid version string");
    assert_eq!(
        parsed_version.unwrap(),
        version_3_6_0,
        "Parsed version should match original"
    );

    // Test invalid version parsing
    let invalid_version = ProtocolVersion::from_string("invalid");
    assert!(
        invalid_version.is_err(),
        "Should fail to parse invalid version"
    );

    println!("✅ Protocol version compatibility test passed");
}

/// Test network message serialization and deserialization
#[tokio::test]
async fn test_network_message_serialization() {
    println!("📦 Testing network message serialization");

    // Test Version message
    let version_msg = VersionMessage {
        version: 0x00030600, // 3.6.0
        services: 0x01,      // NODE_NETWORK
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        port: 10333,
        nonce: 0x1234567890ABCDEF,
        user_agent: "neo-rs/1.0.0".to_string(),
        start_height: 1000000,
        relay: true,
    };

    // Serialize version message
    let serialized = version_msg.serialize();
    assert!(
        serialized.is_ok(),
        "Version message should serialize successfully"
    );
    let serialized_data = serialized.unwrap();
    assert!(
        !serialized_data.is_empty(),
        "Serialized data should not be empty"
    );

    // Deserialize version message
    let deserialized = VersionMessage::deserialize(&serialized_data);
    assert!(
        deserialized.is_ok(),
        "Version message should deserialize successfully"
    );
    let deserialized_msg = deserialized.unwrap();

    // Verify data integrity
    assert_eq!(
        deserialized_msg.version, version_msg.version,
        "Version should match"
    );
    assert_eq!(
        deserialized_msg.user_agent, version_msg.user_agent,
        "User agent should match"
    );
    assert_eq!(
        deserialized_msg.start_height, version_msg.start_height,
        "Start height should match"
    );

    // Test VerAck message
    let verack_msg = VerAckMessage;
    let verack_serialized = verack_msg.serialize().unwrap();
    let verack_deserialized = VerAckMessage::deserialize(&verack_serialized).unwrap();

    // Test Ping/Pong messages
    let ping_msg = PingMessage {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        height: 1000000,
    };

    let ping_serialized = ping_msg.serialize().unwrap();
    let ping_deserialized = PingMessage::deserialize(&ping_serialized).unwrap();
    assert_eq!(
        ping_deserialized.height, ping_msg.height,
        "Ping height should match"
    );

    let pong_msg = PongMessage {
        timestamp: ping_msg.timestamp,
        height: ping_msg.height,
    };

    let pong_serialized = pong_msg.serialize().unwrap();
    let pong_deserialized = PongMessage::deserialize(&pong_serialized).unwrap();
    assert_eq!(
        pong_deserialized.timestamp, pong_msg.timestamp,
        "Pong timestamp should match"
    );

    println!("✅ Network message serialization test passed");
}

/// Test peer management functionality
#[tokio::test]
async fn test_peer_management() {
    println!("👥 Testing peer management");

    let config = P2PConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(), // Use port 0 for automatic assignment
        max_peers: 10,
        connection_timeout: Duration::from_secs(5),
        handshake_timeout: Duration::from_secs(3),
        ping_interval: Duration::from_secs(30),
        message_buffer_size: 100,
        enable_compression: false,
    };

    let mut peer_manager = PeerManager::new(config);

    // Test adding peers
    let peer1: SocketAddr = "192.168.1.1:10333".parse().unwrap();
    let peer2: SocketAddr = "192.168.1.2:10333".parse().unwrap();
    let peer3: SocketAddr = "192.168.1.3:10333".parse().unwrap();

    // Add known peers
    peer_manager.add_known_peer(peer1);
    peer_manager.add_known_peer(peer2);
    peer_manager.add_known_peer(peer3);

    // Verify peers were added
    let known_peers = peer_manager.get_known_peers();
    assert!(known_peers.contains(&peer1), "Should contain peer1");
    assert!(known_peers.contains(&peer2), "Should contain peer2");
    assert!(known_peers.contains(&peer3), "Should contain peer3");
    assert_eq!(known_peers.len(), 3, "Should have 3 known peers");

    // Test peer connection simulation
    let connection_result = peer_manager.attempt_connection(peer1).await;
    // Connection might fail in test environment, but should not panic
    println!("  Connection attempt result: {:?}", connection_result);

    // Test peer removal
    peer_manager.remove_peer(peer2);
    let known_peers_after_removal = peer_manager.get_known_peers();
    assert!(
        !known_peers_after_removal.contains(&peer2),
        "Should not contain removed peer"
    );
    assert_eq!(
        known_peers_after_removal.len(),
        2,
        "Should have 2 peers after removal"
    );

    // Test peer banning
    peer_manager.ban_peer(peer3, Duration::from_secs(300)); // Ban for 5 minutes
    assert!(peer_manager.is_peer_banned(&peer3), "Peer should be banned");

    // Test getting peer statistics
    let stats = peer_manager.get_peer_stats(&peer1);
    if let Some(peer_stats) = stats {
        println!("  Peer {} stats: {:?}", peer1, peer_stats);
    }

    println!("✅ Peer management test passed");
}

/// Test P2P node functionality
#[tokio::test]
async fn test_p2p_node_functionality() {
    println!("🔗 Testing P2P node functionality");

    let config = P2PConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(),
        max_peers: 5,
        connection_timeout: Duration::from_secs(5),
        handshake_timeout: Duration::from_secs(3),
        ping_interval: Duration::from_secs(10),
        message_buffer_size: 50,
        enable_compression: false,
    };

    // Create P2P node
    let p2p_node_result = P2PNode::new(config.clone());
    assert!(
        p2p_node_result.is_ok(),
        "P2P node should be created successfully"
    );

    let mut p2p_node = p2p_node_result.unwrap();

    // Test node initialization
    assert_eq!(p2p_node.config(), &config, "Node config should match");
    assert_eq!(p2p_node.peer_count(), 0, "Should start with 0 peers");

    // Test adding seed nodes
    let seed_nodes = vec![
        "127.0.0.1:20333".parse().unwrap(),
        "127.0.0.1:20334".parse().unwrap(),
    ];

    for seed in &seed_nodes {
        p2p_node.add_seed_node(*seed);
    }

    // Test getting node info
    let node_info = p2p_node.get_node_info();
    assert!(
        node_info.listen_address.port() > 0,
        "Should have valid listen address"
    );
    assert_eq!(
        node_info.protocol_version.major, 3,
        "Should use protocol version 3"
    );

    // Test connection attempts (might fail in test environment)
    let connection_attempts = p2p_node.connect_to_seeds().await;
    println!("  Seed connection attempts: {:?}", connection_attempts);

    // Test message broadcasting preparation
    let test_message = NetworkMessage::new(
        MessageType::Ping,
        vec![1, 2, 3, 4], // Sample payload
    );

    // In a real scenario, this would broadcast to connected peers
    let broadcast_result = p2p_node.prepare_broadcast(test_message);
    assert!(
        broadcast_result.is_ok(),
        "Should prepare broadcast successfully"
    );

    println!("✅ P2P node functionality test passed");
}

/// Test network message handling
#[tokio::test]
async fn test_network_message_handling() {
    println!("💬 Testing network message handling");

    let config = NetworkConfig::default();
    let network_server_result = NetworkServer::new(config);
    assert!(
        network_server_result.is_ok(),
        "Network server should be created successfully"
    );

    let mut network_server = network_server_result.unwrap();

    // Test handling Version message
    let version_msg = VersionMessage {
        version: 0x00030600,
        services: 0x01,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        port: 10333,
        nonce: 0x1234567890ABCDEF,
        user_agent: "test-node/1.0.0".to_string(),
        start_height: 500000,
        relay: true,
    };

    let version_payload = version_msg.serialize().unwrap();
    let version_network_msg = NetworkMessage::new(MessageType::Version, version_payload);

    // Handle version message
    let handle_result = network_server
        .handle_message("127.0.0.1:10333".parse().unwrap(), version_network_msg)
        .await;

    assert!(
        handle_result.is_ok(),
        "Should handle version message successfully"
    );

    // Test handling GetBlocks message
    let get_blocks_msg = GetBlocksMessage {
        hash_start: vec![UInt256::zero()],
        hash_stop: UInt256::zero(),
    };

    let get_blocks_payload = get_blocks_msg.serialize().unwrap();
    let get_blocks_network_msg = NetworkMessage::new(MessageType::GetBlocks, get_blocks_payload);

    let get_blocks_result = network_server
        .handle_message("127.0.0.1:10333".parse().unwrap(), get_blocks_network_msg)
        .await;

    assert!(
        get_blocks_result.is_ok(),
        "Should handle GetBlocks message successfully"
    );

    // Test invalid message handling
    let invalid_msg = NetworkMessage::new(MessageType::Version, vec![0xFF, 0xFF]); // Invalid payload

    let invalid_result = network_server
        .handle_message("127.0.0.1:10333".parse().unwrap(), invalid_msg)
        .await;

    // Invalid messages should be handled gracefully (not panic)
    println!("  Invalid message handling result: {:?}", invalid_result);

    println!("✅ Network message handling test passed");
}

/// Test network statistics and monitoring
#[tokio::test]
async fn test_network_statistics() {
    println!("📊 Testing network statistics");

    let mut network_stats = NetworkStats::new();

    // Test initial state
    assert_eq!(
        network_stats.total_connections(),
        0,
        "Should start with 0 connections"
    );
    assert_eq!(
        network_stats.active_connections(),
        0,
        "Should start with 0 active connections"
    );
    assert_eq!(
        network_stats.messages_sent(),
        0,
        "Should start with 0 messages sent"
    );
    assert_eq!(
        network_stats.messages_received(),
        0,
        "Should start with 0 messages received"
    );

    // Simulate network activity
    network_stats.record_connection_attempt();
    network_stats.record_connection_success();
    network_stats.record_connection_success();
    network_stats.record_connection_failure();

    assert_eq!(
        network_stats.total_connections(),
        3,
        "Should have 3 total connections"
    );
    assert_eq!(
        network_stats.active_connections(),
        2,
        "Should have 2 active connections"
    );
    assert_eq!(
        network_stats.failed_connections(),
        1,
        "Should have 1 failed connection"
    );

    // Simulate message activity
    for _ in 0..10 {
        network_stats.record_message_sent(MessageType::Ping);
        network_stats.record_message_received(MessageType::Pong);
    }

    for _ in 0..5 {
        network_stats.record_message_sent(MessageType::Version);
        network_stats.record_message_received(MessageType::VerAck);
    }

    assert_eq!(
        network_stats.messages_sent(),
        15,
        "Should have sent 15 messages"
    );
    assert_eq!(
        network_stats.messages_received(),
        15,
        "Should have received 15 messages"
    );

    // Test message type statistics
    let ping_stats = network_stats.get_message_stats(MessageType::Ping);
    assert_eq!(ping_stats.sent, 10, "Should have sent 10 ping messages");

    let pong_stats = network_stats.get_message_stats(MessageType::Pong);
    assert_eq!(
        pong_stats.received, 10,
        "Should have received 10 pong messages"
    );

    // Test bandwidth statistics
    network_stats.record_bytes_sent(1024);
    network_stats.record_bytes_received(2048);

    assert_eq!(
        network_stats.total_bytes_sent(),
        1024,
        "Should have sent 1024 bytes"
    );
    assert_eq!(
        network_stats.total_bytes_received(),
        2048,
        "Should have received 2048 bytes"
    );

    // Test uptime
    let uptime = network_stats.uptime();
    assert!(uptime.as_millis() > 0, "Uptime should be greater than 0");

    println!("✅ Network statistics test passed");
}

/// Test concurrent network operations
#[tokio::test]
async fn test_concurrent_network_operations() {
    println!("🔄 Testing concurrent network operations");

    let config = P2PConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(),
        max_peers: 20,
        connection_timeout: Duration::from_secs(5),
        handshake_timeout: Duration::from_secs(3),
        ping_interval: Duration::from_secs(30),
        message_buffer_size: 100,
        enable_compression: false,
    };

    // Create multiple P2P nodes concurrently
    let node_creation_tasks = (0..5)
        .map(|i| {
            let config_clone = config.clone();
            tokio::spawn(async move {
                let node_result = P2PNode::new(config_clone);
                (i, node_result.is_ok())
            })
        })
        .collect::<Vec<_>>();

    let node_results = futures::future::join_all(node_creation_tasks).await;

    // Verify all nodes were created successfully
    for (i, result) in node_results.iter().enumerate() {
        let (node_id, success) = result.as_ref().unwrap();
        assert_eq!(*node_id, i, "Node ID should match");
        assert!(*success, "Node {} should be created successfully", i);
    }

    // Test concurrent message creation and serialization
    let message_tasks = (0..100)
        .map(|i| {
            tokio::spawn(async move {
                let ping_msg = PingMessage {
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    height: i as u32,
                };

                let serialized = ping_msg.serialize();
                let success = serialized.is_ok();

                if success {
                    let deserialized = PingMessage::deserialize(&serialized.unwrap());
                    (i, success && deserialized.is_ok())
                } else {
                    (i, false)
                }
            })
        })
        .collect::<Vec<_>>();

    let message_results = futures::future::join_all(message_tasks).await;

    // Verify all message operations completed successfully
    for result in message_results.iter() {
        let (msg_id, success) = result.as_ref().unwrap();
        assert!(
            *success,
            "Message {} should be processed successfully",
            msg_id
        );
    }

    println!("✅ Concurrent network operations test passed");
}

/// Test network error handling and recovery
#[tokio::test]
async fn test_network_error_handling() {
    println!("⚠️ Testing network error handling and recovery");

    let config = P2PConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(),
        max_peers: 5,
        connection_timeout: Duration::from_millis(100), // Very short timeout
        handshake_timeout: Duration::from_millis(50),   // Very short timeout
        ping_interval: Duration::from_secs(30),
        message_buffer_size: 10,
        enable_compression: false,
    };

    let mut peer_manager = PeerManager::new(config);

    // Test connection to non-existent peer
    let invalid_peer: SocketAddr = "192.168.255.255:99999".parse().unwrap();
    let connection_result = peer_manager.attempt_connection(invalid_peer).await;

    // Connection should fail gracefully
    assert!(
        connection_result.is_err(),
        "Connection to invalid peer should fail"
    );

    // Test handling invalid message data
    let invalid_data = vec![0xFF; 1000]; // Large invalid data
    let deserialize_result = VersionMessage::deserialize(&invalid_data);
    assert!(
        deserialize_result.is_err(),
        "Invalid message data should fail to deserialize"
    );

    // Test peer manager with too many peers
    for i in 0..20 {
        let peer: SocketAddr = format!("192.168.1.{}:10333", i + 1).parse().unwrap();
        peer_manager.add_known_peer(peer);
    }

    // Should handle gracefully even if we exceed max_peers in known peers
    let known_peers = peer_manager.get_known_peers();
    println!("  Known peers count after adding 20: {}", known_peers.len());

    // Test network recovery after errors
    let mut network_stats = NetworkStats::new();

    // Simulate multiple failures
    for _ in 0..10 {
        network_stats.record_connection_failure();
    }

    // Then simulate recovery
    for _ in 0..5 {
        network_stats.record_connection_success();
    }

    assert_eq!(
        network_stats.failed_connections(),
        10,
        "Should record all failures"
    );
    assert_eq!(
        network_stats.active_connections(),
        5,
        "Should record recovery connections"
    );

    println!("✅ Network error handling test passed");
}

/// Test network protocol handshake simulation
#[tokio::test]
async fn test_network_protocol_handshake() {
    println!("🤝 Testing network protocol handshake simulation");

    // Simulate handshake between two nodes
    let node1_version = VersionMessage {
        version: 0x00030600,
        services: 0x01,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        port: 10333,
        nonce: 0x1111111111111111,
        user_agent: "neo-rs-node1/1.0.0".to_string(),
        start_height: 1000000,
        relay: true,
    };

    let node2_version = VersionMessage {
        version: 0x00030600,
        services: 0x01,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        port: 10333,
        nonce: 0x2222222222222222,
        user_agent: "neo-rs-node2/1.0.0".to_string(),
        start_height: 1000001,
        relay: true,
    };

    // Simulate handshake steps

    // Step 1: Node1 sends Version to Node2
    let node1_version_data = node1_version.serialize().unwrap();
    let node1_version_msg = NetworkMessage::new(MessageType::Version, node1_version_data);

    // Step 2: Node2 receives Version and sends VerAck + Version
    let received_version = VersionMessage::deserialize(&node1_version_msg.payload()).unwrap();
    assert_eq!(
        received_version.nonce, node1_version.nonce,
        "Version nonce should match"
    );

    let node2_verack = VerAckMessage;
    let node2_verack_data = node2_verack.serialize().unwrap();
    let node2_verack_msg = NetworkMessage::new(MessageType::VerAck, node2_verack_data);

    let node2_version_data = node2_version.serialize().unwrap();
    let node2_version_msg = NetworkMessage::new(MessageType::Version, node2_version_data);

    // Step 3: Node1 receives VerAck and Version, sends VerAck
    let received_verack = VerAckMessage::deserialize(&node2_verack_msg.payload()).unwrap();
    let received_node2_version = VersionMessage::deserialize(&node2_version_msg.payload()).unwrap();

    assert_eq!(
        received_node2_version.nonce, node2_version.nonce,
        "Node2 version nonce should match"
    );

    let node1_verack = VerAckMessage;
    let node1_verack_data = node1_verack.serialize().unwrap();
    let node1_verack_msg = NetworkMessage::new(MessageType::VerAck, node1_verack_data);

    // Step 4: Node2 receives final VerAck
    let final_verack = VerAckMessage::deserialize(&node1_verack_msg.payload()).unwrap();

    // Handshake complete - verify both nodes have compatible versions
    assert_eq!(
        received_version.version, received_node2_version.version,
        "Versions should be compatible"
    );
    assert_ne!(
        received_version.nonce, received_node2_version.nonce,
        "Nonces should be different"
    );

    // Test ping-pong after handshake
    let ping = PingMessage {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        height: 1000000,
    };

    let ping_data = ping.serialize().unwrap();
    let ping_msg = NetworkMessage::new(MessageType::Ping, ping_data);

    let received_ping = PingMessage::deserialize(&ping_msg.payload()).unwrap();

    let pong = PongMessage {
        timestamp: received_ping.timestamp,
        height: received_ping.height,
    };

    let pong_data = pong.serialize().unwrap();
    let pong_msg = NetworkMessage::new(MessageType::Pong, pong_data);

    let received_pong = PongMessage::deserialize(&pong_msg.payload()).unwrap();
    assert_eq!(
        received_pong.timestamp, ping.timestamp,
        "Pong timestamp should match ping"
    );

    println!("✅ Network protocol handshake test passed");
}

/// Test network performance under load
#[tokio::test]
async fn test_network_performance_under_load() {
    println!("⚡ Testing network performance under load");

    let start_time = std::time::Instant::now();

    // Test 1: Rapid message creation and serialization
    let message_count = 1000;
    let mut serialization_times = Vec::new();

    for i in 0..message_count {
        let msg_start = std::time::Instant::now();

        let ping_msg = PingMessage {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            height: i,
        };

        let serialized = ping_msg.serialize().unwrap();
        let _deserialized = PingMessage::deserialize(&serialized).unwrap();

        serialization_times.push(msg_start.elapsed());
    }

    let avg_serialization_time =
        serialization_times.iter().sum::<Duration>() / message_count as u32;
    println!(
        "  Average message serialization time: {:?}",
        avg_serialization_time
    );

    // Test 2: Network stats performance
    let mut network_stats = NetworkStats::new();

    let stats_start = std::time::Instant::now();
    for _ in 0..10000 {
        network_stats.record_message_sent(MessageType::Ping);
        network_stats.record_message_received(MessageType::Pong);
        network_stats.record_bytes_sent(64);
        network_stats.record_bytes_received(64);
    }
    let stats_time = stats_start.elapsed();

    println!("  Time to record 20,000 stat events: {:?}", stats_time);

    // Test 3: Peer management performance
    let config = P2PConfig::default();
    let mut peer_manager = PeerManager::new(config);

    let peer_start = std::time::Instant::now();
    for i in 0..1000 {
        let peer: SocketAddr = format!("192.168.{}.{}:10333", i / 255, i % 255)
            .parse()
            .unwrap();
        peer_manager.add_known_peer(peer);
    }
    let peer_time = peer_start.elapsed();

    println!("  Time to add 1,000 peers: {:?}", peer_time);

    let total_time = start_time.elapsed();
    println!("  Total test time: {:?}", total_time);

    // Performance assertions
    assert!(
        avg_serialization_time.as_micros() < 1000,
        "Message serialization should be fast"
    );
    assert!(
        stats_time.as_millis() < 100,
        "Stats recording should be fast"
    );
    assert!(peer_time.as_millis() < 50, "Peer management should be fast");

    println!("✅ Network performance test passed");
}
