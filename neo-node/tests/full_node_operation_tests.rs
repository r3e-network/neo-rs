//! Integration tests for full node operation including all services

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_system::NeoSystem;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::time::Duration;

#[tokio::test]
async fn test_full_node_initialization() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    // Start full node
    system.start_node(&None)?;

    // Verify core components are initialized
    assert!(system.blockchain().is_some());
    assert!(system.mempool().lock().get_all_verified().is_empty());

    tokio::time::sleep(Duration::from_millis(100)).await;

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_full_node_with_persistence() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let storage_path = temp_dir.path().to_str().unwrap();

    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(
        protocol_settings,
        Some(storage_path),
        None,
    )?);

    system.start_node(&None)?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify blockchain is persisted
    let blockchain = system.blockchain().as_ref().unwrap();
    let height = blockchain.height()?;
    assert_eq!(height, 0);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_full_node_mempool_integration() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    system.start_node(&None)?;

    let mempool = system.mempool();
    let pool = mempool.lock();

    // Mempool should be operational
    assert_eq!(pool.verified_count(), 0);
    assert!(pool.get_all_verified().is_empty());

    drop(pool);
    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_full_node_store_cache_operations() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    system.start_node(&None)?;

    let store_cache = system.store_cache();

    // Verify store cache is in commit mode
    assert!(store_cache.is_commit_mode());

    // Access data cache
    let _data_cache = store_cache.data_cache();

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_full_node_concurrent_operations() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    system.start_node(&None)?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Spawn concurrent operations
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let sys = system.clone();
            tokio::spawn(async move {
                let blockchain = sys.blockchain().as_ref().unwrap();
                let _height = blockchain.height().unwrap();

                let mempool = sys.mempool();
                let _pool = mempool.lock();

                let _store_cache = sys.store_cache();
            })
        })
        .collect();

    for handle in handles {
        handle.await?;
    }

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_full_node_restart_scenario() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let storage_path = temp_dir.path().to_str().unwrap();

    // First run
    {
        let protocol_settings = ProtocolSettings::default();
        let system = Arc::new(NeoSystem::new(
            protocol_settings,
            Some(storage_path),
            None,
        )?);

        system.start_node(&None)?;
        tokio::time::sleep(Duration::from_millis(50)).await;

        system.shutdown().await?;
    }

    // Second run (simulating restart)
    {
        let protocol_settings = ProtocolSettings::default();
        let system = Arc::new(NeoSystem::new(
            protocol_settings,
            Some(storage_path),
            None,
        )?);

        system.start_node(&None)?;
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Blockchain should still be at genesis
        let blockchain = system.blockchain().as_ref().unwrap();
        let height = blockchain.height()?;
        assert_eq!(height, 0);

        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test]
async fn test_full_node_protocol_settings_propagation() -> Result<()> {
    let mut protocol_settings = ProtocolSettings::default();
    protocol_settings.network = 0x334F454E;

    let system = Arc::new(NeoSystem::new(protocol_settings.clone(), None, None)?);
    system.start_node(&None)?;

    // Verify protocol settings are propagated
    assert_eq!(system.protocol_settings().network, 0x334F454E);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_full_node_multiple_parallel_instances() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();

    let systems: Vec<_> = (0..3)
        .map(|_| Arc::new(NeoSystem::new(protocol_settings.clone(), None, None).unwrap()))
        .collect();

    // Start all nodes
    for system in &systems {
        system.start_node(&None)?;
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify all are operational
    for system in &systems {
        let blockchain = system.blockchain().as_ref().unwrap();
        let height = blockchain.height()?;
        assert_eq!(height, 0);
    }

    // Shutdown all
    for system in systems {
        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test]
async fn test_full_node_graceful_shutdown_under_load() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    system.start_node(&None)?;

    // Generate some load
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let sys = system.clone();
            tokio::spawn(async move {
                for _ in 0..10 {
                    let blockchain = sys.blockchain().as_ref().unwrap();
                    let _ = blockchain.height();
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
        })
        .collect();

    // Let some load build up
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Shutdown should wait for operations to complete
    let shutdown_result = tokio::time::timeout(
        Duration::from_secs(5),
        system.shutdown()
    ).await;

    assert!(shutdown_result.is_ok(), "Shutdown should complete within timeout");

    // Wait for all tasks
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

#[tokio::test]
async fn test_full_node_blockchain_state_consistency() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    system.start_node(&None)?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let blockchain = system.blockchain().as_ref().unwrap();

    // Multiple reads should return consistent results
    for _ in 0..5 {
        let height = blockchain.height()?;
        assert_eq!(height, 0);

        let genesis = blockchain.get_block(0)?;
        assert!(genesis.is_some());
    }

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_full_node_service_coordination() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    system.start_node(&None)?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // All services should be coordinated
    let blockchain = system.blockchain().as_ref().unwrap();
    let _height = blockchain.height()?;

    let store_cache = system.store_cache();
    assert!(store_cache.is_commit_mode());

    let mempool = system.mempool();
    let pool = mempool.lock();
    assert_eq!(pool.verified_count(), 0);

    drop(pool);
    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn test_full_node_memory_efficiency() -> Result<()> {
    // Test that multiple sequential node instances don't leak memory
    let protocol_settings = ProtocolSettings::default();

    for _ in 0..5 {
        let system = Arc::new(NeoSystem::new(protocol_settings.clone(), None, None)?);
        system.start_node(&None)?;
        tokio::time::sleep(Duration::from_millis(20)).await;
        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test]
async fn test_full_node_error_recovery() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = Arc::new(NeoSystem::new(protocol_settings, None, None)?);

    system.start_node(&None)?;

    // Simulate operations that might fail
    let blockchain = system.blockchain().as_ref().unwrap();

    // Try to access non-existent blocks
    for height in 1..10 {
        let result = blockchain.get_block(height);
        assert!(result.is_ok());
        assert!(result?.is_none());
    }

    // System should remain operational
    let current_height = blockchain.height()?;
    assert_eq!(current_height, 0);

    system.shutdown().await?;
    Ok(())
}
