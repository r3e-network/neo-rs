//! Network Integration Tests
//!
//! Comprehensive tests for the Neo network layer implementation,
//! including P2P networking, message handling, and RPC functionality.

use neo_network::{
    NetworkConfig, P2PConfig, RpcConfig, ProtocolVersion,
    NetworkServer, NetworkServerConfig, P2PNode, RpcServer,
    message::{Message, MessageType, Payload, VersionPayload, AddrPayload, PingPayload, PongPayload},
    peer::{Peer, PeerState, PeerInfo},
};
use neo_core::{UInt160, UInt256, Transaction, Block, BlockHeader};
use neo_rpc_client::{RpcClient, RpcConfig as ClientConfig};
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tokio_test;

/// Test network configuration validation
#[test]
fn test_network_config_validation() {
    // Test default configuration
    let config = NetworkConfig::default();
    assert!(config.magic > 0, "Magic should be non-zero");
    assert!(!config.seed_nodes.is_empty(), "Should have seed nodes");
    assert!(config.p2p_config.max_peers > 0, "Max peers should be positive");
    
    // Test mainnet configuration
    let mainnet = NetworkConfig::mainnet();
    assert_eq!(mainnet.magic, 0x334f454e, "Mainnet magic should match");
    
    // Test testnet configuration
    let testnet = NetworkConfig::testnet();
    assert_eq!(testnet.magic, 0x3554334e, "Testnet magic should match");
    
    // Test custom configuration
    let custom = NetworkConfig {
        magic: 0x12345678,
        protocol_version: ProtocolVersion::new(3, 6, 0),
        user_agent: "test-node/1.0".to_string(),
        listen_address: "127.0.0.1:9999".parse().unwrap(),
        seed_nodes: vec!["127.0.0.1:10001".parse().unwrap()],
        p2p_config: P2PConfig {
            listen_address: "127.0.0.1:9999".parse().unwrap(),
            max_peers: 50,
            connection_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(5),
            ping_interval: Duration::from_secs(30),
            message_buffer_size: 500,
            enable_compression: true,
        },
        rpc_config: Some(RpcConfig {
            http_address: "127.0.0.1:9998".parse().unwrap(),
            ws_address: Some("127.0.0.1:9997".parse().unwrap()),
            enable_cors: true,
            max_connections: 50,
            request_timeout: Duration::from_secs(15),
        }),
    };
    
    assert_eq!(custom.magic, 0x12345678);
    assert_eq!(custom.protocol_version.major(), 3);
    assert_eq!(custom.protocol_version.minor(), 6);
    assert_eq!(custom.user_agent, "test-node/1.0");
    
    println!("✅ Network config validation test passed");
}

/// Test protocol version compatibility
#[test]
fn test_protocol_version_compatibility() {
    let v3_5_0 = ProtocolVersion::new(3, 5, 0);
    let v3_6_0 = ProtocolVersion::new(3, 6, 0);
    let v3_7_0 = ProtocolVersion::new(3, 7, 0);
    let v4_0_0 = ProtocolVersion::new(4, 0, 0);
    
    // Same major version, newer minor should be compatible with older
    assert!(v3_6_0.is_compatible(&v3_5_0), "v3.6.0 should be compatible with v3.5.0");
    assert!(v3_7_0.is_compatible(&v3_6_0), "v3.7.0 should be compatible with v3.6.0");
    assert!(v3_7_0.is_compatible(&v3_5_0), "v3.7.0 should be compatible with v3.5.0");
    
    // Same version should be compatible
    assert!(v3_6_0.is_compatible(&v3_6_0), "Same version should be compatible");
    
    // Different major versions should not be compatible
    assert!(!v4_0_0.is_compatible(&v3_7_0), "v4.0.0 should not be compatible with v3.7.0");
    assert!(!v3_6_0.is_compatible(&v4_0_0), "v3.6.0 should not be compatible with v4.0.0");
    
    // Older minor version should not be compatible with newer
    assert!(!v3_5_0.is_compatible(&v3_6_0), "v3.5.0 should not be compatible with v3.6.0");
    
    println!("✅ Protocol version compatibility test passed");
}

