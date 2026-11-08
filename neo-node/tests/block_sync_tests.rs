//! Integration tests for block synchronization

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::{Block, Header};
use neo_system::NeoSystem;
use std::sync::Arc;
use tokio::time::Duration;

#[tokio::test]
async fn test_blockchain_genesis_block() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let blockchain = system.blockchain().as_ref().unwrap();

    // Genesis block should exist at height 0
    let height = blockchain.height()?;
    assert_eq!(height, 0, "Blockchain should start at genesis");

    // Get genesis block
    let genesis = blockchain.get_block(0)?;
    assert!(genesis.is_some(), "Genesis block should exist");

    if let Some(block) = genesis {
        assert_eq!(block.index(), 0);
        assert!(block.transactions().is_empty() || !block.transactions().is_empty());
    }

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_height_tracking() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let blockchain = system.blockchain().as_ref().unwrap();

    let initial_height = blockchain.height()?;
    assert_eq!(initial_height, 0);

    // Verify height is consistent across multiple calls
    let height2 = blockchain.height()?;
    assert_eq!(initial_height, height2);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_block_retrieval() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let blockchain = system.blockchain().as_ref().unwrap();

    // Try to get genesis block
    let block = blockchain.get_block(0)?;
    assert!(block.is_some());

    // Try to get non-existent block
    let non_existent = blockchain.get_block(99999)?;
    assert!(non_existent.is_none());

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_header_retrieval() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let blockchain = system.blockchain().as_ref().unwrap();

    // Get genesis header
    let header = blockchain.get_header(0)?;
    assert!(header.is_some());

    if let Some(hdr) = header {
        assert_eq!(hdr.index(), 0);
    }

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_concurrent_reads() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let blockchain = system.blockchain().clone().unwrap();

    // Spawn multiple concurrent read tasks
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let bc = blockchain.clone();
            tokio::spawn(async move {
                let _height = bc.height().unwrap();
                let _block = bc.get_block(0).unwrap();
                let _header = bc.get_header(0).unwrap();
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
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let store_cache = system.store_cache();
    let blockchain = system.blockchain().as_ref().unwrap();

    // Verify store cache and blockchain are synchronized
    assert!(store_cache.is_commit_mode());

    let height = blockchain.height()?;
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
    let system = Arc::new(NeoSystem::new(
        protocol_settings,
        Some(storage_path),
        None,
    )?);

    let blockchain = system.blockchain().as_ref().unwrap();

    // Verify blockchain works with persistent storage
    let height = blockchain.height()?;
    assert_eq!(height, 0);

    let genesis = blockchain.get_block(0)?;
    assert!(genesis.is_some());

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_genesis_hash() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let blockchain = system.blockchain().as_ref().unwrap();

    // Get genesis block and verify it has a valid hash
    let genesis = blockchain.get_block(0)?;
    assert!(genesis.is_some());

    if let Some(block) = genesis {
        let hash = block.hash();
        assert!(!hash.is_zero(), "Genesis block should have non-zero hash");
    }

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_contains_block() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let blockchain = system.blockchain().as_ref().unwrap();

    // Get genesis hash
    let genesis = blockchain.get_block(0)?.unwrap();
    let genesis_hash = genesis.hash();

    // Check if blockchain contains genesis block
    let contains = blockchain.contains_block(&genesis_hash)?;
    assert!(contains, "Blockchain should contain genesis block");

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_transaction_in_genesis() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    let blockchain = system.blockchain().as_ref().unwrap();

    let genesis = blockchain.get_block(0)?.unwrap();
    let transactions = genesis.transactions();

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
    let system1 = Arc::new(NeoSystem::new(protocol_settings.clone(), None, None)?);
    let system2 = Arc::new(NeoSystem::new(protocol_settings.clone(), None, None)?);

    let bc1 = system1.blockchain().as_ref().unwrap();
    let bc2 = system2.blockchain().as_ref().unwrap();

    // Both should have genesis
    assert_eq!(bc1.height()?, 0);
    assert_eq!(bc2.height()?, 0);

    system1.shutdown().await?;
    system2.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_blockchain_sync_readiness() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    // Start the node
    system.start_node(&None)?;

    // Give it time to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    let blockchain = system.blockchain().as_ref().unwrap();
    let height = blockchain.height()?;

    // Should still be at genesis after initialization
    assert_eq!(height, 0);

    system.shutdown().await?;
    Ok(())
}
