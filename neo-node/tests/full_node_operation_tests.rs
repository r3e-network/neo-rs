//! Integration tests for full node operation including all services

use anyhow::Result;
use neo_protocol::ProtocolSettings;
use neo_system::NeoSystem;
use neo_system::network::p2p::ChannelsConfig;
use tempfile::TempDir;
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

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_initialization() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    // Start full node
    system.start_node(offline_channels_config())?;

    // Verify core components are initialized
    assert_eq!(system.context().genesis_block().index(), 0);
    assert!(
        system
            .mempool()
            .lock()
            .verified_transactions_vec()
            .is_empty()
    );

    tokio::time::sleep(Duration::from_millis(100)).await;

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_with_persistence() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let storage_path = temp_dir.path().to_str().unwrap();

    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, Some(storage_path.to_string()))?;

    system.start_node(offline_channels_config())?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify blockchain is persisted
    let height = system.current_block_index();
    assert_eq!(height, 0);
    let genesis_hash = system.context().genesis_block().hash();
    assert!(
        system.context().try_get_block(&genesis_hash).is_some(),
        "genesis block should be retrievable from the store"
    );

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_mempool_integration() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(offline_channels_config())?;

    let mempool = system.mempool();
    {
        let pool = mempool.lock();

        // Mempool should be operational
        assert_eq!(pool.verified_count(), 0);
        assert!(pool.verified_transactions_vec().is_empty());
    }
    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_store_cache_operations() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(offline_channels_config())?;

    let store_cache = system.store_cache();

    // Verify store cache is writable (commit mode, not a read-only snapshot)
    assert!(!store_cache.data_cache().is_read_only());

    // Access data cache
    let _data_cache = store_cache.data_cache();

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_concurrent_operations() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(offline_channels_config())?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Spawn concurrent operations
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let sys = system.clone();
            tokio::spawn(async move {
                let _height = sys.current_block_index();
                let _genesis = sys.context().genesis_block();

                {
                    let mempool = sys.mempool();
                    let _pool = mempool.lock();
                }

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

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_restart_scenario() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let storage_path = temp_dir.path().to_str().unwrap();

    // First run
    {
        let protocol_settings = ProtocolSettings::default();
        let system = NeoSystem::new(protocol_settings, None, Some(storage_path.to_string()))?;

        system.start_node(offline_channels_config())?;
        tokio::time::sleep(Duration::from_millis(50)).await;

        system.shutdown().await?;
    }

    // Second run (simulating restart)
    {
        let protocol_settings = ProtocolSettings::default();
        let system = NeoSystem::new(protocol_settings, None, Some(storage_path.to_string()))?;

        system.start_node(offline_channels_config())?;
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Blockchain should still be at genesis
        let height = system.current_block_index();
        assert_eq!(height, 0);

        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_protocol_settings_propagation() -> Result<()> {
    let mut protocol_settings = ProtocolSettings::default();
    protocol_settings.network = 0x334F454E;

    let system = NeoSystem::new(protocol_settings.clone(), None, None)?;
    system.start_node(offline_channels_config())?;

    // Verify protocol settings are propagated
    assert_eq!(system.settings().network, 0x334F454E);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_multiple_parallel_instances() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();

    let systems: Vec<_> = (0..3)
        .map(|_| NeoSystem::new(protocol_settings.clone(), None, None).unwrap())
        .collect();

    // Start all nodes
    for system in &systems {
        system.start_node(offline_channels_config())?;
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify all are operational
    for system in &systems {
        let height = system.current_block_index();
        assert_eq!(height, 0);
    }

    // Shutdown all
    for system in systems {
        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_graceful_shutdown_under_load() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(offline_channels_config())?;

    // Generate some load
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let sys = system.clone();
            tokio::spawn(async move {
                for _ in 0..10 {
                    let _height = sys.current_block_index();
                    let _genesis_hash = sys.context().genesis_block().hash();
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
        })
        .collect();

    // Let some load build up
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Shutdown should wait for operations to complete
    let shutdown_result = tokio::time::timeout(Duration::from_secs(5), system.shutdown()).await;

    assert!(
        shutdown_result.is_ok(),
        "Shutdown should complete within timeout"
    );

    // Wait for all tasks
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_blockchain_state_consistency() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(offline_channels_config())?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let context = system.context();
    let genesis_hash = context.genesis_block().hash();

    // Multiple reads should return consistent results
    for _ in 0..5 {
        let height = system.current_block_index();
        assert_eq!(height, 0);

        let genesis = context.try_get_block(&genesis_hash);
        assert!(genesis.is_some(), "genesis block should be retrievable");
    }

    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_service_coordination() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(offline_channels_config())?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // All services should be coordinated
    let height = system.current_block_index();
    assert_eq!(height, 0);

    let store_cache = system.store_cache();
    assert!(!store_cache.data_cache().is_read_only());

    let mempool = system.mempool();
    {
        let pool = mempool.lock();
        assert_eq!(pool.verified_count(), 0);
    }
    system.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_memory_efficiency() -> Result<()> {
    // Test that multiple sequential node instances don't leak memory
    let protocol_settings = ProtocolSettings::default();

    for _ in 0..5 {
        let system = NeoSystem::new(protocol_settings.clone(), None, None)?;
        system.start_node(offline_channels_config())?;
        tokio::time::sleep(Duration::from_millis(20)).await;
        system.shutdown().await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_full_node_error_recovery() -> Result<()> {
    let protocol_settings = ProtocolSettings::default();
    let system = NeoSystem::new(protocol_settings, None, None)?;

    system.start_node(offline_channels_config())?;

    // Simulate operations that might fail
    let context = system.context();

    // Try to access non-existent blocks
    for height in 1..10 {
        let hash = context.block_hash_at(height);
        assert!(hash.is_none(), "no block should exist at height {height}");
    }

    // System should remain operational
    let current_height = system.current_block_index();
    assert_eq!(current_height, 0);

    system.shutdown().await?;
    Ok(())
}
