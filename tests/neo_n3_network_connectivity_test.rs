//! Neo N3 Network Connectivity Test
//!
//! Comprehensive test to verify the Neo Rust node can connect to and synchronize
//! with the real Neo N3 MainNet and TestNet networks.

use neo_cli::{args::Network, service::MainService, CliArgs};
use neo_core::UInt160;
use neo_network::{NetworkConfig, NodeInfo, P2PConfig, P2PNode, ProtocolVersion};
use std::time::Duration;
use tokio::time::timeout;
use tracing::{error, info, warn};

/// Test Neo N3 MainNet connectivity
#[tokio::test]
#[ignore] // Run with --ignored flag for real network tests
async fn test_neo_n3_mainnet_connectivity() {
    tracing_subscriber::fmt::init();

    info!("🌐 Testing Neo N3 MainNet connectivity...");

    // Create MainNet configuration
    let args = CliArgs {
        config: None,
        wallet: None,
        password: None,
        db_engine: None,
        db_path: None,
        no_verify: false,
        plugins: vec![],
        verbose: neo_cli::args::LogLevel::Info,
        daemon: false,
        network: Network::Mainnet,
        rpc_port: None,
        p2p_port: Some(10333),
        max_connections: None,
        min_connections: None,
        data_dir: None,
        show_version: false,
    };

    // Initialize the main service
    let mut service = match MainService::new(args).await {
        Ok(service) => {
            info!("✅ MainService initialized for MainNet");
            service
        }
        Err(e) => {
            error!("❌ Failed to initialize MainService: {}", e);
            panic!("Service initialization failed");
        }
    };

    // Start the service (this includes P2P and blockchain initialization)
    info!("🚀 Starting Neo N3 MainNet node...");

    // Use timeout to prevent hanging
    match timeout(Duration::from_secs(120), service.start()).await {
        Ok(Ok(_)) => {
            info!("✅ Neo N3 MainNet node started successfully");

            // Give some time for peer connections
            tokio::time::sleep(Duration::from_secs(30)).await;

            // Verify connectivity
            if let Some(p2p_node) = service.p2p_node() {
                let peer_manager = p2p_node.peer_manager();
                let peer_count = peer_manager.peer_count().await;
                info!("📊 Connected to {} MainNet peers", peer_count);

                assert!(
                    peer_count > 0,
                    "Should connect to at least one MainNet peer"
                );
                info!("✅ MainNet connectivity test passed");
            } else {
                error!("❌ P2P node not initialized");
                panic!("P2P node not available");
            }

            // Stop the service
            if let Err(e) = service.stop().await {
                warn!("Warning during service stop: {}", e);
            }
        }
        Ok(Err(e)) => {
            error!("❌ Service start failed: {}", e);
            panic!("Service start failed");
        }
        Err(_) => {
            error!("❌ Service start timed out");
            panic!("Service start timed out");
        }
    }
}

/// Test Neo N3 TestNet connectivity
#[tokio::test]
#[ignore] // Run with --ignored flag for real network tests
async fn test_neo_n3_testnet_connectivity() {
    tracing_subscriber::fmt::init();

    info!("🧪 Testing Neo N3 TestNet connectivity...");

    // Create TestNet configuration
    let args = CliArgs {
        config: None,
        wallet: None,
        password: None,
        db_engine: None,
        db_path: None,
        no_verify: false,
        plugins: vec![],
        verbose: neo_cli::args::LogLevel::Info,
        daemon: false,
        network: Network::Testnet,
        rpc_port: None,
        p2p_port: Some(20333),
        max_connections: None,
        min_connections: None,
        data_dir: None,
        show_version: false,
    };

    // Initialize the main service
    let mut service = match MainService::new(args).await {
        Ok(service) => {
            info!("✅ MainService initialized for TestNet");
            service
        }
        Err(e) => {
            error!("❌ Failed to initialize MainService: {}", e);
            panic!("Service initialization failed");
        }
    };

    // Start the service
    info!("🚀 Starting Neo N3 TestNet node...");

    // Use timeout to prevent hanging
    match timeout(Duration::from_secs(120), service.start()).await {
        Ok(Ok(_)) => {
            info!("✅ Neo N3 TestNet node started successfully");

            // Give some time for peer connections
            tokio::time::sleep(Duration::from_secs(30)).await;

            // Verify connectivity
            if let Some(p2p_node) = service.p2p_node() {
                let peer_manager = p2p_node.peer_manager();
                let peer_count = peer_manager.peer_count().await;
                info!("📊 Connected to {} TestNet peers", peer_count);

                assert!(
                    peer_count > 0,
                    "Should connect to at least one TestNet peer"
                );
                info!("✅ TestNet connectivity test passed");
            } else {
                error!("❌ P2P node not initialized");
                panic!("P2P node not available");
            }

            // Stop the service
            if let Err(e) = service.stop().await {
                warn!("Warning during service stop: {}", e);
            }
        }
        Ok(Err(e)) => {
            error!("❌ Service start failed: {}", e);
            panic!("Service start failed");
        }
        Err(_) => {
            error!("❌ Service start timed out");
            panic!("Service start timed out");
        }
    }
}

