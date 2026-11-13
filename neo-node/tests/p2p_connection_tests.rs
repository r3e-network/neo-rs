//! Integration tests for P2P networking and connections

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_system::NeoSystem;
use neo_system::network::p2p::ChannelsConfig;
use tokio::time::Duration;

/// Channels configuration that keeps the P2P layer dormant in tests:
/// no TCP listener and no desired peers.
fn offline_channels_config() -> ChannelsConfig {
    ChannelsConfig {
        tcp: None,
        min_desired_connections: 0,
        max_connections: 0,
        ..Default::default()
    }
}

/// Configuration with explicit connection bounds. The former per-channel
/// capacity fields (`task_manager`/`blockchain`/`remote_node`/`local_node`)
/// were replaced by the connection-limit based `ChannelsConfig`.
fn bounded_channels_config(min_connections: usize, max_connections: usize) -> ChannelsConfig {
    ChannelsConfig {
        min_desired_connections: min_connections,
        max_connections,
        ..offline_channels_config()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_system_initialization() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Start the node with P2P disabled (no actual network connections)
    system.start_node(offline_channels_config())?;

    // Verify system is running
    tokio::time::sleep(Duration::from_millis(100)).await;

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_channels_config() -> Result<()> {
    let channels_config = bounded_channels_config(16, 16);

    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(channels_config)?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_custom_channels() -> Result<()> {
    // Test with larger connection limits for high-throughput scenarios
    let channels_config = bounded_channels_config(64, 64);

    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(channels_config)?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_minimal_channels() -> Result<()> {
    // Test with minimal connection limits
    let channels_config = bounded_channels_config(4, 4);

    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(channels_config)?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_p2p_protocol_settings() -> Result<()> {
    let mut protocol_settings = ProtocolSettings::default();

    // Configure for testnet
    protocol_settings.network = 0x334F454E;
    protocol_settings.seed_list = vec![];

    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

    assert_eq!(system.settings().network, 0x334F454E);
    assert_eq!(system.settings().seed_list.len(), 0);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_p2p_seed_list_configuration() -> Result<()> {
    let mut protocol_settings = ProtocolSettings::default();

    // Add seed nodes (even though we won't connect in tests)
    protocol_settings.seed_list = vec![
        "seed1.neo.org:10333".to_string(),
        "seed2.neo.org:10333".to_string(),
    ];

    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

    assert_eq!(system.settings().seed_list.len(), 2);
    assert!(
        system
            .settings()
            .seed_list
            .contains(&"seed1.neo.org:10333".to_string())
    );

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_concurrent_node_startup() -> Result<()> {
    // Test multiple nodes can start concurrently (different instances)
    let protocol_settings = ProtocolSettings::default();

    let system1 = NeoSystem::new(protocol_settings.clone(), None, None)?;
    let system2 = NeoSystem::new(protocol_settings.clone(), None, None)?;

    system1.start_node(offline_channels_config())?;
    system2.start_node(offline_channels_config())?;

    tokio::time::sleep(Duration::from_millis(100)).await;

    system1.shutdown().await?;
    system2.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_node_restart() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Start node
    system.start_node(offline_channels_config())?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Shutdown
    system.shutdown().await?;

    // Note: In a real scenario, you'd create a new system to restart
    // This test verifies clean shutdown for restart scenarios
    Ok(())
}

#[tokio::test]
async fn test_p2p_network_magic_validation() -> Result<()> {
    let mut protocol_settings = ProtocolSettings::default();

    // Test different network magics
    let test_magics = vec![
        0x334F454E, // NEO3 mainnet
        0x3554454E, // Testnet
        0x4F454E,   // Custom
    ];

    for magic in test_magics {
        protocol_settings.network = magic;
        let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

        assert_eq!(system.settings().network, magic);
        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_connection_timeout_settings() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(offline_channels_config())?;

    // Verify system can handle quick shutdown (no hanging connections)
    tokio::time::timeout(Duration::from_secs(5), system.shutdown()).await??;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_multiple_sequential_startups() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();

    // Test multiple sequential node startups
    for _ in 0..3 {
        let system = NeoSystem::new(protocol_settings.clone(), None, None)?;
        system.start_node(offline_channels_config())?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_p2p_channels_independent() -> Result<()> {
    // Verify different channel configurations work independently
    let configs = vec![
        bounded_channels_config(8, 8),
        bounded_channels_config(16, 16),
        bounded_channels_config(32, 32),
    ];

    for config in configs {
        let protocol_settings = ProtocolSettings::default();
        let system = NeoSystem::new(protocol_settings, None, None)?;
        system.start_node(config)?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        system.shutdown().await?;
    }

    Ok(())
}
