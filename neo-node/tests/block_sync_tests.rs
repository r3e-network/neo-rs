//! Integration tests for block synchronization

use anyhow::Result;
use neo_primitives::UInt256;
use neo_protocol::ProtocolSettings;
use neo_system::NeoSystem;
use neo_system::network::p2p::ChannelsConfig;
use tokio::time::Duration;

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
async fn test_blockchain_genesis_block() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let context = system.context();

    // Genesis block should exist at height 0
    let height = system.current_block_index();
    assert_eq!(height, 0, "Blockchain should start at genesis");

    // Get genesis block
    let genesis = context.genesis_block();
    assert_eq!(genesis.index(), 0, "Genesis block should exist");
    let _tx_count = genesis.transactions.len();

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_height_tracking() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let initial_height = system.current_block_index();
    assert_eq!(initial_height, 0);

    // Verify height is consistent across multiple calls
    let height2 = system.current_block_index();
    assert_eq!(initial_height, height2);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_block_retrieval() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let context = system.context();
    let genesis_hash = context.genesis_block().hash();

    // Try to get genesis block
    let block = context.try_get_block(&genesis_hash);
    assert!(block.is_some());

    // Try to get non-existent block
    let non_existent_hash = UInt256::from_bytes(&[0x77; 32])?;
    let non_existent = context.try_get_block(&non_existent_hash);
    assert!(non_existent.is_none());

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_header_retrieval() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let context = system.context();

    // Get genesis header
    let headers = context.headers_from_index(0, 1);
    assert!(!headers.is_empty());

    if let Some(hdr) = headers.first() {
        assert_eq!(hdr.index(), 0);
    }

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blockchain_concurrent_reads() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let context = system.context();
    let genesis_hash = context.genesis_block().hash();

    // Spawn multiple concurrent read tasks
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let ctx = context.clone();
            let hash = genesis_hash;
            tokio::spawn(async move {
                let _height = ctx.current_block_index();
                let _block = ctx.try_get_block(&hash);
                let _headers = ctx.headers_from_index(0, 1);
            })
        })
        .collect();

    // Wait for all reads to complete
    for handle in handles {
        handle.await?;
    }

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_store_cache_integration() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let store_cache = system.store_cache();

    // Verify store cache is writable (commit mode, not a read-only snapshot)
    assert!(!store_cache.data_cache().is_read_only());

    let height = system.current_block_index();
    assert_eq!(height, 0);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_with_storage_backend() -> Result<()> {
    use tempfile::TempDir;

    let temp_dir = TempDir::new()?;
    let storage_path = temp_dir.path().to_str().unwrap();

    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, Some(storage_path.to_string()))?;

    let context = system.context();

    // Verify blockchain works with persistent storage
    let height = system.current_block_index();
    assert_eq!(height, 0);

    let genesis_hash = context.genesis_block().hash();
    let genesis = context.try_get_block(&genesis_hash);
    assert!(genesis.is_some());

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_genesis_hash() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let genesis = system.context().genesis_block();

    // Get genesis block and verify it has a valid hash
    let hash = genesis.hash();
    assert!(!hash.is_zero(), "Genesis block should have non-zero hash");

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_contains_block() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let context = system.context();

    // Get genesis hash
    let genesis_hash = context.genesis_block().hash();

    // Check if blockchain contains genesis block
    let contains = context.try_get_block(&genesis_hash).is_some();
    assert!(contains, "Blockchain should contain genesis block");

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_transaction_in_genesis() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    let genesis = system.context().genesis_block();
    let transactions = &genesis.transactions;

    // Genesis block may or may not have transactions depending on protocol settings
    // Just verify we can access them without error
    let _tx_count = transactions.len();

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_multiple_instances() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();

    // Create multiple independent blockchain instances
    let system1 = NeoSystem::new(protocol_settings.clone(), None, None)?;
    let system2 = NeoSystem::new(protocol_settings.clone(), None, None)?;

    // Both should have genesis
    assert_eq!(system1.current_block_index(), 0);
    assert_eq!(system2.current_block_index(), 0);

    system1.shutdown().await?;
    system2.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_blockchain_sync_readiness() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Start the node
    system.start_node(offline_channels_config())?;

    // Give it time to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    let height = system.current_block_index();

    // Should still be at genesis after initialization
    assert_eq!(height, 0);

    system.shutdown().await?;
    Ok(())
}