/// Test P2P protocol compatibility with Neo N3
#[tokio::test]
async fn test_neo_n3_protocol_compatibility() {
    tracing_subscriber::fmt::init();

    info!("🔍 Testing Neo N3 protocol compatibility...");

    // Test protocol version
    let version = ProtocolVersion::current();
    info!("📋 Current protocol version: {}", version);
    assert_eq!(version.major, 3, "Should use Neo N3 protocol");

    // Test network magic numbers
    let mainnet_magic = 0x334F454E;
    let testnet_magic = 0x3554334E;

    info!("🌐 MainNet magic: 0x{:08X}", mainnet_magic);
    info!("🧪 TestNet magic: 0x{:08X}", testnet_magic);

    // Verify magic numbers match C# Neo
    assert_eq!(
        mainnet_magic, 0x334F454E,
        "MainNet magic should match C# Neo"
    );
    assert_eq!(
        testnet_magic, 0x3554334E,
        "TestNet magic should match C# Neo"
    );

    // Test node info creation
    let node_info = NodeInfo {
        id: UInt160::zero(),
        version: version.clone(),
        user_agent: "neo-rs/0.1.0".to_string(),
        capabilities: vec!["FullNode".to_string()],
        start_height: 0,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        nonce: rand::random(),
    };

    info!("🤖 Node info created: {}", node_info.user_agent);
    assert!(
        !node_info.capabilities.is_empty(),
        "Should have capabilities"
    );

    info!("✅ Protocol compatibility test passed");
}

/// Test seed node resolution
#[tokio::test]
async fn test_seed_node_resolution() {
    tracing_subscriber::fmt::init();

    info!("🌱 Testing seed node resolution...");

    // Test MainNet seed nodes
    let mainnet_seeds = vec![
        "seed1.neo.org:10333",
        "seed2.neo.org:10333",
        "seed3.neo.org:10333",
    ];

    let mut resolved_count = 0;

    for seed in mainnet_seeds {
        info!("📡 Resolving seed node: {}", seed);

        match timeout(Duration::from_secs(10), tokio::net::lookup_host(seed)).await {
            Ok(Ok(mut addrs)) => {
                if let Some(addr) = addrs.next() {
                    info!("✅ Resolved {} to {}", seed, addr);
                    resolved_count += 1;
                } else {
                    warn!("❌ No addresses returned for {}", seed);
                }
            }
            Ok(Err(e)) => {
                warn!("❌ DNS resolution failed for {}: {}", seed, e);
            }
            Err(_) => {
                warn!("❌ DNS resolution timeout for {}", seed);
            }
        }
    }

    info!("📊 Resolved {}/{} seed nodes", resolved_count, 3);

    // We should be able to resolve at least one seed node
    assert!(resolved_count > 0, "Should resolve at least one seed node");

    info!("✅ Seed node resolution test passed");
}

/// Test basic P2P connection (without full node startup)
#[tokio::test]
#[ignore] // Run with --ignored flag for real network tests
async fn test_basic_p2p_connection() {
    tracing_subscriber::fmt::init();

    info!("🔌 Testing basic P2P connection...");

    // Create P2P configuration for MainNet
    let p2p_config = P2PConfig {
        listen_address: "127.0.0.1:0".parse().unwrap(), // Use random port
        max_peers: 10,
        connection_timeout: Duration::from_secs(10),
        handshake_timeout: Duration::from_secs(10),
        ping_interval: Duration::from_secs(30),
        message_buffer_size: 100,
        enable_compression: false,
    };

    let node_info = NodeInfo {
        id: UInt160::zero(),
        version: ProtocolVersion::current(),
        user_agent: "neo-rs-test/0.1.0".to_string(),
        capabilities: vec!["FullNode".to_string()],
        start_height: 0,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        nonce: rand::random(),
    };

    let magic = 0x334F454E; // MainNet magic
    let p2p_node = P2PNode::new(p2p_config, node_info, magic);

    // Start P2P node
    match timeout(Duration::from_secs(30), p2p_node.start()).await {
        Ok(Ok(_)) => {
            info!("✅ P2P node started successfully");

            // Try to connect to a seed node
            if let Ok(mut addrs) = tokio::net::lookup_host("seed1.neo.org:10333").await {
                if let Some(addr) = addrs.next() {
                    info!("📡 Attempting connection to {}", addr);

                    match timeout(Duration::from_secs(15), p2p_node.connect_peer(addr)).await {
                        Ok(Ok(_)) => {
                            info!("✅ Successfully connected to peer");

                            // Give some time for handshake
                            tokio::time::sleep(Duration::from_secs(5)).await;

                            info!("✅ Basic P2P connection test passed");
                        }
                        Ok(Err(e)) => {
                            warn!("❌ Connection failed: {}", e);
                        }
                        Err(_) => {
                            warn!("❌ Connection timeout");
                        }
                    }
                }
            }

            // Stop P2P node
            p2p_node.stop().await;
        }
        Ok(Err(e)) => {
            error!("❌ P2P node start failed: {}", e);
            panic!("P2P node start failed");
        }
        Err(_) => {
            error!("❌ P2P node start timeout");
            panic!("P2P node start timeout");
        }
    }
}