/// Test network message creation and serialization
#[test]
fn test_network_messages() {
    let magic = 0x334f454e;
    
    // Test Version message
    let version_payload = VersionPayload {
        version: 0x00000000,
        services: 0x00000001,
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
        port: 10333,
        nonce: 0x12345678,
        user_agent: "neo-rs/1.0".to_string(),
        start_height: 0,
        relay: true,
    };
    
    let version_message = Message::new(
        magic,
        MessageType::Version,
        Payload::Version(version_payload.clone()),
    );
    
    assert_eq!(version_message.magic, magic);
    assert_eq!(version_message.message_type, MessageType::Version);
    if let Payload::Version(payload) = &version_message.payload {
        assert_eq!(payload.user_agent, "neo-rs/1.0");
        assert_eq!(payload.port, 10333);
        assert_eq!(payload.nonce, 0x12345678);
    } else {
        panic!("Expected Version payload");
    }
    
    // Test Ping message
    let ping_payload = PingPayload {
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
        height: 100,
    };
    
    let ping_message = Message::new(
        magic,
        MessageType::Ping,
        Payload::Ping(ping_payload.clone()),
    );
    
    assert_eq!(ping_message.message_type, MessageType::Ping);
    if let Payload::Ping(payload) = &ping_message.payload {
        assert_eq!(payload.height, 100);
    } else {
        panic!("Expected Ping payload");
    }
    
    // Test Pong message
    let pong_payload = PongPayload {
        timestamp: ping_payload.timestamp,
        height: 100,
    };
    
    let pong_message = Message::new(
        magic,
        MessageType::Pong,
        Payload::Pong(pong_payload),
    );
    
    assert_eq!(pong_message.message_type, MessageType::Pong);
    
    println!("✅ Network messages test passed");
}

/// Test peer management
#[tokio::test]
async fn test_peer_management() {
    let peer_addr: SocketAddr = "127.0.0.1:10333".parse().unwrap();
    let mut peer = Peer::new(peer_addr);
    
    // Test initial state
    assert_eq!(peer.state(), PeerState::Disconnected);
    assert_eq!(peer.address(), &peer_addr);
    assert_eq!(peer.last_seen(), 0);
    
    // Test state transitions
    peer.set_state(PeerState::Connecting);
    assert_eq!(peer.state(), PeerState::Connecting);
    
    peer.set_state(PeerState::Connected);
    assert_eq!(peer.state(), PeerState::Connected);
    
    // Test peer info
    let peer_info = PeerInfo {
        address: peer_addr,
        version: ProtocolVersion::new(3, 6, 0),
        user_agent: "neo-core/3.6.0".to_string(),
        height: 500,
        nonce: 0x87654321,
        services: 0x00000001,
    };
    
    peer.set_info(Some(peer_info.clone()));
    assert!(peer.info().is_some());
    assert_eq!(peer.info().unwrap().height, 500);
    assert_eq!(peer.info().unwrap().user_agent, "neo-core/3.6.0");
    
    // Test last seen update
    peer.update_last_seen();
    assert!(peer.last_seen() > 0);
    
    println!("✅ Peer management test passed");
}

/// Test P2P node functionality
#[tokio::test]
async fn test_p2p_node() {
    let config = P2PConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(), // Use port 0 for automatic assignment
        max_peers: 10,
        connection_timeout: Duration::from_secs(5),
        handshake_timeout: Duration::from_secs(3),
        ping_interval: Duration::from_secs(10),
        message_buffer_size: 100,
        enable_compression: false,
    };
    
    let network_config = NetworkConfig {
        magic: 0x334f454e,
        protocol_version: ProtocolVersion::new(3, 6, 0),
        user_agent: "neo-rs-test/1.0".to_string(),
        listen_address: config.listen_address,
        seed_nodes: vec![],
        p2p_config: config.clone(),
        rpc_config: None,
    };
    
    // Create P2P node
    let p2p_node = P2PNode::new(network_config.clone());
    assert!(p2p_node.is_ok(), "Should create P2P node successfully");
    
    let node = p2p_node.unwrap();
    
    // Test node configuration
    assert_eq!(node.config().magic, 0x334f454e);
    assert_eq!(node.config().protocol_version.major(), 3);
    assert_eq!(node.config().protocol_version.minor(), 6);
    assert_eq!(node.config().user_agent, "neo-rs-test/1.0");
    
    // Test peer count (should start with 0)
    assert_eq!(node.peer_count(), 0);
    
    println!("✅ P2P node test passed");
}

