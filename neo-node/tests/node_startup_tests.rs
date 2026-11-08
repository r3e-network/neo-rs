//! Integration tests for node startup and initialization

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_system::NeoSystem;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_node_system_initialization() -> Result<()> {
    // Create a NeoSystem with default settings
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

    // Verify system is initialized
    assert_eq!(system.protocol_settings().network, protocol_settings.network);
    assert!(system.blockchain().is_some());
    assert!(system.mempool().lock().get_all_verified().is_empty());

    // Verify height starts at 0
    let height = system
        .blockchain()
        .as_ref()
        .unwrap()
        .height()
        .unwrap_or(0);
    assert_eq!(height, 0, "New system should start at genesis");

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_node_system_with_storage() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let storage_path = temp_dir.path().to_str().unwrap();

    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings.clone(), Some(storage_path), None)?;

    // Verify storage is accessible
    assert!(system.blockchain().is_some());
    assert!(system.store_cache().is_commit_mode());

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_node_mempool_initialization() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

    let mempool = system.mempool();
    let pool = mempool.lock();

    // Verify mempool starts empty
    assert_eq!(pool.get_all_verified().len(), 0);
    assert_eq!(pool.verified_count(), 0);

    drop(pool);
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
    assert!(system1.blockchain().is_some());
    assert!(system2.blockchain().is_some());

    system1.shutdown().await?;
    system2.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_system_shutdown_graceful() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Verify shutdown completes without error
    system.shutdown().await?;

    // Shutdown should be idempotent
    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_protocol_settings_validation() -> Result<()> {
    let mut protocol_settings = ProtocolSettings::default();

    // Test with custom network magic
    protocol_settings.network = 0x334F454E; // NEO3 mainnet
    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;

    assert_eq!(system.protocol_settings().network, 0x334F454E);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_store_cache_initialization() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let store_cache = system.store_cache();

    // Verify store cache is accessible
    assert!(store_cache.is_commit_mode());

    // Verify data cache is accessible
    let _data_cache = store_cache.data_cache();

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_height_tracking() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let blockchain = system.blockchain().as_ref().unwrap();

    // Genesis block should exist
    let height = blockchain.height()?;
    assert_eq!(height, 0);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_system_start_services() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    // Start system services
    system.start_node(&None)?;

    // Give services time to initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_concurrent_system_operations() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    // Spawn multiple tasks accessing the system
    let handles: Vec<_> = (0..5)
        .map(|_| {
            let system_clone = system.clone();
            tokio::spawn(async move {
                let _mempool = system_clone.mempool().lock();
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
