//! Integration tests for node startup and initialization

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_system::NeoSystem;
use neo_system::network::p2p::ChannelsConfig;
use tempfile::TempDir;

/// Channels configuration that keeps the P2P layer dormant in tests.
fn offline_channels_config() -> ChannelsConfig {
    ChannelsConfig {
        tcp: None,
        min_desired_connections: 0,
        max_connections: 0,
        ..Default::default()
    }
}

#[tokio::test]
async fn test_node_system_initialization() -> Result<()> {
    // Create a NeoSystem with default settings
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

    // Verify system is initialized
    assert_eq!(system.settings().network, protocol_settings.network);
    assert_eq!(system.context().genesis_block().index(), 0);
    assert!(
        system
            .mempool()
            .lock()
            .verified_transactions_vec()
            .is_empty()
    );

    // Verify height starts at 0
    let height = system.current_block_index();
    assert_eq!(height, 0, "New system should start at genesis");

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_node_system_with_storage() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let storage_path = temp_dir.path().to_str().unwrap();

    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(
        protocol_settings.clone(),
        None,
        Some(storage_path.to_string()),
    )?;

    // Verify storage is accessible
    assert_eq!(system.context().genesis_block().index(), 0);
    assert!(!system.store_cache().data_cache().is_read_only());

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_node_mempool_initialization() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

    let mempool = system.mempool();
    {
        let pool = mempool.lock();

        // Verify mempool starts empty
        assert_eq!(pool.verified_transactions_vec().len(), 0);
        assert_eq!(pool.verified_count(), 0);
    }
    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_multiple_system_instances() -> Result<()> {
    // Test that multiple independent systems can be created
    let protocol_settings = ProtocolSettings::default();

    let system1 = NeoSystem::new(protocol_settings.clone(), None, None)?;
    let system2 = NeoSystem::new(protocol_settings.clone(), None, None)?;

    // Both systems should be independent
    assert_eq!(system1.context().genesis_block().index(), 0);
    assert_eq!(system2.context().genesis_block().index(), 0);

    system1.shutdown().await?;
    system2.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_system_shutdown_graceful() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Keep a second handle: `shutdown` consumes the Arc.
    let second_handle = system.clone();

    // Verify shutdown completes without error
    system.shutdown().await?;

    // Shutdown should be idempotent
    second_handle.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_protocol_settings_validation() -> Result<()> {
    let mut protocol_settings = ProtocolSettings::default();

    // Test with custom network magic
    protocol_settings.network = 0x334F454E; // NEO3 mainnet
    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

    assert_eq!(system.settings().network, 0x334F454E);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_store_cache_initialization() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let store_cache = system.store_cache();

    // Verify store cache is accessible and writable (commit mode)
    assert!(!store_cache.data_cache().is_read_only());

    // Verify data cache is accessible
    let _data_cache = store_cache.data_cache();

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_height_tracking() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Genesis block should exist
    let height = system.current_block_index();
    assert_eq!(height, 0);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_system_start_services() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Start system services
    system.start_node(offline_channels_config())?;

    // Give services time to initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_system_operations() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Spawn multiple tasks accessing the system
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let system_clone = system.clone();
            tokio::spawn(async move {
                // Never hold the mempool lock across an await point.
                {
                    let mempool = system_clone.mempool();
                    let _pool = mempool.lock();
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            })
        })
        .collect();

    // Wait for all tasks
    for handle in handles {
        handle.await?;
    }

    system.shutdown().await?;
    Ok(())
}