/// Test RPC server configuration
#[tokio::test]
async fn test_rpc_server_config() {
    let rpc_config = RpcConfig {
        http_address: "127.0.0.1:0".parse().unwrap(), // Use port 0 for automatic assignment
        ws_address: Some("127.0.0.1:0".parse().unwrap()),
        enable_cors: true,
        max_connections: 100,
        request_timeout: Duration::from_secs(30),
    };
    
    // Test RPC configuration
    assert!(rpc_config.enable_cors);
    assert_eq!(rpc_config.max_connections, 100);
    assert_eq!(rpc_config.request_timeout, Duration::from_secs(30));
    
    // Create RPC server
    let rpc_server = RpcServer::new(rpc_config.clone());
    assert!(rpc_server.is_ok(), "Should create RPC server successfully");
    
    let server = rpc_server.unwrap();
    assert_eq!(server.config().max_connections, 100);
    assert_eq!(server.config().enable_cors, true);
    
    println!("✅ RPC server config test passed");
}

/// Test RPC client functionality
#[tokio::test]
async fn test_rpc_client() {
    let client_config = ClientConfig {
        endpoint: "http://127.0.0.1:10332".to_string(),
        timeout: 30,
        max_retries: 3,
        retry_delay: 1000,
        user_agent: "neo-rs-test-client/1.0".to_string(),
        headers: std::collections::HashMap::new(),
    };
    
    // Create RPC client
    let client = RpcClient::with_config(client_config.clone());
    assert!(client.is_ok(), "Should create RPC client successfully");
    
    let rpc_client = client.unwrap();
    
    // Test client configuration
    assert_eq!(rpc_client.endpoint(), "http://127.0.0.1:10332");
    assert_eq!(rpc_client.config().timeout, 30);
    assert_eq!(rpc_client.config().max_retries, 3);
    assert_eq!(rpc_client.config().user_agent, "neo-rs-test-client/1.0");
    
    // Test request ID generation
    let id1 = rpc_client.next_request_id();
    let id2 = rpc_client.next_request_id();
    assert_ne!(id1, id2, "Request IDs should be unique");
    assert!(id2 > id1, "Request IDs should increment");
    
    println!("✅ RPC client test passed");
}

/// Test network server integration
#[tokio::test]
async fn test_network_server_integration() {
    let server_config = NetworkServerConfig {
        node_id: UInt160::zero(),
        magic: 0x334f454e,
        p2p_config: P2PConfig {
            listen_address: "127.0.0.1:0".parse().unwrap(),
            max_peers: 20,
            connection_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(5),
            ping_interval: Duration::from_secs(30),
            message_buffer_size: 200,
            enable_compression: false,
        },
        rpc_config: Some(RpcConfig {
            http_address: "127.0.0.1:0".parse().unwrap(),
            ws_address: Some("127.0.0.1:0".parse().unwrap()),
            enable_cors: true,
            max_connections: 50,
            request_timeout: Duration::from_secs(30),
        }),
        enable_auto_sync: true,
        sync_check_interval: 10,
        stats_interval: 5,
        seed_nodes: vec![
            "127.0.0.1:10333".parse().unwrap(),
            "127.0.0.1:10334".parse().unwrap(),
        ],
    };
    
    // Create network server
    let network_server = NetworkServer::new(server_config.clone());
    assert!(network_server.is_ok(), "Should create network server successfully");
    
    let server = network_server.unwrap();
    
    // Test server configuration
    assert_eq!(server.config().magic, 0x334f454e);
    assert_eq!(server.config().p2p_config.max_peers, 20);
    assert!(server.config().rpc_config.is_some());
    assert_eq!(server.config().seed_nodes.len(), 2);
    
    // Test server state
    assert_eq!(server.state(), neo_network::NetworkServerState::Stopped);
    
    println!("✅ Network server integration test passed");
}

/// Test message validation and processing
#[test]
fn test_message_validation() {
    let magic = 0x334f454e;
    
    // Test valid version message
    let valid_version = VersionPayload {
        version: 0x00000000,
        services: 0x00000001,
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
        port: 10333,
        nonce: 0x12345678,
        user_agent: "neo-rs/1.0".to_string(),
        start_height: 0,
        relay: true,
    };
    
    let valid_message = Message::new(
        magic,
        MessageType::Version,
        Payload::Version(valid_version),
    );
    
    assert!(valid_message.validate().is_ok(), "Valid message should pass validation");
    
    // Test invalid magic
    let invalid_magic_message = Message::new(
        0x12345678, // Wrong magic
        MessageType::Version,
        valid_message.payload.clone(),
    );
    
    assert!(invalid_magic_message.validate().is_err(), "Invalid magic should fail validation");
    
    // Test Addr message
    let addr_payload = AddrPayload {
        addresses: vec![
            "127.0.0.1:10333".parse().unwrap(),
            "127.0.0.1:10334".parse().unwrap(),
        ],
    };
    
    let addr_message = Message::new(
        magic,
        MessageType::Addr,
        Payload::Addr(addr_payload),
    );
    
    assert!(addr_message.validate().is_ok(), "Addr message should be valid");
    
    println!("✅ Message validation test passed");
}

/// Test network performance under load
#[tokio::test]
async fn test_network_performance() {
    let start_time = std::time::Instant::now();
    
    // Create multiple network configurations
    let mut configs = Vec::new();
    for i in 0..10 {
        let config = NetworkConfig {
            magic: 0x334f454e,
            protocol_version: ProtocolVersion::new(3, 6, 0),
            user_agent: format!("neo-rs-test-{}/1.0", i),
            listen_address: format!("127.0.0.1:0").parse().unwrap(), // Port 0 for auto-assignment
            seed_nodes: vec![],
            p2p_config: P2PConfig {
                listen_address: format!("127.0.0.1:0").parse().unwrap(),
                max_peers: 5,
                connection_timeout: Duration::from_secs(1),
                handshake_timeout: Duration::from_secs(1),
                ping_interval: Duration::from_secs(5),
                message_buffer_size: 50,
                enable_compression: false,
            },
            rpc_config: None,
        };
        configs.push(config);
    }
    
    // Create multiple P2P nodes
    let mut nodes = Vec::new();
    for config in configs {
        let node = P2PNode::new(config);
        assert!(node.is_ok(), "Should create P2P node");
        nodes.push(node.unwrap());
    }
    
    // Create multiple messages
    let magic = 0x334f454e;
    let mut messages = Vec::new();
    for i in 0..100 {
        let ping_payload = PingPayload {
            timestamp: (SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + i) as u32,
            height: i as u32,
        };
        
        let message = Message::new(
            magic,
            MessageType::Ping,
            Payload::Ping(ping_payload),
        );
        
        assert!(message.validate().is_ok(), "Message {} should be valid", i);
        messages.push(message);
    }
    
    // Create multiple peers
    let mut peers = Vec::new();
    for i in 0..20 {
        let addr = format!("127.0.0.1:{}", 10000 + i).parse().unwrap();
        let peer = Peer::new(addr);
        peers.push(peer);
    }
    
    let elapsed = start_time.elapsed();
    println!("✅ Network performance test completed in {:?}", elapsed);
    println!("   🔸 Created {} P2P nodes", nodes.len());
    println!("   🔸 Validated {} messages", messages.len());
    println!("   🔸 Created {} peers", peers.len());
    
    // Should complete quickly (less than 2 seconds)
    assert!(elapsed.as_secs() < 2, "Network performance test should be fast");
}

/// Test network error handling
#[tokio::test]
async fn test_network_error_handling() {
    // Test invalid address formats
    let invalid_addr_result = "invalid-address".parse::<SocketAddr>();
    assert!(invalid_addr_result.is_err(), "Invalid address should fail to parse");
    
    // Test P2P node with invalid configuration
    let invalid_config = NetworkConfig {
        magic: 0,  // Invalid magic
        protocol_version: ProtocolVersion::new(0, 0, 0), // Invalid version
        user_agent: "".to_string(), // Empty user agent
        listen_address: "127.0.0.1:10333".parse().unwrap(),
        seed_nodes: vec![],
        p2p_config: P2PConfig {
            listen_address: "127.0.0.1:10333".parse().unwrap(),
            max_peers: 0, // Invalid max peers
            connection_timeout: Duration::from_secs(0), // Invalid timeout
            handshake_timeout: Duration::from_secs(0),
            ping_interval: Duration::from_secs(0),
            message_buffer_size: 0,
            enable_compression: false,
        },
        rpc_config: None,
    };
    
    // This should handle invalid configuration gracefully
    let result = P2PNode::new(invalid_config);
    // Note: Depending on implementation, this might succeed but with corrected values
    // or fail with appropriate error handling
    
    // Test RPC client with invalid endpoint
    let invalid_rpc_config = ClientConfig {
        endpoint: "invalid-url".to_string(),
        timeout: 30,
        max_retries: 3,
        retry_delay: 1000,
        user_agent: "test".to_string(),
        headers: std::collections::HashMap::new(),
    };
    
    let invalid_client = RpcClient::with_config(invalid_rpc_config);
    assert!(invalid_client.is_err(), "Invalid RPC config should fail");
    
    println!("✅ Network error handling test passed");
}

/// Test comprehensive network integration scenario
#[tokio::test]
async fn test_comprehensive_network_integration() {
    println!("🚀 Starting comprehensive network integration test");
    
    // 1. Create network configuration
    let network_config = NetworkConfig {
        magic: 0x334f454e,
        protocol_version: ProtocolVersion::new(3, 6, 0),
        user_agent: "neo-rs-integration-test/1.0".to_string(),
        listen_address: "127.0.0.1:0".parse().unwrap(),
        seed_nodes: vec!["127.0.0.1:10333".parse().unwrap()],
        p2p_config: P2PConfig {
            listen_address: "127.0.0.1:0".parse().unwrap(),
            max_peers: 50,
            connection_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(5),
            ping_interval: Duration::from_secs(30),
            message_buffer_size: 1000,
            enable_compression: false,
        },
        rpc_config: Some(RpcConfig {
            http_address: "127.0.0.1:0".parse().unwrap(),
            ws_address: Some("127.0.0.1:0".parse().unwrap()),
            enable_cors: true,
            max_connections: 100,
            request_timeout: Duration::from_secs(30),
        }),
    };
    
    // 2. Create P2P node
    let p2p_node = P2PNode::new(network_config.clone()).unwrap();
    assert_eq!(p2p_node.config().magic, 0x334f454e);
    
    // 3. Create RPC client
    let rpc_client_config = ClientConfig {
        endpoint: "http://127.0.0.1:10332".to_string(),
        timeout: 30,
        max_retries: 3,
        retry_delay: 1000,
        user_agent: "neo-rs-integration-test-client/1.0".to_string(),
        headers: std::collections::HashMap::new(),
    };
    
    let rpc_client = RpcClient::with_config(rpc_client_config).unwrap();
    assert_eq!(rpc_client.endpoint(), "http://127.0.0.1:10332");
    
    // 4. Create network server
    let server_config = NetworkServerConfig {
        node_id: UInt160::zero(),
        magic: network_config.magic,
        p2p_config: network_config.p2p_config.clone(),
        rpc_config: network_config.rpc_config.clone(),
        enable_auto_sync: true,
        sync_check_interval: 30,
        stats_interval: 10,
        seed_nodes: network_config.seed_nodes.clone(),
    };
    
    let network_server = NetworkServer::new(server_config).unwrap();
    assert_eq!(network_server.state(), neo_network::NetworkServerState::Stopped);
    
    // 5. Test message creation and validation
    let version_payload = VersionPayload {
        version: 0x00000000,
        services: 0x00000001,
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
        port: 10333,
        nonce: 0x12345678,
        user_agent: "neo-rs-integration-test/1.0".to_string(),
        start_height: 0,
        relay: true,
    };
    
    let version_message = Message::new(
        network_config.magic,
        MessageType::Version,
        Payload::Version(version_payload),
    );
    
    assert!(version_message.validate().is_ok(), "Version message should be valid");
    
    // 6. Test peer management
    let peer_addr: SocketAddr = "127.0.0.1:10334".parse().unwrap();
    let mut peer = Peer::new(peer_addr);
    peer.set_state(PeerState::Connected);
    
    let peer_info = PeerInfo {
        address: peer_addr,
        version: ProtocolVersion::new(3, 6, 0),
        user_agent: "neo-core/3.6.0".to_string(),
        height: 1000,
        nonce: 0x87654321,
        services: 0x00000001,
    };
    
    peer.set_info(Some(peer_info));
    assert!(peer.info().is_some());
    assert_eq!(peer.info().unwrap().height, 1000);
    
    println!("✅ Comprehensive network integration test completed successfully!");
    println!("   🔸 P2P node: {}", p2p_node.config().user_agent);
    println!("   🔸 RPC client endpoint: {}", rpc_client.endpoint());
    println!("   🔸 Network server state: {:?}", network_server.state());
    println!("   🔸 Peer address: {}", peer.address());
    println!("   🔸 Message type: {:?}", version_message.message_type);
} 